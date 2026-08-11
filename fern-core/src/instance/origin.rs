//! 每个文件是**怎么进来的**。
//!
//! 这一层只记录，不判断。「没有记录」不是任何一种信号——用户直接把 jar 拖进
//! `mods/` 是完全正常的用法，我们看不见，也不该因此把那个文件标出来。而且从
//! 数据上看，「用户用文件管理器放进来的」和「别的东西放进来的」完全一样，
//! 区分不了的事就不要暗示。
//!
//! 记录的用途只有一个：等到某个文件的内容发生变化时，说清楚**那次变化是不是
//! 我们干的**。这才是有区分度的问题——已发布的模组 jar 是不可变的，它从不
//! 自己改自己，而任何自我复制的东西都必须写盘。
//!
//! ```text
//! <数据目录>/security/<实例 id>.jsonl
//! ```
//!
//! **不放在实例目录里**，三个理由，第三个才是关键的：外部实例的那个目录不归
//! 我们写（见 `external.rs` 的底线）；跟着实例走会被整合包分发带上，那本身
//! 就是一个攻击面；而 `.minecraft` 底下是模组有合法读写权限的地方，把参照物
//! 放在那里等于把答案写在考卷背面。
//!
//! 每行追加一条，行内带一个滚动哈希：`chain(n) = sha256(chain(n-1) + payload)`。
//! 这挡不住有决心的对手——恶意代码和游戏同权限运行，能读能写的东西我们也能，
//! 反过来也一样，纯本地的防篡改在原理上就赢不了。它做的是把「改一个 json
//! 字段」变成「读懂并重建一条链」，成本差两个数量级。真正改不了的参照物在
//! 别处：一个 sha1 能不能在 Modrinth 上查到，这个事实不存储在本机。

use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DataPaths, supply::Source};

/// 一个文件是怎么进到实例里的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// 补给站装的。
    #[serde(rename_all = "camelCase")]
    Supply {
        source: Source,
        project_id: String,
        version_id: String,
    },
    /// 整合包铺进来的。
    #[serde(rename_all = "camelCase")]
    Modpack {
        source: Source,
        name: String,
        version: String,
    },
    /// 用户从界面上导入的本地文件。
    Import,
    /// 第一次看见它的时候它就在那儿了。
    ///
    /// 接手一个已有的 `.minecraft` 是一种，用户绕开界面直接把 jar 拖进
    /// `mods/` 也是一种。**这两件事在数据上完全一样，我们区分不了**，所以不
    /// 假装区分——记的是「第一次见到它是什么时候、长什么样」，不是「它从哪来」。
    Adopted,
}

/// 要记一笔的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 相对游戏目录的路径，例如 `mods/sodium.jar`。用 `/` 分隔，Windows 上也是
    /// ——这份日志要能跟着实例在两个系统之间搬。
    pub file: String,
    pub sha1: String,
    /// 模组自己在 jar 里声明的版本号。读不到就是 `None`。
    ///
    /// 存它是为了回答一个很窄但很有用的问题：**内容变了，版本号变没变？**
    /// 用户换一个版本会带来版本号变化；往现有 jar 里追加东西不会。而这件事
    /// 只能在当时记下来——旧文件已经被覆盖了，事后谁也读不回来。
    pub version: Option<String>,
    pub origin: Origin,
}

/// 日志里的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// Unix 秒。
    pub at: u64,
    pub file: String,
    pub sha1: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub origin: Origin,
    /// 到这一行为止的滚动哈希。
    pub chain: String,
}

/// 记一批文件。
///
/// **失败什么都不做。** 这是一条旁路：装模组失败要告诉用户，记不上账不要——
/// 让一个用来观察的东西反过来阻断被观察的操作，是这类功能最糟的失效方式。
pub fn record(paths: &DataPaths, instance_id: &str, entries: Vec<Entry>) {
    let _ = append(paths, instance_id, entries);
}

/// 复制实例时，让副本继承同一份账。
///
/// 副本的 `mods/` 是原样拷过去的字节，所以那些记录对它一样成立。不继承的话
/// 副本会从零开始——虽然「没有记录」本身不是信号，但我们手上明明有这段历史，
/// 丢掉它只是让阶段一少一个参照物。
pub fn inherit(paths: &DataPaths, from: &str, to: &str) {
    let (Ok(from), Ok(to)) = (path(paths, from), path(paths, to)) else {
        return;
    };
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(from, to);
}

/// 删实例时把它的账也清掉。
pub fn forget(paths: &DataPaths, instance_id: &str) {
    if let Ok(path) = path(paths, instance_id) {
        let _ = std::fs::remove_file(path);
    }
}

