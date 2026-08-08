//! 对账：磁盘上现在是什么样，和上次记下来的对不对得上。
//!
//! `origin.rs` 管**记**，这里管**对**。分开是因为它们的失效方式不一样——记
//! 不上账只是少一条历史，对错了账是直接冤枉用户。
//!
//! ## 什么不是信号
//!
//! **「这个文件没有记录」不是。** 用户绕开界面直接把 jar 拖进 `mods/` 是完全
//! 正常的用法，我们看不见；而「用户用文件管理器放进来的」和「别的东西放进来
//! 的」在数据上完全一样，区分不了的事就不要暗示。第一次见到一个文件，只做一
//! 件事：把它记成基线，好让**下一次**它变的时候有参照物。
//!
//! ## 什么是信号
//!
//! 只有一件事有区分度：**一个已经记过的文件，内容变了。** 已发布的模组 jar
//! 是不可变的，它从不自己改自己，而任何自我复制的东西都必须写盘。
//!
//! 单独一次变化说明不了什么——用户覆盖一个同名文件也长这样。有区分度的是
//! 变化的**形态**，[`Change`] 上那两个判断就是为它们准备的：
//!
//! - 几十个文件在同一个时间窗口里一起变（用户不会同时手动替换四十个文件）
//! - 内容变了而模组自己声明的版本号没变（换版本会带来版本号变化，往现有 jar
//!   里追加东西不会）
//!
//! 还有第三条，在这里做不了：变化前的 sha1 在 Modrinth 上查得到、变化后查不
//! 到。那要联网，归 `supply::survey`。但它是三条里最难伪造的一条——本机改不了
//! 上游的数据库。
//!
//! ## 为什么分成 compare 和 accept
//!
//! 对账不写盘。写基线是单独一步，由调用方在**确实把结果交给用户之后**才做。
//! 合成一个函数的话，一次后台对账就会把事件悄悄吃掉——用户永远看不到它。

use std::{collections::BTreeMap, path::Path, time::UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    DataPaths,
    instance::{
        hashes::Hashes,
        origin::{self, Entry, Origin, Record},
    },
    launch::crash::Action,
};

/// 会去清点的那几个目录。
///
/// 存档和配置不在其中：它们本来就天天变，盯着它们只会让真正的信号被淹掉。
const WATCHED: [&str; 3] = ["mods", "resourcepacks", "shaderpacks"];

/// 「同时」是多久。
///
/// 一次自我复制会在几秒内改完所有 jar。给到五分钟是为了兜住机械硬盘上几百个
/// 文件慢慢写完的情况——宁可把一次手动的批量替换也算进来，也不要漏掉真的。
const TOGETHER: u64 = 300;

/// 几个文件被静默改写才算「一批」。
///
/// 三个已经很不寻常了：用户换模组是一个一个换的，而一次自我复制是能改多少改
/// 多少。定得再低会把「换一个模组顺带换了它的前置」也算进来。
const AT_ONCE: usize = 3;

/// 要说给用户的一条。
///
/// 和预检查、崩溃分析一样，这里不产出句子：给出的是文案 id（`integrity.<kind>`）
/// 加一组参数，措辞与翻译都在 `fern-ui/src/lib/i18n/`。
///
/// **没有严重程度字段。** 预检查的 `Severity` 说的是「点下去会不会起不来」，
/// 而一批 jar 被改写完全不影响游戏能否启动——两件事不在一条轴上。该用什么分量
/// 呈现由是哪一条 [`kind`] 决定，程度由参数里的数字说话。
///
/// 动作共用崩溃分析那一套，界面上是同一颗按钮。目前四条都没有动作：真正该有
/// 的是「恢复到变化之前的那张快照」，而那要给 `Action` 添一个取值——那颗按钮
/// 对崩溃分析同样有用，该单独做，不该藏在这个功能里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// 这一条在这次对账里的唯一键，界面拿它做列表的 key。
    pub id: String,
    /// 文案 id：界面按 `integrity.<kind>` 查。取值见 [`kind`]。
    pub kind: String,
    /// 文案里的占位符。
    pub args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// 对账会说的话，全部在这里。
