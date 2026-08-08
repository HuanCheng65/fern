//! 快照清单：一张快照是什么，写在哪，怎么读回来。
//!
//! ```text
//! backups/snapshots/<实例 id>/<拍摄时刻>.json.gz
//! ```
//!
//! 文件名就是快照 id，也就是拍摄时刻的 Unix 秒。这样按名字排序就是按时间
//! 排序，不需要读内容——列一屏快照因此只是一次 `read_dir`。
//!
//! **清单存在即快照完整。** 写的顺序是「所有对象都落地 → 最后写清单」，而清单
//! 本身也是临时文件加改名。所以不存在「引用了一个还没写完的对象」的清单。
//!
//! 清单是 gzip 过的 JSON：一万个文件的清单纯文本有几 MB，压完不到十分之一，
//! 而它每拍一张就要写一份。

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{LoaderKind, backup::select::Skipped};

/// 这一版清单的格式号。读到更高的就直说不认识，不猜着恢复。
pub const FORMAT: u32 = 1;

/// 为什么拍这一张。
///
/// 是个枚举而不是一句话：句子归界面（见 AGENTS.md 里的 i18n 那条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// 用户自己按的。默认永久保留。
    Manual,
    /// 改模组之前。整个功能里最有价值的那一张（§1）。
    BeforeModChange,
    /// 游戏正常退出之后，捕获这一次的成果。
    AfterSession,
    /// 启动之前，距上次快照太久了——兜住「上次退出时崩了没拍成」。
    BeforeLaunch,
    /// 恢复之前。否则恢复本身就是一次不可逆操作。
    BeforeRestore,
}

impl Reason {
    /// 界面上的文案 id 用这个。
    pub fn tag(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::BeforeModChange => "before-mod-change",
            Self::AfterSession => "after-session",
            Self::BeforeLaunch => "before-launch",
            Self::BeforeRestore => "before-restore",
        }
    }

    /// 全部取值。界面的文案表照着它检查有没有漏。
    pub const ALL: &'static [Reason] = &[
        Reason::Manual,
        Reason::BeforeModChange,
        Reason::AfterSession,
        Reason::BeforeLaunch,
        Reason::BeforeRestore,
    ];
}

/// 拍下这张快照时，这个实例是什么游戏版本、什么加载器。
///
/// 不能从别处推出来：实例描述记的是**现在**的版本，而快照要回答的恰恰是
/// 「当时是什么」——「更新加载器之后世界打不开」这个场景里两者不一样。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStamp {
    pub minecraft: String,
    #[serde(default)]
    pub loader: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
}

/// 一个文件在这张快照里的样子。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    /// 相对游戏目录，`/` 分隔。
    pub path: String,
    pub size: u64,
    /// 修改时刻，Unix 秒。它和 `size` 一起决定下一次拍摄要不要重新读这个
    /// 文件（§4）——这一条不是优化，是「每次退出都拍」成不成立的分水岭。
    pub mtime: u64,
    /// 内容在对象仓库里的 id。
    ///
    /// **是个数组而不是一个哈希**，虽然现在永远只有一个。留这个口子是因为
    /// 真要做增量，该做的不是通用 CDC，而是 mca 感知的分块——region 文件
    /// 本来就是 1024 个独立压缩的区块，按它自己的边界切最简单也最有效。
    /// 那时候加上去不用改格式。
    pub chunks: Vec<String>,
}

impl FileRecord {
    /// 这一版只写单块。多块的清单来自更新的版本，恢复时要说清楚而不是拼错。
    pub fn only_chunk(&self) -> Result<&str> {
        match self.chunks.as_slice() {
            [one] => Ok(one),
            _ => Err(anyhow!(
                "{} 使用了当前版本不支持的分块格式，请升级 Fern 后再恢复",
                self.path
            )),
        }
    }
}

