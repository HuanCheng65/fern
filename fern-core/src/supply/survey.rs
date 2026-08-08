//! 这个实例里已经装了哪些补给站上的东西。
//!
//! 「装了什么」这个问题在文件名上是答不出来的：同一个模组从 Modrinth 装、从
//! CurseForge 装、作者自己发的构建，文件名可以完全不同；而 `sodium-0.6.jar` 和
//! `sodium-fabric-0.6.13.jar` 是同一个东西。唯一可靠的身份是**文件内容的
//! sha1**——Modrinth 的每个文件都以它为主键，一个 hash 永远对应同一个版本。
//!
//! 所以这一层做两件事：把 `mods/` 里的每个文件哈希一遍，再向 Modrinth 批量
//! 换回它们各自是哪个项目的哪个版本。没有缓存的话，一个三百个 Mod 的整合包每
//! 次打开补给站都要重读两个 G。
//!
//! 哈希那一半交给 `instance::hashes`——对账要的是同一批文件的 sha1，两边各存
//! 一份缓存就意味着各读一遍。换回来的结果留在这里，按 hash 缓存：hash → 版本
//! 是不可变映射，缓存它永远不会过期。
//!
//! 缓存放在 `cache/` 下：删掉它只是下次慢一点。

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::DataPaths;

/// 实例里已经有的一个 Modrinth 版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub file_name: String,
    /// 带 `.disabled` 后缀的那些：文件在，但加载器不会读它。
    pub enabled: bool,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
}

impl Installed {
    /// 这份文件在这个实例上跑得起来吗。
    ///
    /// 装是装了，但装的是 1.20 的版本而实例是 1.21——那不叫「前置已满足」，
    /// 那叫「有一个装错了的前置」，而这两句话对用户的意义完全不同。
    pub fn fits(&self, game_version: &str, loader: crate::LoaderKind) -> bool {
        let version_ok =
            self.game_versions.is_empty() || self.game_versions.iter().any(|it| it == game_version);
        let tags = super::loader_tags(loader);
        let loader_ok = self.loaders.is_empty()
            || tags.is_empty()
            || self
                .loaders
                .iter()
                .any(|it| tags.iter().any(|tag| it.eq_ignore_ascii_case(tag)));
        version_ok && loader_ok
    }
}

/// 扫一遍 mods 目录，按项目 id 归档。
///
/// 认不出来的文件（本地构建、别处下的、根本不在 Modrinth 上）直接跳过——它们
/// 确实存在，但我们说不出它是什么，而**猜一个身份比不认识更危险**：把一个同名
/// 的别的模组当成前置已满足，用户会得到一个起不来的游戏和一句不对的解释。
pub async fn installed(
    paths: &DataPaths,
    game_directory: &Path,
) -> Result<HashMap<String, Installed>> {
    let files = mod_files(game_directory);
    if files.is_empty() {
        return Ok(HashMap::new());
    }

    let mut cache = Cache::read(paths);
    // 哈希走共用的那份缓存：对账也要同一批文件的 sha1，各读一遍两三个 G
    // 是没有道理的（见 `instance::hashes`）。
    let mut digests = crate::instance::hashes::Hashes::open(paths);
    let mut hashes = Vec::new();
    for file in &files {
        let key = format!("mods/{}", file.name);
        let Some(hash) = digests.of(&key, &file.path) else {
            continue;
        };
        hashes.push((hash, file.enabled, file.name.clone()));
    }
    digests.save(paths);

    // 缓存里没见过的才去问。问一次是一个批量请求，不是一个 Mod 一次。
    let unknown: Vec<String> = hashes
        .iter()
        .map(|(hash, _, _)| hash.clone())
        .filter(|hash| !cache.versions.contains_key(hash))
        .collect();
    if !unknown.is_empty() {
        let found = super::versions_by_hash(&unknown).await?;
        for hash in &unknown {
            // 查不到的也记下来（记成 None）：本地构建的模组每次都去问一遍，
            // 是在为一个永远不会变的答案反复付网络往返。
            cache
                .versions
                .insert(hash.clone(), found.get(hash).cloned());
        }
        cache.dirty = true;
    }

    let mut out = HashMap::new();
    for (hash, enabled, file_name) in hashes {
        let Some(Some(version)) = cache.versions.get(&hash) else {
            continue;
        };
        let entry = Installed {
            project_id: version.project_id.clone(),
            version_id: version.version_id.clone(),
            version_number: version.version_number.clone(),
            file_name,
            enabled,
            game_versions: version.game_versions.clone(),
            loaders: version.loaders.clone(),
        };
        // 同一个项目装了两份（一份禁用的旧版）时，启用的那份说了算。
        match out.entry(entry.project_id.clone()) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if entry.enabled && !slot.get().enabled {
                    slot.insert(entry);
                }
            }
        }
    }
    cache.write(paths);
    Ok(out)
}

