//! 验过的那一份，别每次启动都再验一遍。
//!
//! 补全是幂等的：`download_all` 拿到的永远是完整的任务表，每个任务先问一句
//! 「磁盘上这份算不算数」，算数就跳过。而对有 sha1 的任务，那一句话的实现是把
//! **整个文件读进内存重算一遍哈希**。一个 1.21 的实例是四千多个资源文件加几百兆
//! 的库和 client jar，于是每点一次启动，这几百兆都要完整过一遍磁盘。
//!
//! 在有 SHA 指令的 CPU 上这是零点几秒，看不出来。换成机械硬盘、或者 Windows 上
//! 每打开一个文件都要过一遍实时扫描，它就是用户说的「启动很慢」本身。
//!
//! 所以记一笔：`路径|大小|修改时间` → sha1。大小和修改时间都没变，就认上次算出来
//! 的那个值。**这不是把时间戳当成身份**，是用它决定该读哪些文件——伪造得了时间戳
//! 的改动，留给显式的那一遍去抓（见 [`crate::DownloadClient::rechecking`]，界面上
//! 是实例详情里的「校验」）。和 `fern-core` 里对账用的哈希缓存是同一套道理。
//!
//! 账本放在 `cache/` 下，删掉它只是下次慢一点。

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

/// 超过这个条数就只留这次进程碰过的。键会随文件变化自然失效，长不到这里来；
/// 这一条是防着「数据目录搬过好几次、老路径的键永远没人清」那种慢性堆积。
const MAX_ENTRIES: usize = 200_000;

/// 验过的文件。
///
/// 没有 [`Verified::at`] 给的落盘位置就是关着的：`recall` 永远说不知道，`remember`
/// 什么也不做。这样 [`crate::DownloadClient::new`] 的行为和加这层缓存之前一模一样，
/// 只有明确要了缓存的调用方才会拿到它。
#[derive(Default)]
pub struct Verified {
    file: Option<PathBuf>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    known: BTreeMap<String, String>,
    /// 这次进程碰过的键。只在超出上限、要裁的时候用得上。
    touched: HashSet<String>,
    dirty: bool,
}

impl Verified {
    /// 打开落在 `file` 的账本。读不出来就是一本空的——它是缓存，不是数据。
    pub fn at(file: impl Into<PathBuf>) -> Arc<Self> {
        let file = file.into();
        let known = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, String>>(&bytes).ok())
            .unwrap_or_default();
        Arc::new(Self {
            file: Some(file),
            state: Mutex::new(State {
                known,
                ..State::default()
            }),
        })
    }

    /// 这个文件上次算出来的 sha1，前提是它的大小和修改时间都还是那时候的样子。
    pub(crate) fn recall(&self, path: &Path, metadata: &std::fs::Metadata) -> Option<String> {
        if self.file.is_none() {
            return None;
        }
        let stamp = stamp(path, metadata)?;
        let mut state = self.state.lock().ok()?;
        let known = state.known.get(&stamp).cloned()?;
        state.touched.insert(stamp);
        Some(known)
    }

    pub(crate) fn remember(&self, path: &Path, metadata: &std::fs::Metadata, sha1: &str) {
        if self.file.is_none() {
            return;
        }
        let Some(stamp) = stamp(path, metadata) else {
            return;
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let sha1 = sha1.to_ascii_lowercase();
        state.touched.insert(stamp.clone());
        if state.known.insert(stamp, sha1.clone()) != Some(sha1) {
            state.dirty = true;
        }
    }

    /// 现取一次元数据再记。刚下完、或者刚重验通过的时候用——那两处手上有哈希，
    /// 但还没有这个文件的 stat。
    pub(crate) async fn note(&self, path: &Path, sha1: &str) {
        if self.file.is_none() {
            return;
        }
        if let Ok(metadata) = tokio::fs::metadata(path).await {
            self.remember(path, &metadata, sha1);
        }
    }

    /// 落盘。一整批下完之后写一次，尽力而为——缓存目录写不进去不该让下载失败。
    ///
    /// 写之前先把盘上那份读回来垫底：同一台机器上开着两个 Fern 的时候，谁后写
    /// 谁不该把对方这一轮学到的东西抹掉。
    pub(crate) async fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let bytes = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if !state.dirty {
                return;
            }
            if state.known.len() > MAX_ENTRIES {
                let touched = std::mem::take(&mut state.touched);
                state.known.retain(|stamp, _| touched.contains(stamp));
                state.touched = touched;
            }
            let mut merged = std::fs::read(file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, String>>(&bytes).ok())
                .unwrap_or_default();
            merged.extend(state.known.iter().map(|(k, v)| (k.clone(), v.clone())));
            state.dirty = false;
            match serde_json::to_vec(&merged) {
                Ok(bytes) => bytes,
                Err(_) => return,
            }
        };
        if let Some(parent) = file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(file, bytes).await;
    }
}

