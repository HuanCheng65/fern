//! 版本区间。「这个版本落在那个区间里吗」。
//!
//! 两处要用：崩溃规则的 `minecraft = ">=1.14"` 守卫，以及启动前预检查里模组
//! 声明的依赖区间。而模组世界里同时流通着两套写法——
//!
//! ```text
//! Fabric / Quilt   >=0.15.0   *   1.2.x   [">=1.0 <2.0", "=3.0"]   （semver 风）
//! Forge / Neo      [1.0,2.0)  [1.0,]  [1.0]                        （Maven 风）
//! ```
//!
//! 两套写法里都有「或」：Fabric 的数组、Maven 串接的几段。一段里的空格和逗号
//! 才是「与」。折成一层去解，`[">=1.0 <2.0", "=3.0"]` 会变成一个谁也满足不了的
//! 条件，而那正是它想放行的那个版本。所以 `||` 在这里是一个真的分隔符。
//!
//! **看不懂的区间，[`satisfies`] 当作满足，[`contains`] 如实说不知道。** 这一层
//! 是用来给用户提警告的，而一个基于误解的警告比没有警告更糟：他会去动一个本来
//! 没问题的模组。宁可漏报——但「漏报」往哪边倒要看问的是什么，所以判断留给调用
//! 方，见 [`contains`]。
//!
//! 还有一件事：**左边那个版本号也不一定是版本号。** 快照的 id（`25w14a`）不
//! 是语义化版本号，而模组写的区间是。那一半在 [`super::mcversion`]。

/// `version` 落在 `range` 里吗。看不懂就返回 `true`。
pub fn satisfies(range: &str, version: &str) -> bool {
    contains(range, version).unwrap_or(true)
}

/// `version` 落在 `range` 里吗。**`None` 是「看不懂」。**
///
/// [`satisfies`] 把看不懂当成满足，因为它服务的是「这个模组适配这个版本吗」，
/// 猜错要害得用户去动一个本来没问题的模组。但方向反过来的那个问题——「装了它
/// 会不会起不来」——必须自己处理 `None`：那一边把看不懂当成满足，就是凭空报出
/// 一条冲突。所以两种口径都留着，由调用方说清楚自己要哪一种。
pub fn contains(range: &str, version: &str) -> Option<bool> {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    // `||` 分开的几段是「满足其中任意一条」。Fabric 的数组写法在读进来时就
    // 折成了这个形状（见 `instance::jar` 的 `range_text`）。
    any(range.split("||"), |alternative| {
        let alternative = alternative.trim();
        if alternative.starts_with('[') || alternative.starts_with('(') {
            return maven(alternative, version);
        }
        // 逗号或空格分隔的多个条件，全部满足才算。
        all(
            alternative
                .split([',', ' '])
                .map(str::trim)
                .filter(|part| !part.is_empty()),
            |part| simple(part, version),
        )
    })
}

/// 三值的「或」：有一条确定成立就成立，否则只要还有看不懂的就是看不懂。
fn any<T>(items: impl Iterator<Item = T>, of: impl Fn(T) -> Option<bool>) -> Option<bool> {
    let mut unknown = false;
    let mut seen = false;
    for item in items {
        seen = true;
        match of(item) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unknown = true,
        }
    }
    // 一段都没有，和看不懂是一回事。
    if !seen || unknown { None } else { Some(false) }
}

/// 三值的「与」：有一条确定不成立就不成立，否则只要还有看不懂的就是看不懂。
fn all<T>(items: impl Iterator<Item = T>, of: impl Fn(T) -> Option<bool>) -> Option<bool> {
    let mut unknown = false;
    let mut seen = false;
    for item in items {
        seen = true;
        match of(item) {
            Some(false) => return Some(false),
            Some(true) => {}
            None => unknown = true,
        }
    }
    if !seen || unknown { None } else { Some(true) }
}

/// `>=1.2`、`<2.0`、`=1.2.3`、`1.2.x`、`1.2.3`。
fn simple(part: &str, version: &str) -> Option<bool> {
    for (prefix, accept) in [
        (">=", [false, true, true]),
        ("<=", [true, true, false]),
        ("==", [false, true, false]),
        (">", [false, false, true]),
        ("<", [true, false, false]),
        ("=", [false, true, false]),
    ] {
        if let Some(bound) = part.strip_prefix(prefix) {
            let ordering = compare(version, bound.trim());
            return Some(match ordering {
                std::cmp::Ordering::Less => accept[0],
                std::cmp::Ordering::Equal => accept[1],
                std::cmp::Ordering::Greater => accept[2],
            });
        }
    }
    // `~1.2` 与 `^1.2` 的上界规则各家不一，只当作下界——宁可漏报。
    if let Some(bound) = part.strip_prefix(['~', '^']) {
        return Some(compare(version, bound.trim()) != std::cmp::Ordering::Less);
    }
    // `1.20.x` / `1.20.*`：按前缀比。
    if let Some(prefix) = part
        .strip_suffix(".x")
        .or_else(|| part.strip_suffix(".X"))
        .or_else(|| part.strip_suffix(".*"))
    {
        return Some(version == prefix || version.starts_with(&format!("{prefix}.")));
    }
    // 剩下的应该是一个光秃秃的版本号，按相等算。不像版本号的，就是一种我们
    // 没见过的写法——不猜。
    if !part.starts_with(|character: char| character.is_ascii_digit()) {
        return None;
    }
    Some(compare(version, part) == std::cmp::Ordering::Equal)
}

