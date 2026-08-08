//! 版本区间。「这个版本落在那个区间里吗」。
//!
//! 两处要用：崩溃规则的 `minecraft = ">=1.14"` 守卫，以及启动前预检查里模组
//! 声明的依赖区间。而模组世界里同时流通着两套写法——
//!
//! ```text
//! Fabric / Quilt   >=0.15.0   *   1.2.x        （semver 风）
//! Forge / Neo      [1.0,2.0)  [1.0,]  [1.0]    （Maven 风）
//! ```
//!
//! **看不懂的区间一律当作满足。** 这一层是用来给用户提警告的，而一个基于误解
//! 的警告比没有警告更糟：他会去动一个本来没问题的模组。宁可漏报。
//!
//! 还有一件事：**左边那个版本号也不一定是版本号。** 快照的 id（`25w14a`）不
//! 是语义化版本号，而模组写的区间是。那一半在 [`super::mcversion`]。

/// `version` 落在 `range` 里吗。看不懂就返回 `true`。
pub fn satisfies(range: &str, version: &str) -> bool {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return true;
    }
    if range.starts_with('[') || range.starts_with('(') {
        return maven(range, version);
    }
    // 逗号或空格分隔的多个条件，全部满足才算。
    range
        .split([',', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .all(|part| simple(part, version))
}

/// `>=1.2`、`<2.0`、`=1.2.3`、`1.2.x`、`1.2.3`。
fn simple(part: &str, version: &str) -> bool {
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
            return match ordering {
                std::cmp::Ordering::Less => accept[0],
                std::cmp::Ordering::Equal => accept[1],
                std::cmp::Ordering::Greater => accept[2],
            };
        }
    }
    // `~1.2` 与 `^1.2` 的上界规则各家不一，只当作下界——宁可漏报。
    if let Some(bound) = part.strip_prefix(['~', '^']) {
        return compare(version, bound.trim()) != std::cmp::Ordering::Less;
    }
    // `1.20.x` / `1.20.*`：按前缀比。
    if let Some(prefix) = part
        .strip_suffix(".x")
        .or_else(|| part.strip_suffix(".X"))
        .or_else(|| part.strip_suffix(".*"))
    {
        return version == prefix || version.starts_with(&format!("{prefix}."));
    }
    // 剩下的应该是一个光秃秃的版本号，按相等算。不像版本号的，就是一种我们
    // 没见过的写法——当作满足，别去猜。
    if !part.starts_with(|character: char| character.is_ascii_digit()) {
        return true;
    }
    compare(version, part) == std::cmp::Ordering::Equal
}

/// `[1.0,2.0)`、`[1.0,]`、`(,2.0]`、`[1.0]`，逗号可以串接多段。
fn maven(range: &str, version: &str) -> bool {
    let mut rest = range;
    let mut any = false;
    let mut matched = false;
    while let Some(start) = rest.find(['[', '(']) {
        let open = rest.as_bytes()[start];
        let Some(end) = rest[start..].find([']', ')']).map(|offset| start + offset) else {
            // 括号没闭上，看不懂。
            return true;
        };
        let close = rest.as_bytes()[end];
        let body = &rest[start + 1..end];
        any = true;
        matched |= within(body, open == b'[', close == b']', version);
        rest = &rest[end + 1..];
    }
    // 一段都没解析出来就是看不懂。
    !any || matched
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
    }
}
