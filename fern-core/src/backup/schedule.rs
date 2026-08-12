//! 保留策略：哪些快照该留着，哪些该剪掉。
//!
//! 近期密、远期疏（docs/fern-backup-design.md §7）：
//!
//! | 时间 | 留多少 |
//! |---|---|
//! | 24 小时内 | 全留 |
//! | 30 天内 | 每天一份 |
//! | 更早 | 每月一份 |
//! | 手动拍的、打过标签的 | 永久 |
//!
//! 理由是「想回到哪一刻」的分辨率随时间衰减：今天下午装模组之前那一张要精确
//! 到分钟，三个月前的只需要「大概那阵子」。
//!
//! 每个桶留**最新的那一张**，不是最旧的。桶里最新的那张离桶的边界最近，
//! 也就离用户说的「那天结束时的状态」最近。

/// 一天有多少秒。
const DAY: u64 = 86_400;

/// 这段时间内一张都不剪。
const RECENT: u64 = DAY;

/// 这段时间内每天留一张。再往前按月。
const DAILY: u64 = 30 * DAY;

/// 该剪掉哪些。
///
/// `snapshots` 是 `(id, 拍摄时刻, 是否永久保留)`，顺序不限。返回的是要删的 id。
pub fn expired(snapshots: &[(String, u64, bool)], now: u64) -> Vec<String> {
    let mut ordered: Vec<&(String, u64, bool)> = snapshots.iter().collect();
    // 从新到旧：每个桶第一个遇到的就是要留下的那一张。
    ordered.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));

    let mut seen: Vec<(u8, u64)> = Vec::new();
    let mut expired = Vec::new();
    for (id, taken_at, pinned) in ordered {
        if *pinned {
            continue;
        }
        let age = now.saturating_sub(*taken_at);
        if age < RECENT {
            continue;
        }
        // 第一个数字是「按天分」还是「按月分」，免得两种桶的键值撞上。
        let bucket = if age < DAILY {
            (0, taken_at / DAY)
        } else {
            (1, month_of(*taken_at))
        };
        if seen.contains(&bucket) {
            expired.push(id.clone());
        } else {
            seen.push(bucket);
        }
    }
    expired
}

/// 一张快照，以及它引用了哪些对象。给 [`over_limit`] 用。
pub struct Held {
    pub instance: String,
    pub id: String,
    pub taken_at: u64,
    /// 手动拍的、打过标签的。永远不剪，哪怕已经超了上限。
    pub pinned: bool,
    pub objects: Vec<String>,
}

