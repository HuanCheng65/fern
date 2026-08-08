//! Minecraft 的版本 id，翻成模组声明区间时用的那种版本号。
//!
//! 磁盘上那个 id 不是版本号。`25w15a`、`1.21.5-pre2`、`25w14craftmine`、
//! `26.2-snapshot-1`——它们是**名字**，第一段和 `1.21.5` 的第一段根本不是同一种
//! 东西。而 Fabric 与 Quilt 的模组写的是 `>=1.21.6-alpha.25.15.a` 这样的语义化
//! 版本号，因为 fabric-loader 在装载之前先把游戏版本归一化过了。拿 id 直接去比
//! 区间，比出来的是「装的每一个模组都不兼容」。
//!
//! 所以这里照 loader 那一套翻（`McVersionLookup`）。规则有三层：
//!
//! ```text
//! 1.21.5           1.21.5                        发行版原样
//! 26.2             26.2                          新的日期版本号也是发行版
//! 25w15a           1.21.6-alpha.25.15.a          快照：正式版 + 年.周.批次
//! 26.2-snapshot-1  26.2-alpha.1                  日期版本号下的快照
//! 1.21.5-pre2      1.21.5-beta.2                 预发布（1.16 及更早算 rc）
//! 1.21.5-rc1       1.21.5-rc.1                   候选
//! 24w14potato      1.20.5-alpha.24.12.potato     愚人节：查表
//! ```
//!
//! 两处「不讲道理」的地方，也是自己推推不出来的地方：
//!
//! - **愚人节版本和战斗测试在一张手写的对照表里**（[`special`]）。`24w14potato`
//!   派生自 24w12a，所以它的周数是 12 不是 14；`20w14infinite` 是 20.13.inf。
//!   按 id 上写的数字去算，算出来的是另一个版本。
//! - **快照属于哪个正式版**，loader 存了一张年/周对照表（[`snapshot_release`]）。
//!   我们优先用游戏自己写在 client jar 里的 `release_target`——那一份不会过期，
//!   而且 loader 也是优先用它。表只在没有 jar 时兜底，且**会随新版本过期**：
//!   最后一档是「这一年之后都算某个版本」，loader 每出一个正式版就改一次。
//!
//! 1.0 之前的 alpha / beta / classic 不翻（loader 那边另有一套编号）。那些版本
//! 上没有 Fabric 模组，翻不出来返回 `None`，调用方那时该做的是不比。

use std::sync::LazyLock;

use regex::Regex;

use super::ranges::compare;

/// 新的日期版本号：`26.1`、`26.1.1`、`26.1-snapshot-1`、`26.1-pre-1`、`26.1-rc-1`。
static DATE_BASED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{2}\.\d+(?:\.\d+)?)(?:-(snapshot|pre|rc)-(\d+))?$").unwrap());
/// 老的发行版号：`1.6`、`1.16.5`，外加一个重传时间戳。
static RELEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(1\.\d+(?:\.\d+)?)(?:-\d+)?$").unwrap());
/// `1.21.5-pre2`、`1.16.2 Pre-Release 3`。
static PRE_RELEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^.+(?:-pre| Pre-?[Rr]elease ?)(\d+)$").unwrap());
/// `1.21.5-rc1`、`1.16 Release Candidate 1`。
static RELEASE_CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^.+(?:-rc| RC| [Rr]elease Candidate )(\d+)$").unwrap());
/// `25w15a`、`Snapshot 16w02a`。周数的前导零要去掉，批次只有一个字母。
static SNAPSHOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:Snapshot )?(\d+)w0?(0|[1-9]\d*)([a-z])$").unwrap());
/// `1.18 Experimental Snapshot 1`、`1.18_experimental-snapshot-2`、`1.18-exp3`。
static EXPERIMENTAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^.+(?:-exp|(?:_deep_dark)?_experimental[_-]snapshot-|(?: Deep Dark)? [Ee]xperimental [Ss]napshot )(\d+)$",
    )
    .unwrap()
});