/// 一个模组文件的身份。
///
/// 只记文件名和 sha1，因为**这是对象仓库坏掉之后唯一的退路**：拿 sha1 去
/// Modrinth 反查就能把 jar 重新下回来。sha256 在 `files` 里，但那个哈希在
/// 任何模组站上都查不到。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRecord {
    /// `mods/` 下面的文件名，禁用的带 `.disabled`。
    pub file: String,
    pub sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub instance: String,
    /// 拍的是哪个目录。共享布局下两个实例可能指着同一份存档，界面靠它说清
    /// 那是同一份。
    pub game_directory: PathBuf,
    pub taken_at: u64,
    pub reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub game: GameStamp,
    #[serde(default)]
    pub mods: Vec<ModRecord>,
    #[serde(default)]
    pub files: Vec<FileRecord>,
    #[serde(default)]
    pub skipped: Vec<Skipped>,
    /// 拍完复查时文件还在变。
    ///
    /// 与其假装它没事，不如标出来——一张可能不一致的快照仍然值得留着，但
    /// 用户有权在恢复它之前知道这件事（§5）。
    #[serde(default)]
    pub inconsistent: bool,
}

impl Manifest {
    /// 这张快照里的全部对象 id。回收用。
    pub fn objects(&self) -> impl Iterator<Item = &str> {
        self.files
            .iter()
            .flat_map(|file| file.chunks.iter().map(String::as_str))
    }

    /// 备份的内容一共多大（按原文件算，不是仓库里实际占的）。
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// `backups/snapshots/<实例>/`。
pub fn directory(backups: &Path, instance: &str) -> Result<PathBuf> {
    let id = crate::InstanceId::parse(instance).map_err(|error| anyhow!("{error}"))?;
    Ok(backups.join("snapshots").join(id.as_str()))
}

/// 一张快照的清单在哪。
pub fn path(backups: &Path, instance: &str, snapshot: &str) -> Result<PathBuf> {
    if !is_snapshot_id(snapshot) {
        return Err(anyhow!("不是一个快照 id：{snapshot}"));
    }
    Ok(directory(backups, instance)?.join(format!("{snapshot}.json.gz")))
}

/// 快照 id：十进制的拍摄时刻，同一秒内多拍一张就补一个 `-1`。
pub fn is_snapshot_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 24
        && id.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
        && id.bytes().next().is_some_and(|byte| byte.is_ascii_digit())
}

/// 这个实例有哪些快照，从新到旧。
///
/// 只读目录名，不读内容——列一屏快照不该把几十份清单都解压一遍。
pub fn ids(backups: &Path, instance: &str) -> Vec<String> {
    let Ok(directory) = directory(backups, instance) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let id = name.strip_suffix(".json.gz")?.to_owned();
            is_snapshot_id(&id).then_some(id)
        })
        .collect();
    // id 是定长的秒数，按字符串倒序就是从新到旧。
    ids.sort_by(|left, right| right.cmp(left));
    ids
}

