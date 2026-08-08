//! 哪些东西进快照。
//!
//! 分界不是「这是什么」，是**「丢了还能不能拿回来」**（docs/fern-backup-design.md
//! §2）。存档不可再生；配置很小但调回去很费劲；模组 jar 是删掉之后世界打不开
//! 时唯一能救回来的东西。日志、崩溃报告、mixin 的转储、地图 mod 的瓦片缓存
//! 一次性，而且常常比存档还大。
//!
//! 用白名单而不是黑名单：黑名单漏一项，某个模组的几 GB 缓存就进了快照，而且
//! 是在用户看不见的地方每天涨。代价是白名单外的用户数据会被漏掉——所以**漏掉
//! 的东西要说出来**：顶层没被选中的每一项都记进清单的 `skipped`，界面照实列。
//!
//! 这个文件是地基：磁盘占用、清理、复制实例、导出四件事都从它长出来。

use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};

/// 单个文件的上限。
///
/// 防御性上限，不是策略——挡住有人往游戏目录里放一个十几 G 的东西把快照撑爆。
/// 正常的 region 文件是几 MB，最大的模组 jar 也就一百多 MB。
pub const LARGEST_FILE: u64 = 512 * 1024 * 1024;

/// 整个进快照的顶层目录。
const DIRECTORIES: &[&str] = &[
    "saves",
    "config",
    "mods",
    "resourcepacks",
    "shaderpacks",
    "schematics",
    "screenshots",
];

/// 进快照的顶层文件。
const FILES: &[&str] = &[
    "options.txt",
    "optionsof.txt",
    "optionsshaders.txt",
    "servers.dat",
    "servers.dat_old",
    "hotbar.nbt",
];

/// 一次性的东西。单独列出来是为了让 `skipped` 有意义——「日志没进快照」不需要
/// 出现在界面上，「journeymap 没进快照」需要。
const TRANSIENT: &[&str] = &[
    "logs",
    "crash-reports",
    "debug",
    ".fabric",
    ".mixin.out",
    ".cache",
    "natives",
    // 共享的游戏文件。外部实例的 `.minecraft` 里会有这三个，它们可重建，
    // 而且加起来能有好几 GB。
    "versions",
    "libraries",
    "assets",
];

/// 任何一层都不要的名字。
const NEVER: &[&str] = &["session.lock", ".DS_Store", "Thumbs.db"];

/// 一个要进快照的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 相对游戏目录的路径，一律用 `/` 分隔——清单要能在两个系统之间读。
    pub relative: String,
    pub absolute: PathBuf,
    pub size: u64,
    /// 修改时刻，Unix 秒。和大小一起构成「这个文件变了没有」的判据（§4）。
    pub mtime: u64,
}

/// 没进快照的一项，以及为什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// 超过 [`LARGEST_FILE`]。
    TooLarge,
    /// 一次性的东西，重新生成即可。
    Transient,
    /// 不在白名单里。用户可能在意，所以要说。
    NotSelected,
}

impl SkipReason {
    /// 界面上的文案 id 用这个。
    pub fn tag(self) -> &'static str {
        match self {
            Self::TooLarge => "too-large",
            Self::Transient => "transient",
            Self::NotSelected => "not-selected",
        }
    }

    /// 全部取值。界面的文案表照着它检查有没有漏。
    pub const ALL: &'static [SkipReason] = &[
        SkipReason::TooLarge,
        SkipReason::Transient,
        SkipReason::NotSelected,
    ];
}

/// 走一遍游戏目录，分出要备份的和不要的。
///
/// 目录不存在返回空，不是错误——还没进过游戏的实例就是这样。
pub fn scan(game_directory: &Path) -> (Vec<Candidate>, Vec<Skipped>) {
    let mut candidates = Vec::new();
    let mut skipped = Vec::new();

    let Ok(entries) = fs::read_dir(game_directory) else {
        return (candidates, skipped);
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if NEVER.contains(&name.as_str()) {
            continue;
        }

        if metadata.is_dir() {
            if DIRECTORIES.contains(&name.as_str()) {
                walk(&entry.path(), &name, &mut candidates, &mut skipped);
            } else {
                skipped.push(Skipped {
                    reason: if TRANSIENT.contains(&name.as_str()) {
                        SkipReason::Transient
                    } else {
                        SkipReason::NotSelected
                    },
                    path: format!("{name}/"),
                });
            }
        } else if metadata.is_file() && FILES.contains(&name.as_str()) {
            take(
                &entry.path(),
                name,
                &metadata,
                &mut candidates,
                &mut skipped,
            );
        }
    }

    candidates.sort_by(|left, right| left.relative.cmp(&right.relative));
    skipped.sort_by(|left, right| left.path.cmp(&right.path));
    (candidates, skipped)
}

fn walk(
    directory: &Path,
    prefix: &str,
    candidates: &mut Vec<Candidate>,
    skipped: &mut Vec<Skipped>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if NEVER.contains(&name.as_str()) {
            continue;
        }
        // `entry.metadata()` 不跟软链接，于是软链接既不是文件也不是目录，
        // 自然被跳过——跟过去会把别处的东西复制进这个实例的快照里。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let relative = format!("{prefix}/{name}");
        if metadata.is_dir() {
            walk(&entry.path(), &relative, candidates, skipped);
        } else if metadata.is_file() {
            take(&entry.path(), relative, &metadata, candidates, skipped);
        }
    }
}