pub mod kind {
    /// 一批文件在同一个时间窗口里被改写，声明的版本号都没变。
    pub const REWRITTEN_TOGETHER: &str = "rewritten-together";
    /// 变更记录本身对不上。
    pub const LEDGER_BROKEN: &str = "ledger-broken";
    /// 从上游认得的构建，变成了上游不认识的文件。
    pub const LEFT_UPSTREAM: &str = "left-upstream";
    /// 内容变了，模组声明的版本号没变。
    pub const SILENT_REWRITE: &str = "silent-rewrite";

    /// 全部取值，**顺序就是呈现顺序**。界面那边的文案表按它对齐。
    ///
    /// `LEDGER_BROKEN` 排在前面是因为它说的不是「哪个文件变了」，而是「后面
    /// 这几句话有多可信」——那得先讲。
    pub const ALL: [&str; 4] = [
        REWRITTEN_TOGETHER,
        LEDGER_BROKEN,
        LEFT_UPSTREAM,
        SILENT_REWRITE,
    ];
}

/// 这一遍要读多少东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Depth {
    /// 只重算大小或修改时间变过的那些，其余的用缓存里的值。
    ///
    /// 一个四百个 Mod 的整合包在没有变化时就是四百次 `stat`，毫秒级。打开实例
    /// 和点启动之前用这一种——那两个时刻有人在等。
    #[default]
    Quick,
    /// 每个文件都重读一遍，不看缓存。
    ///
    /// 伪造过大小和修改时间的改动只有这一遍看得见。游戏退出后用这一种：风险
    /// 窗口刚关上，而且没有人在等。
    Full,
}

/// 一个已经记过的文件，内容和上次不一样了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub file: String,
    /// 上次记下来的 sha1。
    pub was: String,
    /// 现在的 sha1。
    pub now: String,
    /// 上次记账的时刻，Unix 秒。
    pub recorded_at: u64,
    /// 文件现在的修改时间，Unix 秒。读不到就是 0。
    pub modified_at: u64,
    /// 模组自己声明的版本号：上次记的，和现在的。
    pub version_was: Option<String>,
    pub version_now: Option<String>,
    /// 上次这个文件是怎么进来的。
    pub origin_was: Origin,
    /// 上游认不认得变化前后这两份内容。`None` 表示没问过——离线，或者调用方
    /// 没要。见 [`ask_upstream`]。
    pub was_published: Option<bool>,
    pub now_published: Option<bool>,
}

impl Change {
    /// 内容变了，而模组自己声明的版本号一个字没动。
    ///
    /// 用户换一个版本会带来版本号变化。往现有 jar 里追加 class 不会——感染要
    /// 的就是让它看起来还是原来那个模组。
    ///
    /// 两边都读不到版本号时是 `false`：那说明这压根不是个带元数据的模组（资源
    /// 包、光影），拿一个空值去和另一个空值比，比出来的不是事实。
    pub fn version_stayed_put(&self) -> bool {
        self.version_was.is_some() && self.version_was == self.version_now
    }

    /// 原来是一份上游认得的构建，现在成了上游不认识的文件。
    ///
    /// 三条信号里最难伪造的一条：本机改得了这台机器上的账，改不了 Modrinth 的
    /// 数据库。没问过上游时是 `false`——没问就是不知道，不是没有。
    pub fn left_the_upstream(&self) -> bool {
        self.was_published == Some(true) && self.now_published == Some(false)
    }
}

/// 一次对账的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compared {
    /// 记过的文件里，内容变了的那些。
    pub changes: Vec<Change>,
    /// 第一次见到的文件。**不是信号**，见模块开头。列在这里只是为了
    /// [`accept`] 能把它们记成基线。
    pub first_seen: Vec<Entry>,
}

impl Compared {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.first_seen.is_empty()
    }

    /// 内容变了、而模组声明的版本号一个字没动的那些。
    pub fn silently_rewritten(&self) -> Vec<&Change> {
        self.changes
            .iter()
            .filter(|change| change.version_stayed_put())
            .collect()
    }

    /// 被静默改写的文件里，落在同一个时间窗口里的最大一群有多少个。
    ///
    /// 只数静默改写的那些，是为了避开一个很常见的误报：用户在别的启动器里更新
    /// 了整合包，两百个文件同时变——但它们的版本号也都跟着变了，那是升级，不是
    /// 改写。
    pub fn largest_batch(&self) -> usize {
        let mut times: Vec<u64> = self
            .silently_rewritten()
            .iter()
            .map(|change| change.modified_at)
            .collect();
        times.sort_unstable();

        let mut best = 0;
        let mut start = 0;
        for end in 0..times.len() {
            while times[end].saturating_sub(times[start]) > TOGETHER {
                start += 1;
            }
            best = best.max(end - start + 1);
        }
        best
    }
}