/// 把版本 id 翻成模组比得了的那个版本号。翻不出来是 `None`。
///
/// `release_target` 是游戏写在 client jar 的 `version.json` 里的正式版号。给了就
/// 用它——它是这个问题最权威的答案，也是 loader 优先用的那一个；没给就退回年/周
/// 对照表。
pub fn semantic(id: &str, release_target: Option<&str>) -> Option<String> {
    let id = id.trim();
    // 对照表最先查：那些 id 的规律不在版本号里，别的规则会把它们算错。
    if let Some(known) = special(id) {
        return Some(known.to_owned());
    }

    let release = release_target
        .map(str::trim)
        .filter(|target| is_release(target))
        .map(str::to_owned)
        .or_else(|| release_of(id))?;
    if id == release {
        return Some(release);
    }

    if let Some(caps) = DATE_BASED.captures(id) {
        let Some(kind) = caps.get(2) else {
            // 时间戳之类的尾巴，本体就是那个正式版。
            return Some(release);
        };
        let tag = if kind.as_str() == "snapshot" {
            "alpha"
        } else {
            kind.as_str()
        };
        return Some(format!("{release}-{tag}.{}", &caps[3]));
    }
    if RELEASE.is_match(id) {
        return Some(release);
    }
    if let Some(caps) = EXPERIMENTAL.captures(id) {
        return Some(format!("{release}-Experimental.{}", &caps[1]));
    }
    // 预发布和候选版的 id 以它们的正式版开头，快照不是。
    if id.starts_with(release.as_str()) {
        if let Some(caps) = RELEASE_CANDIDATE.captures(id) {
            let build: u32 = caps[1].parse().ok()?;
            // 1.16 的候选版接在它那 8 个预发布后面，loader 把编号整体加了 8。
            let build = if release == "1.16" { build + 8 } else { build };
            return Some(format!("{release}-rc.{build}"));
        }
        if let Some(caps) = PRE_RELEASE.captures(id) {
            // 1.16 及更早，预发布也叫候选版。注意判据是「正式版 ≤ 1.16」，
            // 所以 1.16.2 的预发布是 beta——1.16.2 比 1.16 新。
            let tag = if compare(&release, "1.16") == std::cmp::Ordering::Greater {
                "beta"
            } else {
                "rc"
            };
            return Some(format!("{release}-{tag}.{}", &caps[1]));
        }
        // 测试构建（`-tb3`）之类的不认。
        return None;
    }
    let caps = SNAPSHOT.captures(id)?;
    Some(format!(
        "{release}-alpha.{}.{}.{}",
        &caps[1], &caps[2], &caps[3]
    ))
}