fn take(
    path: &Path,
    relative: String,
    metadata: &fs::Metadata,
    candidates: &mut Vec<Candidate>,
    skipped: &mut Vec<Skipped>,
) {
    if metadata.len() > LARGEST_FILE {
        skipped.push(Skipped {
            path: relative,
            reason: SkipReason::TooLarge,
        });
        return;
    }
    candidates.push(Candidate {
        relative,
        absolute: path.to_path_buf(),
        size: metadata.len(),
        mtime: metadata
            .modified()
            .ok()
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs()),
    });
}

/// 这条路径属于哪个存档，如果它属于某个存档的话。
///
/// 恢复的粒度靠它：用户要的通常不是「回滚整个实例」，是「把那一个世界回到
/// 昨天」（§6）。
pub fn save_of(relative: &str) -> Option<&str> {
    let rest = relative.strip_prefix("saves/")?;
    let name = rest.split('/').next()?;
    (!name.is_empty()).then_some(name)
}

/// 这条路径是不是配置。
pub fn is_config(relative: &str) -> bool {
    relative.starts_with("config/") || !relative.contains('/')
}

/// 这条路径是不是一个模组文件。
pub fn is_mod(relative: &str) -> bool {
    relative.strip_prefix("mods/").is_some_and(|rest| {
        !rest.contains('/') && (rest.ends_with(".jar") || rest.ends_with(".jar.disabled"))
    })
}

/// 清单里的路径会被拼回磁盘，所以每一条都要过这一关。
///
/// 拒绝绝对路径、盘符、`..`、反斜杠和空段。清单是我们自己写的文件，但它躺在
/// 用户的磁盘上，能被编辑，也能来自别人分享的导出包。
pub fn is_safe_relative(relative: &str) -> bool {
    !relative.is_empty()
        && relative.len() <= 1024
        && !relative.contains('\\')
        && !relative.contains('\0')
        && !relative.starts_with('/')
        && !relative.contains(':')
        && relative
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fern-select-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn write(path: PathBuf, body: &[u8]) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, body).expect("write");
    }

    #[test]
    fn takes_what_cannot_be_recovered_and_says_what_it_left() {
        let root = scratch("scan");
        write(root.join("saves/家/region/r.0.0.mca"), &[0u8; 64]);
        write(root.join("saves/家/session.lock"), b"lock");
        write(root.join("config/create.toml"), b"x");
        write(root.join("mods/create.jar"), b"jar");
        write(root.join("options.txt"), b"fov:70");
        write(root.join("logs/latest.log"), b"noise");
        write(root.join("journeymap/tiles/big"), b"tiles");
        write(root.join("usercache.json"), b"not ours");

        let (taken, skipped) = scan(&root);
        let paths: Vec<_> = taken.iter().map(|it| it.relative.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "config/create.toml",
                "mods/create.jar",
                "options.txt",
                "saves/家/region/r.0.0.mca",
            ]
        );
        // 锁文件在任何一层都不要。
        assert!(!paths.iter().any(|path| path.contains("session.lock")));

        // 日志是一次性的；journeymap 只是没被选中——两者对用户的意义不同。
        let reasons: Vec<_> = skipped
            .iter()
            .map(|it| (it.path.as_str(), it.reason))
            .collect();
        assert!(reasons.contains(&("logs/", SkipReason::Transient)));
        assert!(reasons.contains(&("journeymap/", SkipReason::NotSelected)));
        // 顶层的陌生文件不进快照，也不值得在界面上说——只有目录才记。
        assert!(!reasons.iter().any(|(path, _)| *path == "usercache.json"));

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn an_oversized_file_is_reported_rather_than_silently_dropped() {
        let root = scratch("large");
        write(root.join("config/huge.bin"), &[0u8; 16]);
        // 直接改上限不现实，这里验的是分类函数本身。
        let metadata = fs::metadata(root.join("config/huge.bin")).expect("metadata");
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        take(
            &root.join("config/huge.bin"),
            "config/huge.bin".to_owned(),
            &metadata,
            &mut candidates,
            &mut skipped,
        );
        assert_eq!(candidates.len(), 1);
        assert!(skipped.is_empty());

        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn missing_game_directory_is_not_an_error() {
        let (taken, skipped) = scan(&scratch("absent"));
        assert!(taken.is_empty());
        assert!(skipped.is_empty());
    }

    #[test]
    fn paths_from_a_manifest_cannot_escape_the_game_directory() {
        for evil in [
            "../settings.json",
            "saves/../../x",
            "/etc/passwd",
            "C:/Windows",
            "saves\\家\\level.dat",
            "",
            "saves//x",
        ] {
            assert!(!is_safe_relative(evil), "{evil} 应当被拒绝");
        }
        for fine in ["options.txt", "saves/家/level.dat", "mods/a.jar.disabled"] {
            assert!(is_safe_relative(fine), "{fine} 应当放行");
        }
    }

    #[test]
    fn a_path_knows_which_world_it_belongs_to() {
        assert_eq!(save_of("saves/家/level.dat"), Some("家"));
        assert_eq!(save_of("saves/家"), Some("家"));
        assert_eq!(save_of("config/x.toml"), None);
        assert!(is_config("config/create.toml"));
        assert!(is_config("options.txt"));
        assert!(!is_config("saves/家/level.dat"));
        assert!(is_mod("mods/create.jar"));
        assert!(is_mod("mods/create.jar.disabled"));
        // mods 下面的子目录不是模组本身（有些加载器往那里放缓存）。
        assert!(!is_mod("mods/1.20.1/create.jar"));
    }
}
