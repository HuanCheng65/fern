fn main() {
    // 版本号说明不了「你手上是哪一次构建」。发一个测试包给别人，`0.1.0` 什么
    // 都对不上，`0.1.0 (a1b2c3d)` 才对得上——而关于页第一行就是这个。
    println!("cargo:rustc-env=FERN_COMMIT={}", commit());
    println!("cargo:rustc-env=FERN_BUILD_DATE={}", today());
    // 换了提交要重新生成，否则显示的还是上一次的哈希。
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    tauri_build::build()
}

/// 当前提交的短哈希。源码包里没有 `.git`，那时候留空而不是让构建失败。
fn commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// 构建日期，`YYYY-MM-DD`。
///
/// 自己从 Unix 秒算，不引日期库：构建脚本为了一行字符串多背一个依赖不值得。
/// 算法是 Howard Hinnant 的 civil_from_days，把三月当作一年的开头，于是闰日
/// 落在年末，不需要为它分情况。
fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
