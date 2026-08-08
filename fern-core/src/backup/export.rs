//! 导出：把东西带到别处去。
//!
//! 和备份共用「什么值得带走」的那份知识（[`select`](super::select)），但目标
//! 不同：**备份是给自己、在本机；导出是给别人或者另一台机器，必须自足。**
//!
//! | 格式 | 用途 |
//! |---|---|
//! | `.mrpack` | 互操作。Prism、HMCL、PCL 都认 |
//! | `.fernpack` | 完整搬迁。装得下就一定装得回去 |
//! | 世界 zip | 最常见的分享形式 |
//!
//! mrpack 里的模组是一串下载地址，别人下不到就是下不到——这既是格式的要求，
//! 也避开了分发别人的版权文件。所以 `.fernpack` 默认把 jar 打进去：它是那个
//! 「保证能用」的格式。
//!
//! **导出与导入用的是同一份格式知识**，解析那一半在 [`supply::modpack`](crate::supply::modpack)。

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    DataPaths, InstanceProfile, LoaderKind,
    backup::{select, sha1_of},
};

/// 一次导出的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: PathBuf,
    pub bytes: u64,
    /// 打进包里的文件数。
    pub files: usize,
    /// mrpack 专用：有几个模组是靠下载地址带走的（其余的落进 overrides）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked: Option<usize>,
}

/// `.fernpack` 里带什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contents {
    /// 存档。整包分享时通常不要，换机器时一定要。
    pub saves: bool,
    /// 模组 jar 本身。关掉之后这个包就不再「保证能用」了。
    pub mods: bool,
}

impl Default for Contents {
    fn default() -> Self {
        Self {
            saves: true,
            mods: true,
        }
    }
}

/// 把一个世界打成 zip。
///
/// 压缩包里的顶层就是那个世界的目录，和所有人分享存档的方式一致——解开就能
/// 直接放进 `saves/`。
pub fn world(
    paths: &DataPaths,
    instance_id: &str,
    save: &str,
    destination: &Path,
) -> Result<Exported> {
    if !is_safe_name(save) {
        return Err(anyhow!("不是一个世界名：{save}"));
    }
    let directory = game_directory(paths, instance_id)?.join("saves").join(save);
    if !directory.join("level.dat").is_file() {
        return Err(anyhow!("{save} 里没有 level.dat，它不是一个世界"));
    }

    let mut writer = open(destination)?;
    let mut files = 0;
    stow(&mut writer, &directory, save, &mut files)?;
    finish(writer, destination, files, None)
}

/// 完整搬迁包。
pub fn fernpack(
    paths: &DataPaths,
    instance_id: &str,
    contents: Contents,
    destination: &Path,
) -> Result<Exported> {
    let profile = crate::read_instance(paths, instance_id)?;
    let directory = game_directory(paths, instance_id)?;
    let (candidates, _) = select::scan(&directory);

    let mut writer = open(destination)?;
    let mut files = 0;

    // 实例本身是什么：版本、加载器、设置。没有它，解开的一堆文件不知道该
    // 用什么启动。
    write_entry(
        &mut writer,
        "instance.json",
        &serde_json::to_vec_pretty(&profile).context("序列化实例描述")?,
    )?;
    files += 1;

    for candidate in &candidates {
        if !wanted(&candidate.relative, contents) {
            continue;
        }
        copy_entry(
            &mut writer,
            &candidate.absolute,
            &format!("minecraft/{}", candidate.relative),
            candidate.size,
        )?;
        files += 1;
    }

    finish(writer, destination, files, None)
}

fn wanted(relative: &str, contents: Contents) -> bool {
    if select::save_of(relative).is_some() {
        return contents.saves;
    }
    if select::is_mod(relative) {
        return contents.mods;
    }
    true
}

