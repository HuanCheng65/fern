//! 这台机器长什么样，用来求值 rules（文档 §1.4）。
//!
//! 补全和启动都要一份 `RuleContext`，而且**必须是同一份**：补全按 A 决定下
//! 哪些库、启动按 B 决定哪些进 classpath，差一条就是「文件明明下好了却说
//! 缺」。之前这两处各写了一份几乎相同的构造函数，`os_version` 就是在其中
//! 一处被漏成空串的——合并到这里，以后只有一个地方能写错。

use std::{collections::HashMap, sync::OnceLock};

use fern_meta::RuleContext;

/// Mojang 用的操作系统名，和 Rust 的叫法对不上。
pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// 系统版本号，给 `os.version` 的正则匹配用。
///
/// 元数据里确实有这种规则——1.16.5 就带一条 `windows` + `^10\.` 的 JVM 参数，
/// 那是给 Windows 10 上老 LWJGL 的兼容开关。之前这里恒为空串，那条规则永远
/// 求值为假，Windows 10 上跑 1.16.5 就少了那个参数。
///
/// 查一次就够，macOS 上要起一个子进程。
pub fn os_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(detect_os_version)
}

#[cfg(windows)]
fn detect_os_version() -> String {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    // GetVersionEx 在没有兼容性清单时会谎报（永远说 6.2），RtlGetVersion 不会。
    // SAFETY: 按文档填好 dwOSVersionInfoSize 之后，它只写这一个结构体。
    unsafe {
        let mut info: OSVERSIONINFOW = std::mem::zeroed();
        info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
        if RtlGetVersion(&mut info) == 0 {
            format!(
                "{}.{}.{}",
                info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
            )
        } else {
            String::new()
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_os_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_os_version() -> String {
    // 官方元数据里没有 linux 的版本规则，但第三方加载器可能有，给个真实值
    // 总好过给空串。
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

#[cfg(not(any(windows, unix)))]
fn detect_os_version() -> String {
    String::new()
}

/// 启动上下文里的开关（文档 §1.4）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    /// 实例设置里指定了窗口尺寸。
    pub custom_resolution: bool,
    /// 试玩模式。Fern 不启动试玩版，永远是 false。
    pub demo: bool,
    /// 启动后直接进某个存档或服务器。
    pub quick_play: Option<QuickPlay>,
}

/// 启动之后直接进哪里。
///
/// 元数据把这三个 feature 分开写：`has_quick_plays_support` 决定要不要
/// `--quickPlayPath`，另外两个各自决定单人还是多人那一条参数。所以这里也
/// 分开答，而不是给一个笼统的布尔。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickPlay {
    /// 存档目录名。
    World(String),
    /// `host` 或 `host:port`。
    Server(String),
}

/// 这台机器 + 这次启动的规则上下文。
///
/// feature 全部显式写进去，哪怕是 false。求值器比的是「键存在且值相等」，
/// 少一个键，要求 `false` 的规则就会被判为不匹配——现在的元数据只用到要求
/// `true` 的写法，所以漏掉也看不出问题，但那是碰巧。
pub fn context(features: Features) -> RuleContext {
    RuleContext {
        os_name: os_name().to_owned(),
        os_arch: std::env::consts::ARCH.to_owned(),
        os_version: os_version().to_owned(),
        features: HashMap::from([
            (
                "has_custom_resolution".to_owned(),
                features.custom_resolution,
            ),
            ("is_demo_user".to_owned(), features.demo),
            (
                "has_quick_plays_support".to_owned(),
                features.quick_play.is_some(),
            ),
            (
                "is_quick_play_singleplayer".to_owned(),
                matches!(features.quick_play, Some(QuickPlay::World(_))),
            ),
            (
                "is_quick_play_multiplayer".to_owned(),
                matches!(features.quick_play, Some(QuickPlay::Server(_))),
            ),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_meta::{OsRule, Rule, RuleAction, rules_allow};

    #[test]
    fn quick_play_answers_three_features_separately() {
        // 元数据把它们分开写：一条决定要不要 --quickPlayPath，另外两条各自
        // 决定单人还是多人。合成一个布尔的话，进服务器时会连单人那条参数
        // 一起带上。
        let none = context(Features::default()).features;
        assert!(!none["has_quick_plays_support"]);
        assert!(!none["is_quick_play_singleplayer"]);
        assert!(!none["is_quick_play_multiplayer"]);

        let world = context(Features {
            quick_play: Some(QuickPlay::World("新的世界".to_owned())),
            ..Features::default()
        })
        .features;
        assert!(world["has_quick_plays_support"]);
        assert!(world["is_quick_play_singleplayer"]);
        assert!(!world["is_quick_play_multiplayer"]);

        let server = context(Features {
            quick_play: Some(QuickPlay::Server("mc.example.net".to_owned())),
            ..Features::default()
        })
        .features;
        assert!(!server["is_quick_play_singleplayer"]);
        assert!(server["is_quick_play_multiplayer"]);
    }

    #[test]
    fn this_machine_reports_a_real_os_version() {
        // 空串会让每一条带版本正则的规则恒为假，那正是之前的 bug。
        assert!(
            !os_version().is_empty(),
            "拿不到系统版本号，带 os.version 的规则会全部失效"
        );
    }

    #[test]
    fn a_version_rule_can_actually_match() {
        // 1.16.5 里那条真实规则的形状：windows + ^10\.
        let major = os_version()
            .split(['.', '-'])
            .next()
            .unwrap_or_default()
            .to_owned();
        let rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some(os_name().to_owned()),
                arch: None,
                version: Some(format!("^{major}\\.")),
            }),
            features: None,
        };
        let evaluated = context(Features::default());
        assert!(
            rules_allow(Some(&[rule]), &evaluated),
            "os_version={:?} 匹配不上自己的主版本号",
            os_version()
        );
    }

    #[test]
    fn every_feature_is_stated_even_when_false() {
        let plain = context(Features::default());
        for key in [
            "has_custom_resolution",
            "is_demo_user",
            "has_quick_plays_support",
        ] {
            assert_eq!(
                plain.features.get(key),
                Some(&false),
                "{key} 没有显式给出，要求它为 false 的规则会被误判"
            );
        }

        let with_resolution = context(Features {
            custom_resolution: true,
            ..Features::default()
        });
        assert_eq!(
            with_resolution.features.get("has_custom_resolution"),
            Some(&true)
        );
    }

    #[test]
    fn the_os_name_follows_mojangs_spelling() {
        // Mojang 管 macOS 叫 osx，和 Rust 的 target_os 对不上。
        assert!(matches!(os_name(), "windows" | "osx" | "linux"));
        #[cfg(target_os = "macos")]
        assert_eq!(os_name(), "osx");
    }
}