/// 对一次账。不写任何基线。
pub fn compare(paths: &DataPaths, instance_id: &str, depth: Depth) -> Compared {
    let Ok(profile) = crate::read_instance(paths, instance_id) else {
        return Compared::default();
    };
    let game = crate::instance::paths_for(paths, &profile).game_directory(instance_id);
    let known = origin::latest(paths, instance_id);
    let mut hashes = Hashes::open(paths);

    let mut out = Compared::default();
    for (file, path) in present(&game) {
        let sha1 = match depth {
            Depth::Quick => hashes.of(&file, &path),
            Depth::Full => hashes.reread(&file, &path),
        };
        let Some(sha1) = sha1 else {
            continue;
        };
        match known.get(&file) {
            // 对得上，什么都不说。绝大多数文件走的是这一条。
            Some(record) if record.sha1 == sha1 => {}
            Some(record) => out.changes.push(changed(record, file, &path, sha1)),
            None => out.first_seen.push(Entry {
                file,
                sha1,
                version: declared_version(&path),
                origin: Origin::Adopted,
            }),
        }
    }

    // 按文件名排序，好让两次对账的结果能直接比对。
    out.changes
        .sort_by(|left, right| left.file.cmp(&right.file));
    out.first_seen
        .sort_by(|left, right| left.file.cmp(&right.file));
    hashes.save(paths);
    out
}

/// 这次对账有什么要说给用户的。
///
/// 绝大多数时候是空列表。**「单个文件变了，版本号也跟着变了」不说**——那是用户
/// 自己换了版本，或者在别的启动器里更新了，外部实例上天天发生。说一次是提醒，
/// 说十次是噪音，而噪音的终点是用户把整个功能关掉。
///
/// 放过它安全吗：改一下 `fabric.mod.json` 里的版本号就能躲过这一条。但另外两
/// 条是独立的——改了四十个 jar 照样触发「一批」，变成上游不认识的文件照样触发
/// [`kind::LEFT_UPSTREAM`]。
pub fn notices(paths: &DataPaths, instance_id: &str, compared: &Compared) -> Vec<Notice> {
    let mut out = Vec::new();

    // 账本自己对不上要先讲：它决定后面几句话有多可信。
    if let Some(line) = origin::broken_at(&origin::records(paths, instance_id)) {
        out.push(Notice {
            id: kind::LEDGER_BROKEN.to_owned(),
            kind: kind::LEDGER_BROKEN.to_owned(),
            args: args([("line", (line + 1).to_string())]),
            action: None,
        });
    }

    let silent = compared.silently_rewritten();
    let batch = compared.largest_batch();
    if batch >= AT_ONCE {
        out.push(Notice {
            id: kind::REWRITTEN_TOGETHER.to_owned(),
            kind: kind::REWRITTEN_TOGETHER.to_owned(),
            args: args([("count", batch.to_string())]),
            action: None,
        });
    }

    let left: Vec<&Change> = compared
        .changes
        .iter()
        .filter(|change| change.left_the_upstream())
        .collect();
    if let Some(first) = left.first() {
        out.push(Notice {
            id: kind::LEFT_UPSTREAM.to_owned(),
            kind: kind::LEFT_UPSTREAM.to_owned(),
            args: args([
                ("count", left.len().to_string()),
                ("file", display_name(&first.file)),
            ]),
            action: None,
        });
    }

    // 已经作为「一批」讲过的不再拆开重说。剩下的最多两个，一个文件一条——
    // 这样文案里不必出现「共 1 个文件」这种为了凑数而别扭的说法。
    if batch < AT_ONCE {
        for change in silent {
            out.push(Notice {
                id: format!("{}:{}", kind::SILENT_REWRITE, change.file),
                kind: kind::SILENT_REWRITE.to_owned(),
                args: args([("file", display_name(&change.file))]),
                action: None,
            });
        }
    }

    out
}

fn args<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// `mods/sodium.jar` → `sodium.jar`。文案里说的是文件，不是路径。
fn display_name(file: &str) -> String {
    file.rsplit('/').next().unwrap_or(file).to_owned()
}

