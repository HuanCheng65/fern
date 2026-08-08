fn main() {
    // 版本号说明不了「你手上是哪一次构建」。发一个测试包给别人，`0.1.0` 什么
    // 都对不上，`0.1.0 (a1b2c3d)` 才对得上——而关于页第一行就是这个。
    println!("cargo:rustc-env=FERN_COMMIT={}", commit());
    println!("cargo:rustc-env=FERN_BUILD_DATE={}", today());
    // 换了提交要重新生成，否则显示的还是上一次的哈希。
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    the_two_version_numbers_must_agree();
    tauri_build::build()
}

/// `tauri.conf.json` 的 `version` 必须和这个 crate 的版本一字不差。
///
/// 这两个数字**不是同一个来源**，而且用它们的是两个不同的人：
///
/// - 关于页显示 `CARGO_PKG_VERSION`（见 `lib.rs` 的 `about`）。
/// - 自更新拿 `PackageInfo::version` 和服务器上的清单比大小，而 tauri-codegen
///   的逻辑是「`config.version` 有值就用它，没值才回落到 `CARGO_PKG_VERSION`」。
///
/// 所以发版时漏改一处，症状是**关于页显示 0.2.0，更新器却以为自己还是 0.1.0，
/// 于是每次检查都提示同一个更新，装完还提示**。这个 bug 没有任何测试会失败——
/// 两个文件各自都是合法的，只是不相等。只能在这里当场停下。
///
/// 反过来问过一次「那把 `version` 从 `tauri.conf.json` 里删掉不就只剩一个了」：
/// 不行。`tauri-build` 只在 `config.version` 有值时才写 Windows 的版本资源，
/// 删掉之后 exe 的「属性 → 详细信息」里一片空白，而一个没有版本资源的未签名
/// exe 在杀软眼里更可疑。
fn the_two_version_numbers_must_agree() {
    println!("cargo:rerun-if-changed=tauri.conf.json");

    let crate_version = env!("CARGO_PKG_VERSION");
    let config = std::fs::read_to_string("tauri.conf.json").expect("读不到 tauri.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(&config).expect("tauri.conf.json 不是合法 JSON");

    let Some(config_version) = config.get("version").and_then(|value| value.as_str()) else {
        panic!(
            "tauri.conf.json 没有 version 字段。它必须写成 \"{crate_version}\"——\
             缺了它 Windows 的 exe 不会带版本资源。"
        );
    };

    assert_eq!(
        config_version, crate_version,
        "版本号对不上：tauri.conf.json 是 {config_version}，Cargo.toml 是 {crate_version}。\
         两处都要改成同一个值，见 docs/fern-update-design.md 的版本号一节。"
    );
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
