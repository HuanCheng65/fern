//! 运行历史（设计文档 §6.3）。
//!
//! 键是 `(instance_id, modlist_hash)`。加上 mod 列表的指纹不是为了严谨——是
//! 因为玩家往 `mods/` 里塞四十个新 Mod 之后，上个月那些统计已经在描述另一个
//! 东西了。指纹一变，历史降级为参考，回到静态估算重新学。
//!
//! 存的是**观察**，不是配置：它不该进 `settings.json`（用户会打开、会备份、
//! 会贴给别人的那份文件里不该有一堆机器数），也不该进 `cache/`（那里的东西
//! 随时可以整个删掉，而这些删了就得重学）。所以它有自己的一个文件。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::gclog::SessionMetrics;
use crate::DataPaths;

/// 滚动窗口的长度。
///
/// 八次：短到还能跟上玩法的变化（换了个整合包玩法，两三次之内就该反映出来），
/// 长到单次异常（开了一次创造模式满世界飞）不至于带偏结论。
pub const WINDOW: usize = 8;

/// 太短的会话不算数。启动即退出、点错了马上关掉，那种数据只有噪声。
pub const MINIMUM_MINUTES: f64 = 5.0;

/// 至少要这么多次有效会话，结论才敢用。
pub const MINIMUM_SESSIONS: usize = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Unix 秒。
    pub at: u64,
    pub minutes: f64,
    /// 那一次实际给了多少堆。调整规则算的是「相对上次给的量」，所以必须存。
    pub xmx_mb: u32,
    #[serde(default)]
    pub metrics: SessionMetrics,
    /// 那一次是不是撞了 OutOfMemoryError。
    #[serde(default)]
    pub oom: bool,
    /// 那一次走的是 ZGC 路径。换了 Java 导致路径切换时，系数要跟着换。
    #[serde(default)]
    pub zgc: bool,
}

impl Session {
    pub fn is_valid(&self) -> bool {
        self.minutes >= MINIMUM_MINUTES && !self.metrics.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub modlist_hash: String,
    pub sessions: Vec<Session>,
}

impl Window {
    pub fn valid(&self) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|session| session.is_valid())
            .collect()
    }

    pub fn usable(&self) -> bool {
        self.valid().len() >= MINIMUM_SESSIONS
    }

    fn push(&mut self, session: Session) {
        self.sessions.push(session);
        let overflow = self.sessions.len().saturating_sub(WINDOW);
        self.sessions.drain(..overflow);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    instances: BTreeMap<String, Window>,
}

fn path(paths: &DataPaths) -> PathBuf {
    paths.root.join("history").join("memory.json")
}

fn read_store(paths: &DataPaths) -> Store {
    // 读不出来就当没有：历史是加分项，它坏掉不该让游戏启动不了。
    std::fs::read(path(paths))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// 这个实例当前 mod 列表下的历史。指纹对不上就当没有历史。
pub fn read(paths: &DataPaths, instance_id: &str, modlist_hash: &str) -> Option<Window> {
    read_store(paths)
        .instances
        .remove(instance_id)
        .filter(|window| window.modlist_hash == modlist_hash)
        .filter(|window| window.usable())
}

/// 记一次会话。mod 列表变了就把窗口整个换掉，不做迁移——旧数据描述的是另一个
/// 实例内容，混进来只会让下一次分配算错。
pub fn record(
    paths: &DataPaths,
    instance_id: &str,
    modlist_hash: &str,
    session: Session,
) -> Result<()> {
    let mut store = read_store(paths);
    let window = store.instances.entry(instance_id.to_owned()).or_default();
    if window.modlist_hash != modlist_hash {
        *window = Window {
            modlist_hash: modlist_hash.to_owned(),
            sessions: Vec::new(),
        };
    }
    window.push(session);

    let file = path(paths);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).context("create history directory")?;
    }
    let bytes = serde_json::to_vec_pretty(&store).context("serialize memory history")?;
    std::fs::write(&file, bytes).with_context(|| format!("write {}", file.display()))
}

