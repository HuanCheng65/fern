//! 对象仓库：按内容存文件，一份内容只存一次。
//!
//! 一个对象就是一个文件的完整内容，名字是它的 sha256。于是「同一个 jar 出现
//! 在二十张快照里」和「出现在一张里」占的磁盘一样多——这正是模组文件能进快照
//! 的前提（见 docs/fern-backup-design.md §2.1）。
//!
//! 仓库全局共享，不按实例分。复制出来的实例、同一个整合包的两份安装、同一份
//! 配置，去重的收益都在跨实例这一层。
//!
//! ```text
//! backups/objects/ab/cdef…      原样存
//! backups/objects/ab/cdef….z    deflate 压过
//! backups/objects/.tmp/…        写到一半的
//! ```
//!
//! 压没压过看后缀，不在文件头上留字节。留字节的话对象内容就不等于文件内容，
//! 写时复制那条路（[`place`]）也就走不通了——它要求两边逐字节相同。
//!
//! **写入顺序保证完整性**：先写完所有对象，最后写清单。清单存在即快照完整；
//! 断电最坏留下几个没人引用的孤儿对象，由 [`Store::sweep`] 收走。哪怕真留下
//! 一个内容残缺的对象，取出时也会因为哈希对不上而被拦下——[`Store::extract`]
//! 从不把一个校验不过的文件写到目标位置。

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

/// 读写用的缓冲区。region 文件动辄几十 MB，一次一兆比一次八千字节少两个数量级
/// 的系统调用。
const BUFFER: usize = 1 << 20;

/// 压到原来的这个百分比以下才算值得。省不到就存原样——每次恢复都要解一遍的
/// 代价，换不回百分之几的磁盘。
const WORTH: u64 = 95;

/// 压过的对象的后缀。
const PACKED: &str = "z";

/// 这些格式内部已经压过，再压一遍只会更大，而且它们恰恰是量最大的那些。
///
/// region 文件里每个区块各自 zlib，jar 和 zip 不用说，`.dat` 与 `.nbt` 是
/// gzip 过的 NBT。
const ALREADY_COMPRESSED: &[&str] = &[
    "mca",
    "mcr",
    "jar",
    "zip",
    "png",
    "jpg",
    "jpeg",
    "webp",
    "ogg",
    "mp3",
    "gz",
    "xz",
    "zst",
    "bz2",
    "7z",
    "rar",
    "dat",
    "nbt",
    "litematic",
    "schem",
    "schematic",
];

/// 一次入库的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// 内容的 sha256，小写十六进制。
    pub id: String,
    /// **原文件**有多大。清单里记的是它，不是压完之后的大小——清单描述的是
    /// 游戏目录，不是仓库的内部账。
    pub bytes: u64,
    /// 这一次真的写了一份，而不是命中了已有的对象。
    pub added: bool,
}

/// 一次回收的结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    pub objects: usize,
    /// 收回了多少磁盘空间（对象在仓库里实际占的字节，压过的按压后算）。
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    objects: PathBuf,
}

impl Store {
    /// `backups/` 下面那一份。
    pub fn at(backups: &Path) -> Self {
        Self {
            objects: backups.join("objects"),
        }
    }

    /// 把一个文件的内容存进来，返回它的 id。
    ///
    /// 内容已经有了就什么都不做——这是常态：一次游戏只动几个 region 文件，
    /// 其余几千个都会走到这一条返回上。
    pub fn put(&self, source: &Path) -> Result<Stored> {
        let (id, bytes) = digest(source).with_context(|| format!("读取 {}", source.display()))?;
        if self.locate(&id).is_some() {
            return Ok(Stored {
                id,
                bytes,
                added: false,
            });
        }

        let bucket = self.objects.join(&id[..2]);
        fs::create_dir_all(&bucket).with_context(|| format!("创建 {}", bucket.display()))?;
        let temporary = self.temporary()?;

        let mut target = bucket.join(&id[2..]);
        let packed = compressible(source) && squeeze(source, &temporary, bytes)?;
        if packed {
            target.set_extension(PACKED);
        } else {
            place(source, &temporary)
                .with_context(|| format!("复制 {} 到仓库", source.display()))?;
        }

        // 另一个线程可能刚好也在存同一份内容。它先落地就用它的，两份内容
        // 反正一模一样。
        if self.locate(&id).is_some() {
            let _ = fs::remove_file(&temporary);
            return Ok(Stored {
                id,
                bytes,
                added: false,
            });
        }
        fs::rename(&temporary, &target).with_context(|| format!("写入 {}", target.display()))?;
        Ok(Stored {
            id,
            bytes,
            added: true,
        })
    }