/// 把第一次见到的那些记成基线。
///
/// 变化的那些不动——它们还没说给用户听。第一次见到一个文件不产出任何提示（见
/// 模块开头），所以这一步随时可以做，越早做，下一次它变的时候越有得比。
pub fn accept_new(paths: &DataPaths, instance_id: &str, compared: &Compared) {
    origin::record(paths, instance_id, compared.first_seen.clone());
}

/// 把这次看到的样子全部记成新的基线，包括变化的那些。
///
/// 调用方**确实把结果交给用户之后**才做这一步。同一次变化只该说一遍，但也不能
/// 在用户看到之前就被吃掉。
pub fn accept(paths: &DataPaths, instance_id: &str, compared: &Compared) {
    let mut entries = compared.first_seen.clone();
    entries.extend(compared.changes.iter().map(|change| Entry {
        file: change.file.clone(),
        sha1: change.now.clone(),
        version: change.version_now.clone(),
        origin: Origin::Adopted,
    }));
    origin::record(paths, instance_id, entries);
}

/// 现在这个实例有什么要说的。没有就是空列表。
///
/// 用便宜的那一档：打开实例和点启动之前都会调它，那两个时刻有人在等。顺手把
/// 第一次见到的文件记成基线——那不是信号，但越早记下越好。
///
/// 会去问一次上游。问不到就算了：离线时少一条信号，不是多一条告警。
pub async fn look(paths: &DataPaths, instance_id: &str) -> Vec<Notice> {
    let mut compared = compare(paths, instance_id, Depth::Quick);
    accept_new(paths, instance_id, &compared);
    if !compared.changes.is_empty() {
        let _ = ask_upstream(&mut compared).await;
    }
    notices(paths, instance_id, &compared)
}

/// 游戏退出之后那一遍。
///
/// 读全部文件，不看缓存里的时间戳——伪造过时间戳的改动只有这一遍看得见。
///
/// 结果不需要存到别处：这一遍会把每个文件真正的 sha1 刷回哈希缓存，于是下一次
/// [`look`] 用便宜那一档也照样能把它们比出来。这里只把第一次见到的记成基线。
pub(crate) fn after_session(paths: &DataPaths, instance_id: &str) {
    let compared = compare(paths, instance_id, Depth::Full);
    accept_new(paths, instance_id, &compared);
}

/// 接手一个已有实例时，把当时看到的一切原样记下来。
///
/// 这不是一次判定。那些文件从哪来我们不知道，那段历史发生在 Fern 之前，谁也补
/// 不回来。能诚实说出口的只有「接手时它长这样」——而这一句就够了：从这一刻起
/// 的每一次变化都有了参照物。
///
/// 返回记了几条。
pub fn adopt(paths: &DataPaths, instance_id: &str) -> usize {
    let compared = compare(paths, instance_id, Depth::Quick);
    let count = compared.changes.len() + compared.first_seen.len();
    accept(paths, instance_id, &compared);
    count
}

fn changed(record: &Record, file: String, path: &Path, sha1: String) -> Change {
    Change {
        file,
        was: record.sha1.clone(),
        now: sha1,
        recorded_at: record.at,
        modified_at: modified_at(path),
        version_was: record.version.clone(),
        version_now: declared_version(path),
        origin_was: record.origin.clone(),
        was_published: None,
        now_published: None,
    }
}

/// 拿变化前后的 sha1 去问一遍上游，把 [`Change::left_the_upstream`] 需要的那两
/// 个字段填上。
///
/// 这是本地基线之外的**第二条腿**，也是唯一一条参照物不在本机的：一个 sha1 能
/// 不能在 Modrinth 上查到，这个事实不存储在这台机器上，每次都能重新验证。两条
/// 腿还会互相验证——本地账说「它从没变过」，而上游说「这个 sha1 我不认识」，
/// 那就是本地账在撒谎。
///
/// 一次批量请求问完全部。问不到（离线、上游挂了）就保持 `None`：**不知道要
/// 说成不知道**，不能当成「上游不认识」，那会把每一次断网都变成一次告警。
pub async fn ask_upstream(compared: &mut Compared) -> anyhow::Result<()> {
    let mut hashes: Vec<String> = Vec::with_capacity(compared.changes.len() * 2);
    for change in &compared.changes {
        hashes.push(change.was.to_ascii_lowercase());
        hashes.push(change.now.to_ascii_lowercase());
    }
    hashes.sort_unstable();
    hashes.dedup();
    if hashes.is_empty() {
        return Ok(());
    }

    let known = crate::supply::known_files(&hashes).await?;
    for change in &mut compared.changes {
        change.was_published = Some(known.contains_key(&change.was.to_ascii_lowercase()));
        change.now_published = Some(known.contains_key(&change.now.to_ascii_lowercase()));
    }
    Ok(())
}

