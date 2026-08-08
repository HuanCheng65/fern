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
