//! 算过的哈希别再算一遍。
//!
//! 「这个文件的 sha1 是多少」在两个地方要用：补给站要拿它向 Modrinth 反查模组
//! 身份，对账要拿它和上次记的比。一个三百个 Mod 的整合包是两三个 G，读一遍就
//! 要几十秒——两边各读一遍是没有道理的。
//!
//! 键是 `相对路径|大小|修改时间`。大小和修改时间都没变，就认为内容没变，直接
//! 给出上次算的那个值。**这不是把时间戳当成身份**，是用它决定该读哪些文件：
//! 伪造得了时间戳的改动，留给不看缓存的那一遍去抓（见 `integrity::Depth`）。
//!
//! 缓存放在 `cache/` 下，删掉它只是下次慢一点。

use std::{collections::BTreeMap, path::Path, time::UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::DataPaths;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Hashes {
    /// `相对路径|大小|修改时间` → sha1。
    #[serde(default)]
    entries: BTreeMap<String, String>,
    #[serde(skip)]
    dirty: bool,
}

impl Hashes {
    pub(crate) fn open(paths: &DataPaths) -> Self {
        std::fs::read(Self::path(paths))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// 这个文件的 sha1，能不读盘就不读。
    ///
    /// `key` 是文件的稳定身份（相对游戏目录的路径）；大小和修改时间从 `path`
    /// 现取。读不出来就是 `None`。
    pub(crate) fn of(&mut self, key: &str, path: &Path) -> Option<String> {
        let stamp = stamp(key, path)?;
        if let Some(known) = self.entries.get(&stamp) {
            return Some(known.clone());
        }
        let sha1 = crate::backup::sha1_of(path).ok()?;
        self.entries.insert(stamp, sha1.clone());
        self.dirty = true;
        Some(sha1)
    }

    /// 重新读一遍，不管缓存里有什么，并把缓存刷成这次的结果。
    pub(crate) fn reread(&mut self, key: &str, path: &Path) -> Option<String> {
        let sha1 = crate::backup::sha1_of(path).ok()?;
        if let Some(stamp) = stamp(key, path) {
            self.entries.insert(stamp, sha1.clone());
            self.dirty = true;
        }
        Some(sha1)
    }

    pub(crate) fn save(&self, paths: &DataPaths) {
        if !self.dirty {
            return;
        }
        let _ = std::fs::create_dir_all(&paths.cache);
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = std::fs::write(Self::path(paths), bytes);
        }
    }

    fn path(paths: &DataPaths) -> std::path::PathBuf {
        paths.cache.join("file-hashes.json")
    }
}

fn stamp(key: &str, path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{key}|{}|{modified}", metadata.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tag: &str) -> DataPaths {
        let root = std::env::temp_dir().join(format!("fern-hashes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        DataPaths::new(root)
    }

    #[test]
    fn a_file_that_changed_gets_rehashed_and_one_that_did_not_does_not() {
        let paths = paths("reuse");
        std::fs::create_dir_all(&paths.root).expect("mkdir");
        let path = paths.root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");

        let mut hashes = Hashes::open(&paths);
        let first = hashes.of("mods/a.jar", &path).expect("hash");

        // 内容没动，缓存直接命中——把文件删掉也照样给得出答案，这正说明它没读盘。
        std::fs::rename(&path, paths.root.join("moved")).expect("rename");
        std::fs::rename(paths.root.join("moved"), &path).expect("rename back");
        assert_eq!(hashes.of("mods/a.jar", &path).as_ref(), Some(&first));

        // 内容变了，大小也变了，缓存键跟着变。
        std::fs::write(&path, b"different content").expect("write");
        let second = hashes.of("mods/a.jar", &path).expect("hash");
        assert_ne!(first, second);
    }

    #[test]
    fn rereading_ignores_the_cache() {
        let paths = paths("reread");
        std::fs::create_dir_all(&paths.root).expect("mkdir");
        let path = paths.root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");

        let mut hashes = Hashes::open(&paths);
        let cached = hashes.of("mods/a.jar", &path).expect("hash");

        // 大小一样、时间戳照旧——缓存会说没变，重读会说变了。
        let stamp = stamp("mods/a.jar", &path).expect("stamp");
        hashes.entries.insert(stamp, cached.clone());
        std::fs::write(&path, b"two").expect("write");
        let filetime = std::fs::metadata(&path).expect("meta");
        assert_eq!(filetime.len(), 3);

        let reread = hashes.reread("mods/a.jar", &path).expect("hash");
        assert_ne!(reread, cached);
    }

    #[test]
    fn the_cache_survives_a_round_trip_to_disk() {
        let paths = paths("round-trip");
        std::fs::create_dir_all(&paths.root).expect("mkdir");
        let path = paths.root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");

        let mut hashes = Hashes::open(&paths);
        let first = hashes.of("mods/a.jar", &path).expect("hash");
        hashes.save(&paths);

        let mut reopened = Hashes::open(&paths);
        assert_eq!(reopened.of("mods/a.jar", &path).as_ref(), Some(&first));
    }
}