struct ModFile {
    path: std::path::PathBuf,
    name: String,
    enabled: bool,
}

fn mod_files(game_directory: &Path) -> Vec<ModFile> {
    let Ok(entries) = std::fs::read_dir(game_directory.join("mods")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let enabled = name.ends_with(".jar");
            if !enabled && !name.ends_with(".jar.disabled") {
                return None;
            }
            Some(ModFile {
                path: entry.path(),
                name,
                enabled,
            })
        })
        .collect()
}

/// 缓存里存的那份版本信息。只留判断得上「是不是它、合不合适」需要的字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownVersion {
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cache {
    /// sha1 → 它是哪个版本。`None` 表示问过了，Modrinth 上没有。
    #[serde(default)]
    versions: BTreeMap<String, Option<KnownVersion>>,
    #[serde(skip)]
    dirty: bool,
}

impl Cache {
    fn path(paths: &DataPaths) -> std::path::PathBuf {
        paths.cache.join("modrinth-files.json")
    }

    fn read(paths: &DataPaths) -> Self {
        std::fs::read(Self::path(paths))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, paths: &DataPaths) {
        if !self.dirty {
            return;
        }
        let _ = std::fs::create_dir_all(&paths.cache);
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = std::fs::write(Self::path(paths), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoaderKind;

    fn installed(game_versions: &[&str], loaders: &[&str]) -> Installed {
        Installed {
            project_id: "AANobbMI".to_owned(),
            version_id: "abcd".to_owned(),
            version_number: "0.6.13".to_owned(),
            file_name: "sodium.jar".to_owned(),
            enabled: true,
            game_versions: game_versions.iter().map(|it| (*it).to_owned()).collect(),
            loaders: loaders.iter().map(|it| (*it).to_owned()).collect(),
        }
    }

    #[test]
    fn a_mod_built_for_another_version_is_not_a_satisfied_dependency() {
        let old = installed(&["1.20.1"], &["fabric"]);
        assert!(!old.fits("1.21.1", LoaderKind::Fabric));
        assert!(installed(&["1.21.1"], &["fabric"]).fits("1.21.1", LoaderKind::Fabric));
    }

    #[test]
    fn a_mod_built_for_another_loader_is_not_a_satisfied_dependency() {
        let fabric = installed(&["1.21.1"], &["fabric"]);
        assert!(!fabric.fits("1.21.1", LoaderKind::NeoForge));
        // Quilt 认 fabric 的模组，这一条跟搜索那边用的是同一张表。
        assert!(fabric.fits("1.21.1", LoaderKind::Quilt));
    }

    #[test]
    fn missing_metadata_is_not_treated_as_a_mismatch() {
        // 上游偶尔不标 loaders。没写不等于「不兼容」——它已经装在这个实例里
        // 了，说它不适用需要证据。
        assert!(installed(&["1.21.1"], &[]).fits("1.21.1", LoaderKind::NeoForge));
        assert!(installed(&[], &["fabric"]).fits("1.21.1", LoaderKind::Fabric));
    }
}