/// 所有实例的所有快照。回收对象时要把它们全算上。
pub fn every(backups: &Path) -> Vec<(String, String)> {
    fs::read_dir(backups.join("snapshots"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .flat_map(|instance| {
            ids(backups, &instance)
                .into_iter()
                .map(move |id| (instance.clone(), id))
        })
        .collect()
}

/// 一次把所有清单引用到的对象收齐。
pub fn live_objects(backups: &Path) -> HashSet<String> {
    every(backups)
        .into_iter()
        .filter_map(|(instance, id)| path(backups, &instance, &id).ok())
        .filter_map(|path| read(&path).ok())
        .flat_map(|manifest| manifest.objects().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

/// 写一份清单。临时文件加改名，所以要么是完整的一份，要么根本不存在。
pub fn write(path: &Path, manifest: &Manifest) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} 没有上级目录", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("创建 {}", parent.display()))?;
    let temporary = parent.join(format!(".{}.writing", std::process::id()));

    let body = serde_json::to_vec(manifest).context("序列化快照清单")?;
    let mut encoder =
        flate2::write::GzEncoder::new(File::create(&temporary)?, flate2::Compression::default());
    encoder.write_all(&body)?;
    let file = encoder.finish()?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temporary, path).with_context(|| format!("写入 {}", path.display()))
}

/// 读一份清单。
pub fn read(path: &Path) -> Result<Manifest> {
    let file = File::open(path).with_context(|| format!("打开 {}", path.display()))?;
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(file)
        .read_to_end(&mut text)
        .with_context(|| format!("解压 {}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&text).with_context(|| format!("解析 {}", path.display()))?;
    if manifest.version > FORMAT {
        return Err(anyhow!(
            "这张快照由更新版本的 Fern 写入（格式 {}，当前版本支持 {FORMAT}）",
            manifest.version
        ));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            version: FORMAT,
            instance: "moss".to_owned(),
            game_directory: PathBuf::from("/tmp/moss/.minecraft"),
            taken_at: 1_786_152_000,
            reason: Reason::BeforeModChange,
            label: Some("装 Create 之前".to_owned()),
            game: GameStamp {
                minecraft: "1.20.1".to_owned(),
                loader: LoaderKind::NeoForge,
                loader_version: Some("47.1.3".to_owned()),
            },
            mods: vec![ModRecord {
                file: "create.jar".to_owned(),
                sha1: "abc".to_owned(),
            }],
            files: vec![FileRecord {
                path: "saves/家/level.dat".to_owned(),
                size: 4096,
                mtime: 1_786_151_000,
                chunks: vec!["a".repeat(64)],
            }],
            skipped: Vec::new(),
            inconsistent: false,
        }
    }

    #[test]
    fn a_manifest_survives_a_round_trip() {
        let root = std::env::temp_dir().join(format!("fern-manifest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let target = path(&root, "moss", "1786152000").expect("path");
        write(&target, &sample()).expect("write");
        assert_eq!(read(&target).expect("read"), sample());
        assert_eq!(ids(&root, "moss"), vec!["1786152000".to_owned()]);
        assert_eq!(
            every(&root),
            vec![("moss".to_owned(), "1786152000".to_owned())]
        );
        assert_eq!(live_objects(&root).len(), 1);
        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn snapshots_are_listed_newest_first() {
        let root = std::env::temp_dir().join(format!("fern-manifest-o-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for at in ["1786152000", "1786158000", "1786152000-1"] {
            let mut manifest = sample();
            manifest.taken_at = at.trim_end_matches("-1").parse().expect("parse");
            write(&path(&root, "moss", at).expect("path"), &manifest).expect("write");
        }
        assert_eq!(
            ids(&root, "moss"),
            vec!["1786158000", "1786152000-1", "1786152000"]
        );
        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn a_newer_format_is_refused_rather_than_guessed_at() {
        let root = std::env::temp_dir().join(format!("fern-manifest-v-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut manifest = sample();
        manifest.version = FORMAT + 1;
        let target = path(&root, "moss", "1786152000").expect("path");
        write(&target, &manifest).expect("write");
        assert!(read(&target).is_err());
        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn ids_from_the_interface_cannot_escape_the_snapshot_directory() {
        let root = PathBuf::from("/tmp/fern-not-used");
        for evil in ["../../x", "..", "", "a.json.gz", "/etc/passwd", "-1"] {
            assert!(path(&root, "moss", evil).is_err(), "{evil} 应当被拒绝");
        }
        assert!(path(&root, "../moss", "1786152000").is_err());
    }

    #[test]
    fn a_multi_chunk_record_is_refused_rather_than_restored_wrong() {
        let record = FileRecord {
            path: "saves/家/r.mca".to_owned(),
            size: 1,
            mtime: 1,
            chunks: vec!["a".repeat(64), "b".repeat(64)],
        };
        assert!(record.only_chunk().is_err());
    }
}