/// Modrinth 整合包。
///
/// 模组不进包，进的是下载地址加哈希——这是格式的要求。地址靠 sha1 去 Modrinth
/// 反查，**而不是靠安装时记下来的来源**：后者对拖进来的 jar 无效，也会随时间
/// 和实际文件对不上，而哈希永远描述的是磁盘上真正的那一份。
///
/// 查不到的 jar 落进 `overrides/`，包会大一些，但仍然是个能用的包。
pub async fn mrpack(paths: &DataPaths, instance_id: &str, destination: &Path) -> Result<Exported> {
    let profile = crate::read_instance(paths, instance_id)?;
    let directory = game_directory(paths, instance_id)?;
    let (candidates, _) = select::scan(&directory);

    // 只有启用着的模组能进 files[]：格式里没有「装了但关着」这个概念，
    // 而把一个禁用的模组当成启用的装给别人，是在改变这个整合包的内容。
    let jars: Vec<&select::Candidate> = candidates
        .iter()
        .filter(|candidate| {
            select::is_mod(&candidate.relative) && candidate.relative.ends_with(".jar")
        })
        .collect();

    let mut hashes = Vec::with_capacity(jars.len());
    for jar in &jars {
        hashes.push(sha1_of(&jar.absolute)?);
    }
    // 查不到就整批当作查不到——一个包大一点，好过导出失败。
    let known = crate::supply::known_files(&hashes)
        .await
        .unwrap_or_default();

    let mut index = Index {
        format_version: 1,
        game: "minecraft".to_owned(),
        version_id: profile.game_version.clone(),
        name: profile.name.clone(),
        files: Vec::new(),
        dependencies: HashMap::new(),
    };
    index
        .dependencies
        .insert("minecraft".to_owned(), profile.game_version.clone());
    if let (Some(key), Some(loader)) = (loader_key(profile.loader), profile.loader_profile.as_ref())
    {
        index
            .dependencies
            .insert(key.to_owned(), loader.version.clone());
    }

    let mut writer = open(destination)?;
    let mut files = 0;
    let mut linked = 0;
    let mut embedded: Vec<&select::Candidate> = Vec::new();

    for (jar, sha1) in jars.iter().zip(&hashes) {
        match known.get(sha1) {
            Some(file) => {
                index.files.push(IndexFile {
                    path: jar.relative.clone(),
                    hashes: Hashes {
                        sha1: file.sha1.clone(),
                        sha512: file.sha512.clone(),
                    },
                    downloads: vec![file.url.clone()],
                    file_size: file.size,
                });
                linked += 1;
            }
            None => embedded.push(jar),
        }
    }

    write_entry(
        &mut writer,
        "modrinth.index.json",
        &serde_json::to_vec_pretty(&index).context("序列化 modrinth.index.json")?,
    )?;
    files += 1;

    // 配置、options.txt、资源包，加上查不到来源的那些 jar。存档不进整合包：
    // 整合包是「一套玩法」，不是「某个人的存档」。
    for candidate in &candidates {
        let carried = embedded
            .iter()
            .any(|jar| jar.relative == candidate.relative);
        if select::save_of(&candidate.relative).is_some() {
            continue;
        }
        if select::is_mod(&candidate.relative) && !carried {
            continue;
        }
        copy_entry(
            &mut writer,
            &candidate.absolute,
            &format!("overrides/{}", candidate.relative),
            candidate.size,
        )?;
        files += 1;
    }

    finish(writer, destination, files, Some(linked))
}

/// mrpack 的 `dependencies` 里加载器叫什么。格式定的，不是我们能选的。
fn loader_key(loader: LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Fabric => Some("fabric-loader"),
        LoaderKind::Quilt => Some("quilt-loader"),
        LoaderKind::NeoForge => Some("neoforge"),
        LoaderKind::Forge => Some("forge"),
        LoaderKind::Vanilla => None,
    }
}