fn stamp(path: &Path, metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}|{}|{modified}", path.display(), metadata.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fern-verified-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test root");
        root
    }

    #[tokio::test]
    async fn a_ledger_without_a_file_never_remembers_anything() {
        let root = root("off");
        let path = root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");
        let metadata = std::fs::metadata(&path).expect("stat");

        let ledger = Verified::default();
        ledger.remember(&path, &metadata, "abc");
        assert_eq!(ledger.recall(&path, &metadata), None);

        std::fs::remove_dir_all(root).expect("clean up");
    }

    #[tokio::test]
    async fn the_ledger_survives_a_round_trip_to_disk() {
        let root = root("round-trip");
        let path = root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");
        let metadata = std::fs::metadata(&path).expect("stat");

        let ledger = Verified::at(root.join("verified.json"));
        ledger.remember(&path, &metadata, "abc");
        ledger.save().await;

        let reopened = Verified::at(root.join("verified.json"));
        assert_eq!(
            reopened.recall(&path, &metadata).as_deref(),
            Some("abc"),
            "重开一次账本，上次记的还在"
        );

        std::fs::remove_dir_all(root).expect("clean up");
    }

    /// 改了大小或者改了修改时间，键就不是那个键了。
    #[tokio::test]
    async fn a_file_that_changed_is_no_longer_recognised() {
        let root = root("changed");
        let path = root.join("a.jar");
        std::fs::write(&path, b"one").expect("write");
        let before = std::fs::metadata(&path).expect("stat");

        let ledger = Verified::at(root.join("verified.json"));
        ledger.remember(&path, &before, "abc");

        std::fs::write(&path, b"different length").expect("write");
        let after = std::fs::metadata(&path).expect("stat");
        assert_eq!(ledger.recall(&path, &after), None);

        std::fs::remove_dir_all(root).expect("clean up");
    }

    /// 别人的旧账不会被这一次的保存抹掉。
    #[tokio::test]
    async fn saving_merges_with_what_is_already_on_disk() {
        let root = root("merge");
        let file = root.join("verified.json");
        let first = root.join("a.jar");
        let second = root.join("b.jar");
        std::fs::write(&first, b"one").expect("write");
        std::fs::write(&second, b"two").expect("write");
        let first_meta = std::fs::metadata(&first).expect("stat");
        let second_meta = std::fs::metadata(&second).expect("stat");

        let one = Verified::at(&file);
        one.remember(&first, &first_meta, "aaa");
        one.save().await;

        // 第二本账本开在第一本写完之后，只认识 b.jar。
        let two = Verified::at(&file);
        two.remember(&second, &second_meta, "bbb");
        two.save().await;

        let reopened = Verified::at(&file);
        assert_eq!(reopened.recall(&first, &first_meta).as_deref(), Some("aaa"));
        assert_eq!(
            reopened.recall(&second, &second_meta).as_deref(),
            Some("bbb")
        );

        std::fs::remove_dir_all(root).expect("clean up");
    }
}