    /// 仓库里有没有这份内容。
    pub fn has(&self, id: &str) -> bool {
        self.locate(id).is_some()
    }

    /// 把一个对象取出到指定路径。
    ///
    /// 边写边算哈希，**对不上就不改动目标位置**——写到临时文件，校验通过才
    /// 改名过去。一个恢复出来打不开的世界比没有备份更糟，所以这里宁可报错。
    pub fn extract(&self, id: &str, destination: &Path) -> Result<()> {
        let object = self
            .locate(id)
            .ok_or_else(|| anyhow!("备份中没有这份内容（{id}）"))?;
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow!("{} 没有上级目录", destination.display()))?;
        fs::create_dir_all(parent).with_context(|| format!("创建 {}", parent.display()))?;
        let temporary = parent.join(scratch_name(".fern-restore"));

        let source = File::open(&object).with_context(|| format!("打开 {}", object.display()))?;
        let mut reader: Box<dyn Read> = if object.extension().is_some_and(|it| it == PACKED) {
            Box::new(flate2::read::DeflateDecoder::new(source))
        } else {
            Box::new(source)
        };

        let outcome = (|| -> io::Result<String> {
            let mut file = File::create(&temporary)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; BUFFER];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                file.write_all(&buffer[..read])?;
            }
            file.sync_all()?;
            Ok(hex(&hasher.finalize()))
        })();

        match outcome {
            Ok(actual) if actual == id => fs::rename(&temporary, destination)
                .with_context(|| format!("写入 {}", destination.display())),
            Ok(_) => {
                let _ = fs::remove_file(&temporary);
                Err(anyhow!(
                    "备份中这份内容已损坏，{} 未被改动",
                    destination.display()
                ))
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(anyhow::Error::new(error).context(format!("恢复 {}", destination.display())))
            }
        }
    }

    /// 一个对象在仓库里实际占多少字节，压过的按压后算。
    ///
    /// 「快照共占 3.2 GB」要的是这个数，不是原文件的大小。
    pub fn stored_bytes(&self, id: &str) -> u64 {
        self.locate(id)
            .and_then(|path| fs::metadata(path).ok())
            .map_or(0, |metadata| metadata.len())
    }

    /// 仓库现在占多少磁盘。
    pub fn bytes(&self) -> u64 {
        self.buckets()
            .flat_map(|bucket| fs::read_dir(bucket).into_iter().flatten().flatten())
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// 收掉没人引用的对象。
    ///
    /// 标记—清扫，不做引用计数：计数要在每次拍摄和删除时正确地加减，错一次就
    /// 是永久的泄漏或者永久的误删，而清单本来就是全部引用的完整记录。
    ///
    /// `grace` 之内新建的对象一律留着——正在写入的那一批还没有清单引用它们，
    /// 按引用判断会把它们当成孤儿删掉。
    pub fn sweep(&self, live: &HashSet<String>, grace: Duration) -> Result<Swept> {
        let mut swept = Swept::default();
        let cutoff = SystemTime::now()
            .checked_sub(grace)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        for bucket in self.buckets().collect::<Vec<_>>() {
            let name = bucket
                .file_name()
                .and_then(|it| it.to_str())
                .unwrap_or_default()
                .to_owned();
            for entry in fs::read_dir(&bucket).into_iter().flatten().flatten() {
                let path = entry.path();
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let id = format!(
                    "{name}{}",
                    path.file_stem().and_then(|it| it.to_str()).unwrap_or("")
                );
                if live.contains(&id) {
                    continue;
                }
                if metadata.modified().is_ok_and(|at| at > cutoff) {
                    continue;
                }
                if fs::remove_file(&path).is_ok() {
                    swept.objects += 1;
                    swept.bytes += metadata.len();
                }
            }
            // 空桶留着没有意义，但删失败也无所谓。
            let _ = fs::remove_dir(&bucket);
        }

        self.sweep_temporaries(cutoff);
        Ok(swept)
    }

    /// 断电或者进程被杀之后留下的半份对象。
    fn sweep_temporaries(&self, cutoff: SystemTime) {
        for entry in fs::read_dir(self.objects.join(".tmp"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|at| at <= cutoff);
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// 两位十六进制的分桶目录。`.tmp` 不在其中。
    fn buckets(&self) -> impl Iterator<Item = PathBuf> {
        fs::read_dir(&self.objects)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
            })
    }

    /// 这个 id 对应的对象在哪，压没压过都找得到。
    fn locate(&self, id: &str) -> Option<PathBuf> {
        // id 来自清单文件，而清单是能被手工编辑的。不验一遍就往路径里拼，
        // 一个写着 `../../` 的 id 就能读写仓库外面的东西。
        if !is_object_id(id) {
            return None;
        }
        let raw = self.objects.join(&id[..2]).join(&id[2..]);
        if raw.is_file() {
            return Some(raw);
        }
        let packed = raw.with_extension(PACKED);
        packed.is_file().then_some(packed)
    }

    fn temporary(&self) -> Result<PathBuf> {
        let directory = self.objects.join(".tmp");
        fs::create_dir_all(&directory).with_context(|| format!("创建 {}", directory.display()))?;
        Ok(directory.join(scratch_name("put")))
    }
}