/// 那几个目录里现在躺着哪些文件，键是相对游戏目录的路径。
fn present(game: &Path) -> BTreeMap<String, std::path::PathBuf> {
    let mut out = BTreeMap::new();
    for directory in WATCHED {
        let Ok(listing) = std::fs::read_dir(game.join(directory)) else {
            continue;
        };
        for item in listing.flatten() {
            if !item.metadata().is_ok_and(|data| data.is_file()) {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            out.insert(format!("{directory}/{name}"), item.path());
        }
    }
    out
}

/// 模组在 jar 里自己声明的版本号。读不到（资源包、光影、读不懂的 jar）就是
/// `None`——猜一个比没有更糟。
pub(crate) fn declared_version(path: &Path) -> Option<String> {
    crate::instance::mods::declared_version(path)
}

fn modified_at(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn paths(tag: &str) -> DataPaths {
        let root =
            std::env::temp_dir().join(format!("fern-integrity-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        DataPaths::new(root)
    }

    /// 一个能被读出版本号的最小 fabric 模组，外加一段用来改变内容的填充。
    fn jar(path: &Path, version: &str, filler: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).expect("create"));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("fabric.mod.json", options)
            .expect("start");
        writer
            .write_all(format!(r#"{{"id":"x","name":"X","version":"{version}"}}"#).as_bytes())
            .expect("write");
        if !filler.is_empty() {
            writer.start_file("payload.class", options).expect("start");
            writer.write_all(filler.as_bytes()).expect("write");
        }
        writer.finish().expect("finish");
    }

    fn instance(paths: &DataPaths, name: &str) -> (String, std::path::PathBuf) {
        let profile = crate::create_instance(paths, name, "1.21.1").expect("create");
        let id = profile.id.as_str().to_owned();
        let game = crate::instance::paths_for(paths, &profile).game_directory(&id);
        (id, game)
    }

    #[test]
    fn taking_stock_covers_the_directories_that_hold_other_peoples_code() {
        let paths = paths("watched");
        let (id, game) = instance(&paths, "接手");
        jar(&game.join("mods/alpha.jar"), "1.0", "");
        // 关掉的模组也要清点：它随时能被开回来，那时才发现没有参照物就晚了。
        jar(&game.join("mods/beta.jar.disabled"), "1.0", "");
        std::fs::create_dir_all(game.join("resourcepacks")).expect("mkdir");
        std::fs::write(game.join("resourcepacks/pack.zip"), b"pack").expect("write");
        // 存档天天变，盯着它只会让真正的信号被淹掉。
        std::fs::create_dir_all(game.join("saves/世界")).expect("mkdir");
        std::fs::write(game.join("saves/世界/level.dat"), b"dat").expect("write");

        assert_eq!(adopt(&paths, &id), 3);
        let files: Vec<String> = origin::latest(&paths, &id).into_keys().collect();
        assert_eq!(
            files,
            [
                "mods/alpha.jar",
                "mods/beta.jar.disabled",
                "resourcepacks/pack.zip"
            ]
        );
        assert_eq!(origin::broken_at(&origin::records(&paths, &id)), None);
    }

    #[test]
    fn a_file_nobody_recorded_is_not_a_signal_it_is_just_a_new_baseline() {
        let paths = paths("first-seen");
        let (id, game) = instance(&paths, "拖进来的");
        // 用户绕开界面，直接用文件管理器把 jar 放进去。
        jar(&game.join("mods/dragged.jar"), "1.0", "");

        let compared = compare(&paths, &id, Depth::Full);
        assert!(compared.changes.is_empty(), "第一次见到不该是一次变化");
        assert_eq!(compared.first_seen.len(), 1);
        assert_eq!(compared.first_seen[0].version.as_deref(), Some("1.0"));

        // 记成基线之后就安静了。
        accept(&paths, &id, &compared);
        assert!(compare(&paths, &id, Depth::Full).is_empty());
    }

    #[test]
    fn rewriting_a_recorded_jar_without_touching_its_version_is_the_shape_that_matters() {
        let paths = paths("infected");
        let (id, game) = instance(&paths, "被改过");
        let path = game.join("mods/sodium.jar");
        jar(&path, "0.6.13", "");
        adopt(&paths, &id);

        // 版本号一个字没动，内容变了——往现有 jar 里追加东西就长这样。
        jar(&path, "0.6.13", "definitely not a mod");

        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 1);
        let change = &compared.changes[0];
        assert_eq!(change.file, "mods/sodium.jar");
        assert_ne!(change.was, change.now);
        assert!(
            change.version_stayed_put(),
            "版本号没变才是这条信号的全部意义"
        );
    }

    #[test]
    fn a_normal_update_changes_the_version_too() {
        let paths = paths("updated");
        let (id, game) = instance(&paths, "正常更新");
        let path = game.join("mods/sodium.jar");
        jar(&path, "0.6.13", "");
        adopt(&paths, &id);

        // 用户自己换了一个新版本，文件名没变。
        jar(&path, "0.6.14", "");

        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 1);
        assert!(
            !compared.changes[0].version_stayed_put(),
            "换版本会带来版本号变化，这不该和感染长得一样"
        );
    }

    #[test]
    fn a_resource_pack_without_metadata_never_claims_its_version_stayed_put() {
        let paths = paths("no-metadata");
        let (id, game) = instance(&paths, "资源包");
        std::fs::create_dir_all(game.join("resourcepacks")).expect("mkdir");
        let path = game.join("resourcepacks/pack.zip");
        std::fs::write(&path, b"one").expect("write");
        adopt(&paths, &id);
        std::fs::write(&path, b"two").expect("write");

        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 1);
        // 两边都读不到版本号，拿空值比空值比出来的不是事实。
        assert!(!compared.changes[0].version_stayed_put());
    }

    #[test]
    fn many_jars_rewritten_at_once_shows_up_as_one_batch() {
        let paths = paths("batch");
        let (id, game) = instance(&paths, "一起变");
        for index in 0..12 {
            jar(&game.join(format!("mods/mod{index}.jar")), "1.0", "");
        }
        adopt(&paths, &id);
        for index in 0..12 {
            jar(
                &game.join(format!("mods/mod{index}.jar")),
                "1.0",
                "same payload everywhere",
            );
        }

        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 12);
        assert_eq!(
            compared.largest_batch(),
            12,
            "十二个文件在同一个窗口里改完，这才是自我复制的样子"
        );
        assert!(compared.changes.iter().all(|it| it.version_stayed_put()));
    }

    #[test]
    fn comparing_does_not_swallow_the_event() {
        let paths = paths("no-swallow");
        let (id, game) = instance(&paths, "别吃掉");
        let path = game.join("mods/a.jar");
        jar(&path, "1.0", "");
        adopt(&paths, &id);
        jar(&path, "1.0", "changed");

        // 对多少次账，说的都是同一件事——直到调用方明确接受它。
        assert_eq!(compare(&paths, &id, Depth::Full).changes.len(), 1);
        assert_eq!(compare(&paths, &id, Depth::Full).changes.len(), 1);

        let compared = compare(&paths, &id, Depth::Full);
        accept(&paths, &id, &compared);
        assert!(compare(&paths, &id, Depth::Full).is_empty());
    }

    #[test]
    fn not_having_asked_upstream_never_reads_as_a_verdict() {
        let paths = paths("unasked");
        let (id, game) = instance(&paths, "没问过");
        let path = game.join("mods/a.jar");
        jar(&path, "1.0", "");
        adopt(&paths, &id);
        jar(&path, "1.0", "changed");

        let mut compared = compare(&paths, &id, Depth::Full);
        let change = &compared.changes[0];
        // 没问就是不知道，不是「上游不认识」——否则每一次断网都成了一次告警。
        assert_eq!(change.was_published, None);
        assert!(!change.left_the_upstream());

        // 上游认得原来那份、不认得现在这份，才是这条信号成立的样子。
        let change = &mut compared.changes[0];
        change.was_published = Some(true);
        change.now_published = Some(false);
        assert!(change.left_the_upstream());

        // 两份都不认识说明不了什么：本地构建的模组本来就查不到。
        change.was_published = Some(false);
        assert!(!change.left_the_upstream());
    }

    fn kinds(paths: &DataPaths, id: &str, compared: &Compared) -> Vec<String> {
        notices(paths, id, compared)
            .into_iter()
            .map(|notice| notice.kind)
            .collect()
    }

    #[test]
    fn a_single_ordinary_update_says_nothing_at_all() {
        let paths = paths("quiet");
        let (id, game) = instance(&paths, "安静");
        let path = game.join("mods/sodium.jar");
        jar(&path, "0.6.13", "");
        adopt(&paths, &id);

        // 用户自己换了个版本，或者在别的启动器里更新了。这在外部实例上天天发生。
        jar(&path, "0.6.14", "");
        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 1);
        assert!(
            kinds(&paths, &id, &compared).is_empty(),
            "正常更新说十次就是噪音，而噪音的终点是用户把功能关掉"
        );
    }

    #[test]
    fn one_silently_rewritten_file_is_reported_on_its_own() {
        let paths = paths("one-silent");
        let (id, game) = instance(&paths, "一个");
        let path = game.join("mods/sodium.jar");
        jar(&path, "0.6.13", "");
        adopt(&paths, &id);
        jar(&path, "0.6.13", "appended");

        let compared = compare(&paths, &id, Depth::Full);
        let reported = notices(&paths, &id, &compared);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].kind, kind::SILENT_REWRITE);
        assert_eq!(reported[0].args["file"], "sodium.jar");
    }

    #[test]
    fn a_batch_is_reported_once_and_not_again_file_by_file() {
        let paths = paths("batch-notice");
        let (id, game) = instance(&paths, "一批");
        for index in 0..6 {
            jar(&game.join(format!("mods/mod{index}.jar")), "1.0", "");
        }
        adopt(&paths, &id);
        for index in 0..6 {
            jar(&game.join(format!("mods/mod{index}.jar")), "1.0", "payload");
        }

        let compared = compare(&paths, &id, Depth::Full);
        let reported = notices(&paths, &id, &compared);
        assert_eq!(
            reported
                .iter()
                .map(|it| it.kind.as_str())
                .collect::<Vec<_>>(),
            [kind::REWRITTEN_TOGETHER],
            "讲过「一批」就不该再把六个文件各说一遍"
        );
        assert_eq!(reported[0].args["count"], "6");
    }

    #[test]
    fn a_modpack_update_through_another_launcher_is_not_a_batch() {
        let paths = paths("modpack-update");
        let (id, game) = instance(&paths, "整合包更新");
        for index in 0..20 {
            jar(&game.join(format!("mods/mod{index}.jar")), "1.0", "");
        }
        adopt(&paths, &id);
        // 二十个文件同时变，但版本号都跟着变了——那是升级，不是改写。
        for index in 0..20 {
            jar(&game.join(format!("mods/mod{index}.jar")), "2.0", "newer");
        }

        let compared = compare(&paths, &id, Depth::Full);
        assert_eq!(compared.changes.len(), 20);
        assert!(kinds(&paths, &id, &compared).is_empty());
    }

    #[test]
    fn a_broken_ledger_is_reported_before_anything_else() {
        let paths = paths("broken-first");
        let (id, game) = instance(&paths, "账本坏了");
        let path = game.join("mods/sodium.jar");
        jar(&path, "0.6.13", "");
        adopt(&paths, &id);
        jar(&path, "0.6.13", "appended");

        // 把日志的第一行改掉，链从那里开始接不上。
        let log = paths.root.join("security").join(format!("{id}.jsonl"));
        let text = std::fs::read_to_string(&log).expect("read");
        std::fs::write(&log, text.replacen("\"sha1\":\"", "\"sha1\":\"0", 1)).expect("write");

        let compared = compare(&paths, &id, Depth::Full);
        let reported = kinds(&paths, &id, &compared);
        assert_eq!(
            reported.first().map(String::as_str),
            Some(kind::LEDGER_BROKEN),
            "「我接下来的话有多可信」得排在具体哪个文件变了前面"
        );
    }

    #[test]
    fn deleting_a_file_is_not_reported() {
        let paths = paths("deleted");
        let (id, game) = instance(&paths, "删掉的");
        let path = game.join("mods/a.jar");
        jar(&path, "1.0", "");
        adopt(&paths, &id);
        std::fs::remove_file(&path).expect("remove");

        // 用户删模组是最普通不过的操作，日历上不该留下一条告警。
        assert!(compare(&paths, &id, Depth::Full).is_empty());
    }
}