/// 总占用超了上限时，从最旧的开始剪到不超为止。返回该删的 `(实例, id)`。
///
/// 剪掉一张快照能腾出多少空间，不等于它引用的对象加起来有多大：对象是跨快照、
/// 跨实例去重的，还被别人引用着的那些删了也不会消失。所以这里维护一份引用
/// 计数，一张一张地减，**只有降到零的那些才算进腾出来的空间**——手动拍的那些
/// 从不参与，它们的引用一直压着，共享的对象也就一直在。
///
/// 一遍走完，不反复扫仓库：每删一张就重新量一次占用是几次全目录遍历，而这件事
/// 发生在刚拍完一张快照之后，那时磁盘已经忙过一轮了。
pub fn over_limit(
    held: &[Held],
    size_of: &dyn Fn(&str) -> u64,
    total: u64,
    limit: u64,
) -> Vec<(String, String)> {
    if total <= limit {
        return Vec::new();
    }

    let mut references: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for snapshot in held {
        for object in &snapshot.objects {
            *references.entry(object.as_str()).or_default() += 1;
        }
    }

    // 从旧到新。同一时刻的按 id 定序，免得两次运行剪掉不同的那一张。
    let mut ordered: Vec<&Held> = held.iter().filter(|snapshot| !snapshot.pinned).collect();
    ordered.sort_by(|left, right| {
        left.taken_at
            .cmp(&right.taken_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut freed = 0u64;
    let mut doomed = Vec::new();
    for snapshot in ordered {
        if total.saturating_sub(freed) <= limit {
            break;
        }
        for object in &snapshot.objects {
            let Some(count) = references.get_mut(object.as_str()) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                freed = freed.saturating_add(size_of(object));
            }
        }
        doomed.push((snapshot.instance.clone(), snapshot.id.clone()));
    }
    doomed
}

/// 这个时刻属于哪个自然月，编成一个数。
///
/// 自己算，不引日期库：为一个月份号背上一个依赖的版本策略和时区语义不划算。
/// 算法是 Howard Hinnant 的 civil_from_days——把三月当作一年的开头，于是闰日
/// 落在年末，不需要为它分情况。全程按 UTC，因为快照的时刻本来就是 UTC 秒，
/// 而「按月留一份」不需要贴合用户所在时区的月初。
fn month_of(seconds: u64) -> u64 {
    let days = (seconds / DAY) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year * 12 + month) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-08-08 12:00:00 UTC。
    ///
    /// 特意取正午：取午夜的话「三天前」和「三天前再一小时」会落在相邻的两个
    /// 自然日里，测的就不是分桶而是算术了。
    const NOW: u64 = 1_786_190_400;

    fn snapshot(id: &str, ago: u64) -> (String, u64, bool) {
        (id.to_owned(), NOW - ago, false)
    }

    #[test]
    fn everything_from_the_last_day_survives() {
        let recent = vec![
            snapshot("a", 60),
            snapshot("b", 3600),
            snapshot("c", DAY - 1),
        ];
        assert!(expired(&recent, NOW).is_empty());
    }

    #[test]
    fn one_a_day_within_the_month_and_one_a_month_before_that() {
        let snapshots = vec![
            // 同一天里的三张，只留最新那一张。
            snapshot("day3-late", 3 * DAY),
            snapshot("day3-mid", 3 * DAY + 3600),
            snapshot("day3-early", 3 * DAY + 7200),
            // 另一天，各自留一张。
            snapshot("day4", 4 * DAY),
            // 半年前的两张落在同一个月里。
            snapshot("old-a", 180 * DAY),
            snapshot("old-b", 182 * DAY),
        ];
        let mut gone = expired(&snapshots, NOW);
        gone.sort();
        assert_eq!(gone, vec!["day3-early", "day3-mid", "old-b"]);
    }

    #[test]
    fn a_label_or_a_manual_snapshot_is_never_pruned() {
        let snapshots = vec![
            ("kept".to_owned(), NOW - 900 * DAY, true),
            ("also-kept".to_owned(), NOW - 901 * DAY, true),
            ("gone".to_owned(), NOW - 902 * DAY, false),
            ("survivor".to_owned(), NOW - 903 * DAY, false),
        ];
        // 三张都在同一个月里：两张永久的不参与分桶，剩下两张留最新的一张。
        assert_eq!(expired(&snapshots, NOW), vec!["survivor"]);
    }

    fn held(id: &str, ago: u64, pinned: bool, objects: &[&str]) -> Held {
        Held {
            instance: "one".to_owned(),
            id: id.to_owned(),
            taken_at: NOW - ago,
            pinned,
            objects: objects.iter().map(|it| (*it).to_owned()).collect(),
        }
    }

    /// 每个对象都算 100 字节。
    fn flat(_: &str) -> u64 {
        100
    }

    #[test]
    fn nothing_is_cut_while_the_repository_fits() {
        let snapshots = vec![held("a", DAY, false, &["x"]), held("b", 0, false, &["y"])];
        assert!(over_limit(&snapshots, &flat, 200, 200).is_empty());
    }

    #[test]
    fn the_oldest_go_first_and_only_until_it_fits() {
        let snapshots = vec![
            held("newest", 0, false, &["c"]),
            held("oldest", 3 * DAY, false, &["a"]),
            held("middle", 2 * DAY, false, &["b"]),
        ];
        // 300 字节要压到 150 以下：剪掉最旧的腾出 100 还不够，再剪一张就够了。
        assert_eq!(
            over_limit(&snapshots, &flat, 300, 150),
            vec![
                ("one".to_owned(), "oldest".to_owned()),
                ("one".to_owned(), "middle".to_owned())
            ]
        );
    }

    /// 被别人也引用着的对象，删了不会腾出空间——所以不能算进账里。
    #[test]
    fn shared_objects_do_not_count_as_freed() {
        let snapshots = vec![
            held("kept-forever", 9 * DAY, true, &["shared"]),
            held("old", 3 * DAY, false, &["shared"]),
            held("new", 0, false, &["own"]),
        ];
        // `old` 只引用了一个手动那张也引用着的对象，剪掉它一个字节都腾不出来，
        // 于是只能接着剪下一张。
        assert_eq!(
            over_limit(&snapshots, &flat, 200, 50),
            vec![
                ("one".to_owned(), "old".to_owned()),
                ("one".to_owned(), "new".to_owned())
            ]
        );
    }

    /// 上限低到连手动那些都装不下时，能剪的都剪掉，然后停手。
    #[test]
    fn pinned_snapshots_survive_a_limit_they_cannot_meet() {
        let snapshots = vec![
            held("manual", 5 * DAY, true, &["a"]),
            held("auto", 4 * DAY, false, &["b"]),
        ];
        assert_eq!(
            over_limit(&snapshots, &flat, 200, 10),
            vec![("one".to_owned(), "auto".to_owned())]
        );
    }

    #[test]
    fn months_are_calendar_months() {
        // 2026-01-31 与 2026-02-01 相隔一天，但不在同一个月。
        let january = 1_769_817_600; // 2026-01-31 00:00:00 UTC
        assert_ne!(month_of(january), month_of(january + DAY));
        assert_eq!(month_of(january), month_of(january - 10 * DAY));
        // 闰年的二月末。
        let leap = 1_709_164_800; // 2024-02-29 00:00:00 UTC
        assert_eq!(month_of(leap), month_of(leap - DAY));
        assert_ne!(month_of(leap), month_of(leap + DAY));
    }
}