// ——— modrinth.index.json 的形状 ———

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    files: Vec<IndexFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    path: String,
    hashes: Hashes,
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Debug, Serialize)]
struct Hashes {
    sha1: String,
    sha512: String,
}

// ——— zip 那一半 ———

fn open(destination: &Path) -> Result<ZipWriter<File>> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("创建 {}", parent.display()))?;
    }
    let file =
        File::create(destination).with_context(|| format!("创建 {}", destination.display()))?;
    Ok(ZipWriter::new(file))
}

fn finish(
    writer: ZipWriter<File>,
    destination: &Path,
    files: usize,
    linked: Option<usize>,
) -> Result<Exported> {
    writer.finish().context("写完压缩包")?;
    Ok(Exported {
        bytes: fs::metadata(destination).map(|it| it.len()).unwrap_or(0),
        path: destination.to_path_buf(),
        files,
        linked,
    })
}

fn options(size: u64) -> SimpleFileOptions {
    // 单个条目超过 4 GB 就要 zip64，否则写出来的包别人打不开。
    SimpleFileOptions::default().large_file(size > u64::from(u32::MAX))
}

fn write_entry(writer: &mut ZipWriter<File>, name: &str, body: &[u8]) -> Result<()> {
    writer
        .start_file(name, options(body.len() as u64))
        .with_context(|| format!("写入 {name}"))?;
    writer
        .write_all(body)
        .with_context(|| format!("写入 {name}"))
}

fn copy_entry(writer: &mut ZipWriter<File>, source: &Path, name: &str, size: u64) -> Result<()> {
    writer
        .start_file(name, options(size))
        .with_context(|| format!("写入 {name}"))?;
    let mut file = File::open(source).with_context(|| format!("打开 {}", source.display()))?;
    io::copy(&mut file, writer).with_context(|| format!("写入 {name}"))?;
    Ok(())
}

/// 把一棵目录树放进压缩包，`prefix` 是包里的顶层名字。
fn stow(
    writer: &mut ZipWriter<File>,
    directory: &Path,
    prefix: &str,
    files: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("读取 {}", directory.display()))?
        .flatten()
    {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // 锁文件对别人毫无意义，还会让某些启动器以为世界正被占用。
        if name == "session.lock" {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let inner = format!("{prefix}/{name}");
        if metadata.is_dir() {
            stow(writer, &entry.path(), &inner, files)?;
        } else if metadata.is_file() {
            copy_entry(writer, &entry.path(), &inner, metadata.len())?;
            *files += 1;
        }
    }
    Ok(())
}

fn game_directory(paths: &DataPaths, instance_id: &str) -> Result<PathBuf> {
    let profile: InstanceProfile = crate::read_instance(paths, instance_id)?;
    Ok(crate::instance::paths_for(paths, &profile).game_directory(profile.id.as_str()))
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_names_from_the_interface_cannot_escape_the_saves_directory() {
        for evil in ["../../x", "..", "", "a/b", "C:x"] {
            assert!(!is_safe_name(evil), "{evil} 应当被拒绝");
        }
        assert!(is_safe_name("家"));
    }

    #[test]
    fn a_fernpack_carries_what_was_asked_for() {
        let bare = Contents {
            saves: false,
            mods: false,
        };
        assert!(!wanted("saves/家/level.dat", bare));
        assert!(!wanted("mods/create.jar", bare));
        assert!(wanted("config/create.toml", bare));
        // 默认是「保证能用」的那一份：存档和 jar 都带上。
        assert!(wanted("saves/家/level.dat", Contents::default()));
        assert!(wanted("mods/create.jar", Contents::default()));
    }

    #[test]
    fn the_loader_key_matches_what_the_format_defines() {
        assert_eq!(loader_key(LoaderKind::Fabric), Some("fabric-loader"));
        assert_eq!(loader_key(LoaderKind::NeoForge), Some("neoforge"));
        assert_eq!(loader_key(LoaderKind::Vanilla), None);
    }
}