/// 一个不会撞车的临时文件名：进程 id 分开不同进程，计数器分开同一进程里的线程。
fn scratch_name(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// 64 位小写十六进制，别的一律不认。
pub fn is_object_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// 边读边算 sha256，顺带把大小带回来——反正已经读完整一遍了。
fn digest(path: &Path) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    let mut bytes = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((hex(&hasher.finalize()), bytes))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// 值不值得试着压。
fn compressible(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|it| it.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    !ALREADY_COMPRESSED.contains(&extension.as_str())
}

/// 压到临时文件。省得不够多就返回 false，让调用方存原样。
fn squeeze(source: &Path, destination: &Path, original: u64) -> Result<bool> {
    let mut input = File::open(source).with_context(|| format!("打开 {}", source.display()))?;
    let mut encoder = flate2::write::DeflateEncoder::new(
        File::create(destination)?,
        flate2::Compression::default(),
    );
    io::copy(&mut input, &mut encoder)?;
    let output = encoder.finish()?;
    let packed = output.metadata()?.len();
    output.sync_all()?;
    Ok(packed.saturating_mul(100) < original.saturating_mul(WORTH))
}

/// 把源文件的内容放到目标位置：能共享数据块就共享，不能就复制。
///
/// **不用硬链接。** 硬链接和源文件是同一个 inode，而 Minecraft 会原地重写
/// region 文件——仓库里那份「已经存下来的内容」会跟着一起变，哈希对不上，
/// 快照就在没人察觉的时候坏掉了。写时复制没有这个问题：任何一边写入都会先
/// 断开共享，各拿各的。
///
/// 支持 reflink 的文件系统（btrfs、XFS、APFS）上第一次快照几乎不占空间。
/// 不支持就退回复制——只是不省空间，不影响正确性。
fn place(source: &Path, destination: &Path) -> io::Result<()> {
    let _ = fs::remove_file(destination);
    if reflink(source, destination).is_ok() {
        return Ok(());
    }
    let _ = fs::remove_file(destination);
    fs::copy(source, destination)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn reflink(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    /// `_IOW(0x94, 9, int)`。btrfs 与 XFS 的共享数据块 ioctl。
    const FICLONE: libc::c_ulong = 0x4004_9409;

    let source = File::open(source)?;
    let destination = File::create(destination)?;
    let result = unsafe { libc::ioctl(destination.as_raw_fd(), FICLONE, source.as_raw_fd()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn reflink(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let cstring = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "路径里有 NUL"))
    };
    let from = cstring(source)?;
    let to = cstring(destination)?;
    // clonefile 要求目标不存在。
    let _ = fs::remove_file(destination);
    let result = unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn reflink(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "这个平台没有写时复制",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fern-store-{tag}-{}-{}",
            std::process::id(),
            scratch_name("t")
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create scratch");
        root
    }

    fn write(path: &Path, body: &[u8]) -> PathBuf {
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, body).expect("write");
        path.to_path_buf()
    }

    #[test]
    fn the_same_content_is_stored_once() {
        let root = scratch("dedupe");
        let store = Store::at(&root);
        let one = write(&root.join("one.txt"), b"same content");
        let two = write(&root.join("two.txt"), b"same content");

        let first = store.put(&one).expect("put one");
        let second = store.put(&two).expect("put two");
        assert_eq!(first.id, second.id);
        assert!(first.added);
        // 第二份没有真的写进去——这正是模组 jar 能进快照的理由。
        assert!(!second.added);
        assert_eq!(second.bytes, 12);

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn extracting_reproduces_the_original_bytes() {
        let root = scratch("roundtrip");
        let store = Store::at(&root);
        // 一个压得动的（文本）和一个压不动的（后缀在名单里）。
        let text = write(&root.join("options.txt"), &b"fov:70\n".repeat(500));
        let region = write(&root.join("r.0.0.mca"), &[7u8; 4096]);

        for source in [&text, &region] {
            let stored = store.put(source).expect("put");
            let back = root.join("back").join(source.file_name().expect("name"));
            store.extract(&stored.id, &back).expect("extract");
            assert_eq!(
                fs::read(&back).expect("read back"),
                fs::read(source).expect("read source"),
                "{}",
                source.display()
            );
        }

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn a_corrupt_object_never_reaches_the_destination() {
        // 「恢复出来打不开的世界比没有备份更糟」——所以校验不过就一个字节都
        // 不写。
        let root = scratch("corrupt");
        let store = Store::at(&root);
        let source = write(&root.join("level.txt"), b"a world");
        let stored = store.put(&source).expect("put");

        let object = store.locate(&stored.id).expect("locate");
        fs::write(&object, b"tampered").expect("tamper");

        let destination = root.join("out").join("level.txt");
        fs::create_dir_all(destination.parent().unwrap()).expect("create out");
        fs::write(&destination, b"do not touch me").expect("seed");

        assert!(store.extract(&stored.id, &destination).is_err());
        assert_eq!(fs::read(&destination).expect("read"), b"do not touch me");
        // 临时文件也不许留下。
        let leftovers: Vec<_> = fs::read_dir(destination.parent().unwrap())
            .expect("read dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["level.txt".to_owned()]);

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn sweeping_keeps_what_is_referenced_and_what_is_new() {
        let root = scratch("sweep");
        let store = Store::at(&root);
        let live = store
            .put(&write(&root.join("a.txt"), b"referenced"))
            .expect("put a");
        let orphan = store
            .put(&write(&root.join("b.txt"), b"orphan"))
            .expect("put b");

        // 宽限期内的一律留着：正在写入的那一批还没有清单引用它们。
        let kept = store
            .sweep(&HashSet::from([live.id.clone()]), Duration::from_secs(3600))
            .expect("sweep");
        assert_eq!(kept.objects, 0);
        assert!(store.has(&orphan.id));

        let swept = store
            .sweep(&HashSet::from([live.id.clone()]), Duration::ZERO)
            .expect("sweep");
        assert_eq!(swept.objects, 1);
        assert!(store.has(&live.id));
        assert!(!store.has(&orphan.id));

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn ids_from_a_manifest_cannot_escape_the_store() {
        let root = scratch("escape");
        let store = Store::at(&root);
        for evil in [
            "../../settings.json",
            "..",
            "",
            "ZZ",
            &"g".repeat(64),
            &"A".repeat(64),
        ] {
            assert!(!store.has(evil), "{evil} 应当被拒绝");
            assert!(store.extract(evil, &root.join("out")).is_err(), "{evil}");
        }
        fs::remove_dir_all(root).expect("clean up");
    }
}