/// `[1.0,2.0)`、`[1.0,]`、`(,2.0]`、`[1.0]`，逗号可以串接多段。
fn maven(range: &str, version: &str) -> Option<bool> {
    let mut rest = range;
    let mut sections = Vec::new();
    while let Some(start) = rest.find(['[', '(']) {
        let open = rest.as_bytes()[start];
        let Some(end) = rest[start..].find([']', ')']).map(|offset| start + offset) else {
            // 括号没闭上，看不懂。
            return None;
        };
        let close = rest.as_bytes()[end];
        sections.push((rest[start + 1..end].to_owned(), open == b'[', close == b']'));
        rest = &rest[end + 1..];
    }
    // 串接的几段之间是「或」，一段都没解析出来就是看不懂。
    any(sections.iter(), |(body, lower, upper)| {
        Some(within(body, *lower, *upper, version))
    })
}

fn within(body: &str, lower_closed: bool, upper_closed: bool, version: &str) -> bool {
    let (low, high) = match body.split_once(',') {
        Some((low, high)) => (low.trim(), high.trim()),
        // `[1.0]` 是「正好这一个」。
        None => return compare(version, body.trim()) == std::cmp::Ordering::Equal,
    };
    if !low.is_empty() {
        let ordering = compare(version, low);
        let ok = ordering == std::cmp::Ordering::Greater
            || (lower_closed && ordering == std::cmp::Ordering::Equal);
        if !ok {
            return false;
        }
    }
    if !high.is_empty() {
        let ordering = compare(version, high);
        let ok = ordering == std::cmp::Ordering::Less
            || (upper_closed && ordering == std::cmp::Ordering::Equal);
        if !ok {
            return false;
        }
    }
    true
}