/// 忘掉一个实例的历史。删实例时调用——留着它，下一个拿到同一个 id 的实例会
/// 继承一份不属于它的统计。
pub fn forget(paths: &DataPaths, instance_id: &str) {
    let mut store = read_store(paths);
    if store.instances.remove(instance_id).is_none() {
        return;
    }
    let file = path(paths);
    if let Ok(bytes) = serde_json::to_vec_pretty(&store) {
        let _ = std::fs::write(file, bytes);
    }
}

/// mods 目录的指纹：排序后的文件名加大小。
///
/// 不读文件内容——一个 300 个 Mod 的整合包每次启动都做一遍完整哈希是几秒钟的
/// 磁盘时间，而文件名加大小已经足够回答「这堆 Mod 还是不是上次那堆」。
pub fn modlist_hash(game_directory: &Path) -> String {
    use sha2::{Digest, Sha256};

    let mut entries: Vec<(String, u64)> = std::fs::read_dir(game_directory.join("mods"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata.len(),
                )
            })
        })
        .collect();
    entries.sort();

    let mut hasher = Sha256::new();
    for (name, size) in entries {
        hasher.update(name.as_bytes());
        hasher.update(size.to_le_bytes());
    }
    // 前 16 位十六进制够了：它只用来判断「变没变」，不防伪造。
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> DataPaths {
        let root = std::env::temp_dir().join(format!(
            "fern-history-{name}-{}-{}",
            std::process::id(),
            now_seconds()
        ));
        DataPaths::new(root)
    }

    fn session(minutes: f64, xmx_mb: u32, peak_mb: u32) -> Session {
        Session {
            at: now_seconds(),
            minutes,
            xmx_mb,
            metrics: SessionMetrics {
                peak_mb,
                live_set_mb: peak_mb / 2,
                pause_p99_ms: 20.0,
                collections: 40,
                stalls: 0,
            },
            oom: false,
            zgc: false,
        }
    }

    #[test]
    fn a_changed_mod_list_throws_the_window_away() {
        let paths = temporary("modlist");
        record(&paths, "moss", "aaaa", session(30.0, 4096, 3000)).expect("record");
        record(&paths, "moss", "aaaa", session(30.0, 4096, 3100)).expect("record");
        assert!(read(&paths, "moss", "aaaa").is_some());
        // 指纹变了：上个月的统计在描述另一个实例内容。
        assert!(read(&paths, "moss", "bbbb").is_none());

        record(&paths, "moss", "bbbb", session(30.0, 4096, 5000)).expect("record");
        let window = read(&paths, "moss", "bbbb");
        assert!(window.is_none(), "一次会话还不够下结论");
        record(&paths, "moss", "bbbb", session(30.0, 4096, 5200)).expect("record");
        let window = read(&paths, "moss", "bbbb").expect("two sessions is enough");
        assert_eq!(window.sessions.len(), 2);
        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn short_sessions_never_count_towards_a_conclusion() {
        let paths = temporary("short");
        record(&paths, "moss", "aaaa", session(0.5, 4096, 200)).expect("record");
        record(&paths, "moss", "aaaa", session(1.0, 4096, 300)).expect("record");
        assert!(
            read(&paths, "moss", "aaaa").is_none(),
            "点错了马上关掉不该变成依据"
        );
        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn the_window_keeps_only_the_most_recent_runs() {
        let paths = temporary("window");
        for index in 0..12 {
            record(&paths, "moss", "aaaa", session(30.0, 4096, 1000 + index)).expect("record");
        }
        let window = read(&paths, "moss", "aaaa").expect("history");
        assert_eq!(window.sessions.len(), WINDOW);
        assert_eq!(window.sessions.last().expect("last").metrics.peak_mb, 1011);
        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn forgetting_an_instance_leaves_the_others_alone() {
        let paths = temporary("forget");
        record(&paths, "moss", "aaaa", session(30.0, 4096, 1000)).expect("record");
        record(&paths, "cinder", "bbbb", session(30.0, 4096, 1000)).expect("record");
        forget(&paths, "moss");
        let store = read_store(&paths);
        assert!(!store.instances.contains_key("moss"));
        assert!(store.instances.contains_key("cinder"));
        std::fs::remove_dir_all(&paths.root).ok();
    }
}