/// 一个光秃秃的正式版号：`1.21`、`1.21.5`、`26.2`。
pub fn is_release(version: &str) -> bool {
    let segments: Vec<&str> = version.split('.').collect();
    segments.len() >= 2
        && segments
            .iter()
            .all(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
}

/// 这个 id 属于哪个正式版。
fn release_of(id: &str) -> Option<String> {
    if let Some(caps) = DATE_BASED.captures(id) {
        return Some(caps[1].to_owned());
    }
    if let Some(caps) = RELEASE.captures(id) {
        return Some(caps[1].to_owned());
    }
    // 带后缀的都从后缀那里切开。顺序照抄 loader：长的写法要排在短的前面，
    // 否则 `_experimental-snapshot-` 会先被 `-exp` 之外的规则切错。
    const MARKERS: [&str; 15] = [
        "_deep_dark_experimental_snapshot-",
        "_experimental-snapshot-",
        "-exp",
        "-tb",
        "-pre",
        "-rc",
        " Deep Dark Experimental Snapshot",
        " Experimental Snapshot",
        " experimental snapshot ",
        " Test Build",
        " Pre-Release",
        " Pre-release",
        " Prerelease",
        " RC",
        " Release Candidate",
    ];
    for marker in MARKERS {
        if let Some(position) = id.find(marker) {
            return Some(id[..position].to_owned());
        }
    }
    snapshot_release(id)
}

/// 一个周更快照奔着哪个正式版去。
///
/// 抄自 loader 的同名判断。**它会过期**：最后一档是「这一年之后都算 X」，而 X
/// 每出一个正式版就往前挪一次。所以 client jar 里的 `release_target` 优先——那是
/// 游戏自己写的，不会过期。
fn snapshot_release(id: &str) -> Option<String> {
    let caps = SNAPSHOT.captures(id)?;
    let year: u32 = caps[1].parse().ok()?;
    let week: u32 = caps[2].parse().ok()?;
    let release = match (year, week) {
        (26, 14) => "26.1.1", // 2026 愚人节
        (25, 41..) | (26.., _) => "1.21.11",
        (25, 31..=37) => "1.21.9",
        (25, 15..=21) => "1.21.6",
        (25, 2..=10) => "1.21.5",
        (24, 44..) => "1.21.4",
        (24, 33..=40) => "1.21.2",
        (24, 18..=21) => "1.21",
        (23, 51..) | (24, ..=14) => "1.20.5",
        (23, 40..=46) => "1.20.3",
        (23, 31..=35) => "1.20.2",
        (23, 12..=18) => "1.20",
        (23, ..=7) => "1.19.4",
        (22, 42..) => "1.19.3",
        (22, 24) => "1.19.1",
        (22, 11..=19) => "1.19",
        (22, 3..=7) => "1.18.2",
        (21, 37..=44) => "1.18",
        (20, 45..) | (21, ..=20) => "1.17",
        (20, 27..=30) => "1.16.2",
        (20, 6..=22) => "1.16",
        (19, 34..) => "1.15",
        (18, 43..) | (19, ..=14) => "1.14",
        (18, 30..=33) => "1.13.1",
        (17, 43..) | (18, ..=22) => "1.13",
        (17, 31) => "1.12.1",
        (17, 6..=18) => "1.12",
        (16, 50) => "1.11.1",
        (16, 32..=44) => "1.11",
        (16, 20..=21) => "1.10",
        (16, 14..=15) => "1.9.3",
        (15, 31..) | (16, ..=7) => "1.9",
        (14, 2..=34) => "1.8",
        (13, 47..=49) => "1.7.3",
        (13, 36..=43) => "1.7",
        (13, 16..=26) => "1.6",
        (13, 11..=12) => "1.5.1",
        (13, 1..=10) => "1.5",
        (12, 49..=50) => "1.4.6",
        (12, 32..=42) => "1.4",
        (12, 15..=30) => "1.3",
        (12, 3..=8) => "1.2",
        (11, 47..) | (12, ..=1) => "1.1",
        _ => return None,
    };
    Some(release.to_owned())
}

/// 那些自己一套的版本，loader 手写了一张对照表。
///
/// 愚人节版本、战斗测试、几个修补重传——它们派生自哪个版本，从 id 上是看不出来
/// 的（`24w14potato` 派生自 24w12a，`20w14infinite` 派生自 20w13b）。这张表照抄
/// loader 的 `normalizeSpecialVersionBase`，因为模组比的就是它给出的那个字符串。
fn special(id: &str) -> Option<&'static str> {
    Some(match id {
        "b1.2_02-dev" => "1.0.0-beta.2.dev",
        "b1.3-demo" => "1.0.0-beta.3.demo",
        "b1.6-trailer" | "b1.6-pre-trailer" => "1.0.0-beta.6.0.0",

        "13w02a-whitetexturefix" => "1.5-alpha.13.2.a.whitetexturefix",
        "13w04a-whitelinefix" => "1.5-alpha.13.4.a.whitelinefix",
        "1.5-whitelinefix" | "1.5-pre-whitelinefix" => "1.5-rc.whitelinefix",
        "13w12~" => "1.5.1-alpha.13.12.a",

        "2.0" => "1.5.2-2.0",
        "2.0-preview" => "1.5.2-2.0+preview",
        "2.0-red" | "2point0_red" | "af-2013-red" => "1.5.2-2.0+red",
        "2.0-purple" | "2point0_purple" | "af-2013-purple" => "1.5.2-2.0+purple",
        "2.0-blue" | "2point0_blue" | "af-2013-blue" => "1.5.2-2.0+blue",

        "15w14a" | "af-2015" => "1.8.4-alpha.15.14.a+loveandhugs",
        "1.RV-Pre1" | "af-2016" => "1.9.2-rv+trendy",
        "3D Shareware v1.34" | "af-2019" => "1.14-alpha.19.13.shareware",
        "20w14infinite" | "20w14~" | "af-2020" => "1.16-alpha.20.13.inf",
        "22w13oneblockatatime" | "22w13oneBlockAtATime" | "af-2022" => {
            "1.18.3-alpha.22.13.oneblockatatime"
        }
        "23w13a_or_b" | "af-2023" => "1.20-alpha.23.13.ab",
        "23w13a_or_b_original" => "1.20-alpha.23.13.ab+original",
        "24w14potato" | "af-2024" => "1.20.5-alpha.24.12.potato",
        "24w14potato_original" => "1.20.5-alpha.24.12.potato+original",
        "25w14craftmine" | "af-2025" => "1.21.6-alpha.25.14.craftmine",

        "1.14_combat-212796" | "1.14.3 - Combat Test" | "combat1" => "1.14.3-rc.4.combat.1",
        "1.14_combat-0" | "Combat Test 2" | "combat2" => "1.14.5-combat.2",
        "1.14_combat-3" | "Combat Test 3" | "combat3" => "1.14.5-combat.3",
        "1.15_combat-1" | "Combat Test 4" | "combat4" => "1.15-rc.3.combat.4",
        "1.15_combat-6" | "Combat Test 5" | "combat5" => "1.15.2-rc.2.combat.5",
        "1.16_combat-0" | "Combat Test 6" | "combat6" => "1.16.2-beta.3.combat.6",
        "1.16_combat-1" | "Combat Test 7" | "combat7" => "1.16.3-combat.7",
        "1.16_combat-2" | "Combat Test 7b" | "combat7b" => "1.16.3-combat.7.b",
        "1.16_combat-3" | "Combat Test 7c" | "combat7c" => "1.16.3-combat.7.c",
        "1.16_combat-4" | "Combat Test 8" | "combat8" => "1.16.3-combat.8",
        "1.16_combat-5" | "Combat Test 8b" | "combat8b" => "1.16.3-combat.8.b",
        "1.16_combat-6" | "Combat Test 8c" | "combat8c" => "1.16.3-combat.8.c",

        "26w14a" => "26.1.1-alpha.26.14.a",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::ranges::satisfies;
    use super::*;

    #[test]
    fn a_release_is_already_the_version_mods_declare() {
        assert_eq!(semantic("1.21.5", None).as_deref(), Some("1.21.5"));
        assert_eq!(semantic("1.21", None).as_deref(), Some("1.21"));
        // 新的日期版本号同样是发行版。
        assert_eq!(semantic("26.2", None).as_deref(), Some("26.2"));
        assert_eq!(semantic("26.1.1", None).as_deref(), Some("26.1.1"));
    }

    #[test]
    fn a_weekly_snapshot_takes_the_release_it_is_heading_for() {
        // client jar 说了就听它的。
        assert_eq!(
            semantic("25w15a", Some("1.21.6")).as_deref(),
            Some("1.21.6-alpha.25.15.a")
        );
        // 没说就查年/周表，答案一样。
        assert_eq!(
            semantic("25w15a", None).as_deref(),
            Some("1.21.6-alpha.25.15.a")
        );
        // 周数的前导零要去掉：loader 那边是 16.2，不是 16.02。
        assert_eq!(
            semantic("16w02a", None).as_deref(),
            Some("1.9-alpha.16.2.a")
        );
        // 表里没有的年份就是不知道，不猜。
        assert_eq!(semantic("09w15a", None), None);
    }

    #[test]
    fn a_pre_release_says_which_release_it_belongs_to() {
        assert_eq!(
            semantic("1.21.5-pre2", None).as_deref(),
            Some("1.21.5-beta.2")
        );
        assert_eq!(semantic("1.21.5-rc1", None).as_deref(), Some("1.21.5-rc.1"));
        // 1.16 及更早，预发布也算候选版；而判据是正式版 ≤ 1.16，所以 1.16.2
        // 的预发布是 beta——这一条 loader 的注释和代码不一致，代码说了算，
        // 战斗测试那张表也印证了它（1.16.2 Pre-release 3 是 1.16.2-beta.3）。
        assert_eq!(
            semantic("1.15.2-pre2", None).as_deref(),
            Some("1.15.2-rc.2")
        );
        assert_eq!(
            semantic("1.16.2-pre3", None).as_deref(),
            Some("1.16.2-beta.3")
        );
        // 1.16 的候选版接在它那 8 个预发布后面。
        assert_eq!(semantic("1.16-rc1", None).as_deref(), Some("1.16-rc.9"));
    }

    #[test]
    fn the_date_based_scheme_has_its_own_snapshots() {
        assert_eq!(
            semantic("26.2-snapshot-1", None).as_deref(),
            Some("26.2-alpha.1")
        );
        assert_eq!(semantic("26.2-pre-1", None).as_deref(), Some("26.2-pre.1"));
        assert_eq!(semantic("26.2-rc-2", None).as_deref(), Some("26.2-rc.2"));
    }

    /// 愚人节版本派生自哪一个，从 id 上看不出来。
    #[test]
    fn the_april_fools_versions_come_from_the_table() {
        // 周数是 12 不是 14：它派生自 24w12a。
        assert_eq!(
            semantic("24w14potato", None).as_deref(),
            Some("1.20.5-alpha.24.12.potato")
        );
        assert_eq!(
            semantic("25w14craftmine", None).as_deref(),
            Some("1.21.6-alpha.25.14.craftmine")
        );
        assert_eq!(
            semantic("20w14infinite", None).as_deref(),
            Some("1.16-alpha.20.13.inf")
        );
        // 表比 client jar 的说法优先——表里那些正是 release_target 推不出来的。
        assert_eq!(
            semantic("25w14craftmine", Some("1.21.6")).as_deref(),
            Some("1.21.6-alpha.25.14.craftmine")
        );
        // 长得像周更快照的愚人节版本也在表里，别按快照规则算。
        assert_eq!(
            semantic("26w14a", None).as_deref(),
            Some("26.1.1-alpha.26.14.a")
        );
    }

    /// 1.0 之前不翻，认不出来的也不翻。
    #[test]
    fn what_we_cannot_place_stays_unplaced() {
        assert_eq!(semantic("b1.7.3", None), None);
        assert_eq!(semantic("a1.2.6", None), None);
        assert_eq!(semantic("我的整合包", None), None);
        assert_eq!(semantic("", None), None);
    }

    /// 翻完之后，模组声明的区间才比得对。
    #[test]
    fn the_ranges_mods_declare_now_land_where_they_should() {
        let craftmine = semantic("25w14craftmine", None).expect("normalized");
        assert!(satisfies(">=1.21.6-alpha.25.14.craftmine", &craftmine));
        assert!(satisfies("~1.21.6-alpha.25.14.craftmine", &craftmine));
        assert!(satisfies(">=1.21.5", &craftmine));
        // 快照比它指向的正式版旧，这是语义化版本号本来的规矩。
        assert!(!satisfies(">=1.21.6", &craftmine));
        // 为上一个正式版编译的模组，声明里不含这个快照。
        assert!(!satisfies("1.21.5", &craftmine));
    }
}