/// 逐段比较。数字段按数值比，其余按字典序——`1.10` 要大于 `1.9`。
pub fn compare(left: &str, right: &str) -> std::cmp::Ordering {
    let mut left = segments(left);
    let mut right = segments(right);
    loop {
        match (left.next(), right.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            // 段数少的那个补零：`1.20` 和 `1.20.0` 是同一个版本。
            (Some(one), None) => {
                if one != "0" {
                    return compare_segment(one, "0");
                }
            }
            (None, Some(other)) => {
                if other != "0" {
                    return compare_segment("0", other);
                }
            }
            (Some(one), Some(other)) => {
                let ordering = compare_segment(one, other);
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

fn segments(version: &str) -> impl Iterator<Item = &str> {
    let version = version.trim();
    // 结尾那一道分隔符是有意义的，不能跟着空段一起丢掉：`>=1.21.6-` 是模组声明
    // 「支持 1.21.6 的快照」的标准写法——空的先行版本段比任何一个先行版本段都
    // 小，于是 `1.21.6-alpha.25.15.a` 落在它上面，而 `1.21.6` 本身也落在上面。
    // 丢掉它，这个区间就退化成 `>=1.21.6`，为快照发布的模组会被判成不兼容。
    let open_ended = version.ends_with(['.', '-', '+', '_']);
    version
        .split(['.', '-', '+', '_'])
        .filter(|part| !part.is_empty())
        .chain(open_ended.then_some(""))
}

fn compare_segment(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        // 数字段大于文本段：`1.21` 比 `1.21-rc1` 新，而 rc 段是文本。
        (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
        (Err(_), Ok(_)) => std::cmp::Ordering::Less,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_compare_as_numbers_not_as_text() {
        // 字典序会说 1.10 < 1.9，那是启动器里最经典的一个错。
        assert_eq!(compare("1.10", "1.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare("1.20", "1.20.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare("1.21", "1.21-rc1"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn semver_style_ranges() {
        assert!(satisfies(">=0.15.0", "0.16.9"));
        assert!(!satisfies(">=0.15.0", "0.14.0"));
        assert!(satisfies("*", "随便什么"));
        assert!(satisfies("1.20.x", "1.20.4"));
        assert!(!satisfies("1.20.x", "1.21"));
        assert!(satisfies(">=1.20 <1.22", "1.21.1"));
        assert!(!satisfies(">=1.20 <1.22", "1.22"));
    }

    #[test]
    fn maven_style_ranges() {
        assert!(satisfies("[1.20,1.22)", "1.21.1"));
        assert!(!satisfies("[1.20,1.22)", "1.22"));
        assert!(satisfies("[1.20,]", "1.25"));
        assert!(satisfies("[1.20.1]", "1.20.1"));
        assert!(!satisfies("[1.20.1]", "1.20.2"));
        assert!(satisfies("(,1.20]", "1.19"));
        // 串接的多段，命中任意一段即可。
        assert!(satisfies("[1.16,1.17),[1.20,1.21)", "1.20.4"));
        assert!(!satisfies("[1.16,1.17),[1.20,1.21)", "1.18"));
    }

    /// `>=1.21.6-` 是「1.21.6 连同它的快照」，不是 `>=1.21.6`。
    #[test]
    fn a_trailing_dash_opens_a_range_up_to_the_pre_releases() {
        assert!(satisfies(">=1.21.6-", "1.21.6-alpha.25.15.a"));
        assert!(satisfies(">=1.21.6-", "1.21.6"));
        assert!(satisfies(">=1.21.6-", "1.21.7"));
        assert!(!satisfies(">=1.21.6-", "1.21.5"));
        // 没有那道横杠时，快照比正式版旧。
        assert!(!satisfies(">=1.21.6", "1.21.6-alpha.25.15.a"));
        // 空的先行版本段排在所有先行版本段前面，也排在正式版前面。
        assert_eq!(compare("1.21.6-", "1.21.6"), std::cmp::Ordering::Less);
        assert_eq!(
            compare("1.21.6-", "1.21.6-alpha.1"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn anything_unparseable_counts_as_satisfied() {
        // 宁可漏报：一个基于误解的警告会让用户去动一个本来没问题的模组。
        assert!(satisfies("完全看不懂的东西", "1.21"));
        assert!(satisfies("[1.20", "0.1"));
        assert!(satisfies("^1.2", "9.9"));
        // 但「看不懂」本身是拿得到的，方向反过来的那些判断要靠它。
        assert_eq!(contains("完全看不懂的东西", "1.21"), None);
        assert_eq!(contains("[1.20", "0.1"), None);
        assert_eq!(contains("", "1.21"), Some(true));
        assert_eq!(contains("*", "1.21"), Some(true));
        assert_eq!(contains(">=1.21", "1.20"), Some(false));
    }

    /// `||` 分开的几段是「或」，一段里的空格是「与」。
    ///
    /// LambDynamicLights 写的是 `["~1.21.5- <1.21.6-", "=1.21.6-alpha.25.14.craftmine"]`：
    /// 第一段划出 1.21.5 系列，第二段单独把那个愚人节快照加回来。两段拉平成一条
    /// 用空格连起来，就成了「既要在 1.21.6 以下、又要正好是那个快照」——谁也满足
    /// 不了，于是一个明明写着支持它的模组被报成不适配。
    #[test]
    fn alternatives_are_an_or_not_an_and() {
        const RANGE: &str = "~1.21.5- <1.21.6- || =1.21.6-alpha.25.14.craftmine";
        const CRAFTMINE: &str = "1.21.6-alpha.25.14.craftmine";

        assert!(satisfies(RANGE, CRAFTMINE));
        // 第一段自己确实容不下它——正是这一段当初把整条判成了不满足。
        assert!(!satisfies("~1.21.5- <1.21.6-", CRAFTMINE));
        // 第一段照常管着 1.21.5 那一支。
        assert!(satisfies(RANGE, "1.21.5"));
        // 两段都不沾的仍然报不满足。
        assert!(!satisfies(RANGE, "1.22"));
        assert!(!satisfies(RANGE, "1.20.1"));

        // 一段确定成立就够了，哪怕另一段看不懂。
        assert_eq!(contains("看不懂 || >=1.0", "2.0"), Some(true));
        // 都不成立、但有看不懂的，就是看不懂。
        assert_eq!(contains("看不懂 || >=9.0", "2.0"), None);
    }

    /// Maven 那边串接的几段同样是「或」。
    #[test]
    fn concatenated_maven_sections_are_an_or() {
        assert!(satisfies("[1.0,2.0),[3.0,4.0)", "3.5"));
        assert!(satisfies("[1.0,2.0),[3.0,4.0)", "1.5"));
        assert!(!satisfies("[1.0,2.0),[3.0,4.0)", "2.5"));
    }
}
