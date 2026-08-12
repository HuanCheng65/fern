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
    let mut out = HashMap::new();
    for file in identify(paths, game_directory).await? {
        let entry = Installed {
            project_id: file.version.project_id,
            version_id: file.version.version_id,
            version_number: file.version.version_number,
            file_name: file.file_name,
            enabled: file.enabled,
            game_versions: file.version.game_versions,
            loaders: file.version.loaders,
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
    Ok(out)
}

/// 一个认得出身份的模组文件。
pub(crate) struct Recognized {
    /// 文件内容的 sha1。它就是这份文件在 Modrinth 上的主键。
    pub(crate) hash: String,
    pub(crate) file_name: String,
    pub(crate) enabled: bool,
    pub(crate) version: KnownVersion,
}

/// `mods/` 里每个认得出身份的文件是哪个版本。
///
/// 「已经装了什么」和「有没有新版」问的是同一件事的两面，中间这一步——哈希、
/// 批量反查、把答案缓存下来——两边完全一样，各写一遍就会各缓存一份，同一批
/// 两三个 G 的 jar 被读两遍。
async fn identify(paths: &DataPaths, game_directory: &Path) -> Result<Vec<Recognized>> {
    let files = mod_files(game_directory);
    if files.is_empty() {
        return Ok(Vec::new());
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

    let recognized = hashes
        .into_iter()
        .filter_map(|(hash, enabled, file_name)| {
            let version = cache.versions.get(&hash)?.clone()?;
            Some(Recognized {
                hash,
                file_name,
                enabled,
                version,
            })
        })
        .collect();
    cache.write(paths);
    Ok(recognized)
}

/// 这个实例里有新版的那些模组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    /// 磁盘上现在那一份。装上新版之后要把它删掉。
    pub file_name: String,
    /// 项目名。文件名认不出是什么的时候，只有它能读。
    pub title: String,
    pub current: String,
    pub latest: String,
    /// 新版本的 id，装它用的。
    pub version_id: String,
    /// 这份文件现在是不是启用的。
    pub enabled: bool,
}

/// 查一遍哪些模组有新版。
///
/// **只在用户按下那一刻查。** 每次打开列表都联网，等于替所有人决定这件事值得
/// 一次等待和一次请求；而它的答案几天才变一次。
///
/// 只看这个实例跑得起来的版本：一个只发了 1.21 版的更新，对 1.20.1 的实例来说
/// 不是「有新版」，是「不适用」——把它列出来，按下去就是一个起不来的游戏。
pub async fn updates(paths: &DataPaths, instance_id: &str) -> Result<Vec<ModUpdate>> {
    let profile = crate::read_instance(paths, instance_id)?;
    let game_directory = crate::instance::paths_for(paths, &profile).game_directory(instance_id);
    let files = identify(paths, &game_directory).await?;
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let hashes: Vec<String> = files.iter().map(|file| file.hash.clone()).collect();
    let latest = super::latest_by_hash(&hashes, &profile.game_version, profile.loader).await?;

    let mut updates = Vec::new();
    let mut projects = Vec::new();
    for file in &files {
        let Some(newer) = latest.get(&file.hash) else {
            continue;
        };
        // 上游给的可能就是手上这一份——那不是「有新版」。
        if newer.version_id == file.version.version_id {
            continue;
        }
        projects.push(file.version.project_id.clone());
        updates.push(ModUpdate {
            file_name: file.file_name.clone(),
            // 名字随后换，换不到就退回文件名——列表里每一行都得有个能读的东西。
            title: file.file_name.clone(),
            current: file.version.version_number.clone(),
            latest: newer.version_number.clone(),
            version_id: newer.version_id.clone(),
            enabled: file.enabled,
        });
    }
    if updates.is_empty() {
        return Ok(Vec::new());
    }

    let names = super::project_names(&projects).await.unwrap_or_default();
    for (update, project) in updates.iter_mut().zip(projects) {
        if let Some(named) = names.get(&project) {
            update.title = named.title.clone();
        }
    }
    updates.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(updates)
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