/// 这个实例的全部记录，按写入顺序。
///
/// 读不出来就是没有记录。读不懂的行直接跳过——它会在链上表现为一处断裂，
/// 而那正是 [`broken_at`] 该说的话，不是这里。
pub fn records(paths: &DataPaths, instance_id: &str) -> Vec<Record> {
    let Ok(path) = path(paths, instance_id) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// 每个文件最后一次被记下来时是什么样。
pub fn latest(paths: &DataPaths, instance_id: &str) -> BTreeMap<String, Record> {
    let mut out = BTreeMap::new();
    for entry in records(paths, instance_id) {
        out.insert(entry.file.clone(), entry);
    }
    out
}

/// 链在第几行断的。
///
/// `None` 表示从头到尾接得上。注意接得上**不等于**没被改过：能重算整条链的
/// 对手照样能伪造一份自洽的日志。它能说的只是「这不是随手改的」。
pub fn broken_at(entries: &[Record]) -> Option<usize> {
    let mut previous = String::new();
    for (index, entry) in entries.iter().enumerate() {
        let expected = link(&previous, &payload(entry));
        if expected != entry.chain {
            return Some(index);
        }
        previous = expected;
    }
    None
}

fn append(paths: &DataPaths, instance_id: &str, entries: Vec<Entry>) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let path = path(paths, instance_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 最后一行的 chain 就是下一行的起点。为了它读一遍整个文件：这个文件按
    // 「装过多少东西」增长，几百行封顶，倒着找行的复杂度不值得。
    let mut previous = records(paths, instance_id)
        .last()
        .map(|entry| entry.chain.clone())
        .unwrap_or_default();

    let at = now();
    let mut buffer = String::new();
    for entry in entries {
        let mut line = Record {
            at,
            file: entry.file,
            sha1: entry.sha1,
            version: entry.version,
            origin: entry.origin,
            chain: String::new(),
        };
        line.chain = link(&previous, &payload(&line));
        previous.clone_from(&line.chain);
        buffer.push_str(&serde_json::to_string(&line)?);
        buffer.push('\n');
    }

    // 一批一次写，不是一个文件一次：整合包一装就是三百行。
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(buffer.as_bytes())?;
    Ok(())
}

fn path(paths: &DataPaths, instance_id: &str) -> Result<PathBuf> {
    let id = crate::InstanceId::parse(instance_id).map_err(|error| anyhow!("{error}"))?;
    Ok(paths
        .root
        .join("security")
        .join(format!("{}.jsonl", id.as_str())))
}

/// 参与链式哈希的那一段。
///
/// 手写而不是把结构体序列化一遍：链一旦对不上就再也接不回去，而 serde 的
/// 字段顺序、`skip_serializing_if`、新增字段的默认值，每一样都可能在一次
/// 无关的重构里悄悄改变编码结果，把所有老日志判成断裂。
fn payload(entry: &Record) -> String {
    let origin = match &entry.origin {
        Origin::Supply {
            source,
            project_id,
            version_id,
        } => format!("supply:{}:{project_id}:{version_id}", source.tag()),
        Origin::Modpack {
            source,
            name,
            version,
        } => format!("modpack:{}:{name}:{version}", source.tag()),
        Origin::Import => "import".to_owned(),
        Origin::Adopted => "adopted".to_owned(),
    };
    format!(
        "{}\t{}\t{}\t{}\t{origin}",
        entry.at,
        entry.file,
        entry.sha1,
        entry.version.as_deref().unwrap_or_default()
    )
}

fn link(previous: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(previous.as_bytes());
    hasher.update(b"\n");
    hasher.update(payload.as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tag: &str) -> DataPaths {
        let root = std::env::temp_dir().join(format!("fern-origin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        DataPaths::new(root)
    }

    fn supply(file: &str, sha1: &str) -> Entry {
        Entry {
            file: file.to_owned(),
            sha1: sha1.to_owned(),
            version: Some("1.0".to_owned()),
            origin: Origin::Supply {
                source: Source::Modrinth,
                project_id: "AANobbMI".to_owned(),
                version_id: "abcd".to_owned(),
            },
        }
    }

    #[test]
    fn records_survive_a_round_trip() {
        let paths = paths("round-trip");
        record(&paths, "1", vec![supply("mods/sodium.jar", "aa")]);
        record(&paths, "1", vec![supply("mods/jei.jar", "bb")]);

        let entries = records(&paths, "1");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file, "mods/sodium.jar");
        assert_eq!(entries[1].sha1, "bb");
        assert_eq!(broken_at(&entries), None);
    }

    #[test]
    fn the_chain_survives_appending_in_separate_batches() {
        let paths = paths("batches");
        record(
            &paths,
            "1",
            vec![supply("mods/a.jar", "aa"), supply("mods/b.jar", "bb")],
        );
        record(&paths, "1", vec![supply("mods/c.jar", "cc")]);
        assert_eq!(broken_at(&records(&paths, "1")), None);
    }

    #[test]
    fn editing_a_line_breaks_the_chain_from_there_on() {
        let paths = paths("tamper");
        record(&paths, "1", vec![supply("mods/a.jar", "aa")]);
        record(&paths, "1", vec![supply("mods/b.jar", "bb")]);
        record(&paths, "1", vec![supply("mods/c.jar", "cc")]);

        // 把中间那一行的 sha1 改掉——正是「换了个 jar 再把账抹平」的样子。
        let path = path(&paths, "1").expect("path");
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::write(&path, text.replace("\"sha1\":\"bb\"", "\"sha1\":\"zz\"")).expect("write");

        assert_eq!(broken_at(&records(&paths, "1")), Some(1));
    }

    #[test]
    fn removing_a_line_breaks_the_chain() {
        let paths = paths("truncate");
        record(&paths, "1", vec![supply("mods/a.jar", "aa")]);
        record(&paths, "1", vec![supply("mods/b.jar", "bb")]);

        let path = path(&paths, "1").expect("path");
        let text = std::fs::read_to_string(&path).expect("read");
        let kept: Vec<&str> = text.lines().skip(1).collect();
        std::fs::write(&path, kept.join("\n")).expect("write");

        assert_eq!(broken_at(&records(&paths, "1")), Some(0));
    }

    #[test]
    fn latest_keeps_the_last_word_on_each_file() {
        let paths = paths("latest");
        record(&paths, "1", vec![supply("mods/a.jar", "old")]);
        record(&paths, "1", vec![supply("mods/a.jar", "new")]);

        let latest = latest(&paths, "1");
        assert_eq!(latest.len(), 1);
        assert_eq!(latest["mods/a.jar"].sha1, "new");
    }

    #[test]
    fn an_instance_with_no_log_reads_as_empty() {
        let paths = paths("missing");
        assert!(records(&paths, "1").is_empty());
        assert!(latest(&paths, "1").is_empty());
        assert_eq!(broken_at(&[]), None);
    }

    /// 一个能被 `describe` 读懂的最小 jar。
    fn jar(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).expect("create"));
        writer
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .expect("start");
        std::io::Write::write_all(&mut writer, br#"{"id":"x","version":"1"}"#).expect("write");
        writer.finish().expect("finish");
    }

    #[test]
    fn importing_a_local_jar_leaves_a_record() {
        let paths = paths("import");
        let profile = crate::create_instance(&paths, "导入", "1.21.1").expect("create");
        let id = profile.id.as_str();

        let source = paths.root.join("外面/example-1.0.jar");
        jar(&source);
        crate::instance::mods::install(&paths, id, &source).expect("install");

        let entries = records(&paths, id);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file, "mods/example-1.0.jar");
        assert_eq!(entries[0].origin, Origin::Import);
        // 版本号要在**当时**记下来：文件被覆盖之后谁也读不回来。
        assert_eq!(entries[0].version.as_deref(), Some("1"));
    }

    #[test]
    fn a_copy_inherits_the_ledger_and_a_deleted_instance_leaves_none_behind() {
        let paths = paths("copy-and-delete");
        let profile = crate::create_instance(&paths, "原件", "1.21.1").expect("create");
        let id = profile.id.as_str().to_owned();
        let game = crate::instance::paths_for(&paths, &profile).game_directory(&id);
        jar(&game.join("mods/alpha.jar"));
        crate::instance::integrity::adopt(&paths, &id, None);

        let copy = crate::duplicate_instance(&paths, &id, "副本").expect("duplicate");
        assert_eq!(
            latest(&paths, copy.id.as_str()).len(),
            1,
            "副本的 mods 是原样拷过去的，账也该跟着"
        );
        assert_eq!(broken_at(&records(&paths, copy.id.as_str())), None);

        crate::delete_instance(&paths, &id).expect("delete");
        assert!(records(&paths, &id).is_empty());
        // 删一个不该动到另一个。
        assert_eq!(latest(&paths, copy.id.as_str()).len(), 1);
    }

    #[test]
    fn an_unusable_instance_id_never_becomes_a_path() {
        let paths = paths("traversal");
        assert!(path(&paths, "../../etc/passwd").is_err());
        // 记不上账不能变成一次 panic，也不能写到别处去。
        record(&paths, "../../etc", vec![supply("mods/a.jar", "aa")]);
        assert!(!paths.root.join("security").exists());
    }
}
