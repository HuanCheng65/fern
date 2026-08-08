//! 自更新：判断「该不该更新」。
//!
//! 这个模块**只做判断**，不下载、不验签、不落盘。那一半是
//! `tauri-plugin-updater` 的活（见 [docs/fern-update-design.md](../../../docs/fern-update-design.md) §3），
//! 而且它需要 WebView 才编得动。分开的好处很实在：判断这一半是纯函数，
//! 能在任何机器上真跑测试——而更新逻辑的错误恰恰是那种「跑起来才知道」的错误。
//!
//! 清单有**两个读者**：这里读 `rollout` / `critical` / `minVersion` 决定要不要走，
//! Tauri 的更新器读 `version` / `platforms` 去下载和验签。同一个文件，
//! 因为 Tauri 的 `RemoteRelease` 没有 `deny_unknown_fields`，多出来的字段它会忽略。
//! 别把它拆成两个文件：两份清单会漂移，而漂移的那一天没人会注意到。

use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};

/// 更新源的根地址。
///
/// **发布前必须换成真正的域名。** 不能用 R2 自带的 `*.r2.dev`：官方说明那是给
/// 测试用的，有可变速率限制、带宽也会被限，而且没有缓存和 Workers 的能力。
pub const DEFAULT_ENDPOINT: &str = "https://dl.fern.huanchengfly.top";

/// 更新通道。
///
/// 只有两条。第三条（nightly）不该有自更新——每天变的东西自动装到用户机器上，
/// 风险和收益不成比例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    /// 这个通道的清单地址。
    ///
    /// 通道是路径的一段，不是查询参数：静态对象存储上「一个通道」就是「一个文件」，
    /// 而查询参数会被对象存储忽略——那样两个通道会拿到同一份清单，
    /// 而且这个错误在本地用文件服务器测试时不会出现。
    pub fn manifest_url(self, endpoint: &str) -> String {
        format!(
            "{}/{}/manifest.json",
            endpoint.trim_end_matches('/'),
            self.as_str()
        )
    }
}

/// 清单里的一个平台条目。字段名由 Tauri 的更新器定，不能改。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub url: String,
    pub signature: String,
}

/// 一个通道的清单。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pub_date: Option<String>,
    /// 放量百分比，0–100。默认 100 是**故意的**：漏写这个字段的后果应该是
    /// 「全量发布」而不是「一个人也收不到」——后者会安静地什么都不发生，
    /// 而运维会以为自己发过版了。
    #[serde(default = "full_rollout")]
    pub rollout: u8,
    /// 安全更新。它会**无视灰度**（见 [`decide`]）。
    #[serde(default)]
    pub critical: bool,
    /// 能直接升到本版本所需的最低当前版本。
    ///
    /// 用在「中间有一次不可跳过的数据迁移」的时候：比它还旧的客户端不该直接跳过来，
    /// 该去下一个完整包。没有这个字段就是「谁都能直接升」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<Version>,
    #[serde(default)]
    pub platforms: std::collections::BTreeMap<String, Build>,
}

fn full_rollout() -> u8 {
    100
}

/// 检查的结论。
///
/// 每一种「不更新」都有自己的名字，因为界面要说的话不一样：「已经是最新」和
/// 「还没轮到你」和「这个平台没有构建」在用户眼里是三件事，合成一个
/// `Option<Update>` 就只能说「没有更新」，然后收到三种不同的反馈。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Decision {
    /// 已经是这个通道上的最新版。
    UpToDate,
    /// 有更新。
    Available {
        version: String,
        notes: Option<String>,
        critical: bool,
    },
    /// 有新版本，但灰度还没轮到这台机器。界面**什么都不该说**——
    /// 「有更新但不给你」是最招人烦的一种提示。
    HeldBack { version: String },
    /// 手上这份比通道上的还新。从 beta 切回 stable 就是这个情形：
    /// 我们不降级（降级既是回滚攻击的入口，也会让新版本写下的数据被旧版本读到），
    /// 所以界面要照实说「你会停在这个版本，直到稳定版追上来」。
    AheadOfChannel { version: String },
    /// 中间隔着一次不可跳过的迁移，得去下完整包。
    NeedsFullDownload {
        version: String,
        min_version: String,
    },
    /// 清单里没有这个平台的构建。ARM 的 Windows、还没开始发的架构都会走到这里。
    NoBuild { target: String },
}

/// 该不该更新。
///
/// 顺序是有讲究的，每一条都排在它该在的位置：
///
/// 1. **没有构建**排最前面——后面所有判断对一个不存在的包都没有意义。
/// 2. **版本不比现在新**紧随其后。这一条同时挡住两件事：正常的「已是最新」，
///    以及**回放攻击**——签名只证明这个包是我们发的，不证明它是最新的，
///    所以拿一个旧的、已签名的、有已知漏洞的版本重放，只能在这里被挡下来。
/// 3. **`critical` 无视灰度。** 这是刻意选的失败方向：一个写错的 `rollout`
///    不应该有能力拦住安全更新。想给安全更新也做灰度，就先发 `critical: false`
///    的那一版，放完量再把标志翻上去。
pub fn decide(manifest: &Manifest, current: &Version, target: &str, bucket: u8) -> Decision {
    if !manifest.platforms.contains_key(target) {
        return Decision::NoBuild {
            target: target.to_owned(),
        };
    }

    if manifest.version == *current {
        return Decision::UpToDate;
    }
    if manifest.version < *current {
        return Decision::AheadOfChannel {
            version: manifest.version.to_string(),
        };
    }

    if let Some(minimum) = &manifest.min_version
        && current < minimum
    {
        return Decision::NeedsFullDownload {
            version: manifest.version.to_string(),
            min_version: minimum.to_string(),
        };
    }

    if !manifest.critical && bucket >= manifest.rollout {
        return Decision::HeldBack {
            version: manifest.version.to_string(),
        };
    }

    Decision::Available {
        version: manifest.version.to_string(),
        notes: manifest.notes.clone(),
        critical: manifest.critical,
    }
}

/// 这台机器在清单里对应的那个键。
///
/// 必须和 Tauri 更新器算出来的一模一样，否则会出现「我们说有更新，
/// 交给它下载时它说没有这个平台」。它用的是 `{os}-{arch}`，其中 macOS 叫
/// `darwin`，而架构名跟着 Rust 的 `target_arch` 走。
pub fn target() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// 取这个通道的清单并给出结论。
///
/// 失败要安静：端点挂了、DNS 被污染、清单是坏的 JSON——这些都不该打断任何事。
/// 调用方拿到 `Err` 之后应该**什么都不做**，等下一次检查（见设计文档 §5.7）。
pub async fn check(
    endpoint: &str,
    channel: Channel,
    current: &Version,
    bucket: u8,
) -> Result<Decision> {
    let url = channel.manifest_url(endpoint);
    let body = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .with_context(|| format!("fetch {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?
        .text()
        .await
        .with_context(|| format!("read {url}"))?;
    let manifest: Manifest =
        serde_json::from_str(&body).with_context(|| format!("parse the manifest at {url}"))?;
    Ok(decide(&manifest, current, &target(), bucket))
}

/// 按当前设置检查一次，顺便把第一次抽到的灰度分桶存下来。
///
/// `current` 由调用方给，**必须是 `PackageInfo::version`**——也就是自更新真正
/// 会拿去比大小的那个值。这里不能用 `CARGO_PKG_VERSION`：那是 `fern-core` 的
/// 版本号，被刻意钉在 `0.0.0`（见根 `Cargo.toml`），拿它去比会永远显示有更新。
///
/// 这个函数**不看** `automatic`。它同时服务于「设置里点一下检查」和定时检查，
/// 而前者是用户明确要求的——是定时那一边该在调用之前先问 `automatic`。
pub async fn check_now(paths: &crate::DataPaths, current: &str) -> Result<Decision> {
    let current = Version::parse(current)
        .with_context(|| format!("the running version {current:?} is not valid SemVer"))?;

    let mut settings = crate::data::settings::load(paths);
    let bucket = match settings.update.bucket {
        Some(bucket) => bucket,
        None => {
            let bucket = draw_bucket();
            settings.update.bucket = Some(bucket);
            // 存不下去也照常检查：一个存不了设置的环境里，用不上更新提示是更小的问题。
            // 代价是下次会重新抽一个桶，于是灰度对这台机器不稳定——可以接受。
            let _ = crate::data::settings::save(paths, &settings);
            bucket
        }
    };

    check(DEFAULT_ENDPOINT, settings.update.channel, &current, bucket).await
}

/// 抽一个灰度分桶，0–99。
///
/// 装的时候抽一次，此后不变（存在设置里）。它**从不上传**——灰度的判断完全在
/// 本地做，所以检查更新不会变成一条即使关掉遥测也仍在发送标识符的通道。
/// 100 个桶也没有区分个体的能力，这是它能安心留在设置文件里的原因。
pub fn draw_bucket() -> u8 {
    let mut byte = [0u8; 1];
    // 抽不到随机数就当自己在最后一个桶里：宁可晚点收到更新，也不要在一个
    // 连随机数都取不到的环境里假装自己被灰度选中了。
    if getrandom::fill(&mut byte).is_err() {
        return 99;
    }
    byte[0] % 100
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Manifest {
        Manifest {
            version: Version::parse(version).unwrap(),
            notes: None,
            pub_date: None,
            rollout: 100,
            critical: false,
            min_version: None,
            platforms: [(
                "windows-x86_64".to_owned(),
                Build {
                    url: "https://example.invalid/Fern.exe".to_owned(),
                    signature: "sig".to_owned(),
                },
            )]
            .into_iter()
            .collect(),
        }
    }

    fn at(version: &str) -> Version {
        Version::parse(version).unwrap()
    }

    #[test]
    fn a_newer_version_is_offered() {
        let decision = decide(&manifest("0.2.0"), &at("0.1.0"), "windows-x86_64", 0);
        assert!(matches!(decision, Decision::Available { .. }));
    }

    #[test]
    fn the_same_version_is_not_offered_again() {
        let decision = decide(&manifest("0.2.0"), &at("0.2.0"), "windows-x86_64", 0);
        assert_eq!(decision, Decision::UpToDate);
    }

    /// 签名只证明「这个包是我们发的」，不证明「这是最新的」。一个旧的、
    /// 签名完全正确的版本被重放回来时，唯一能挡住它的就是这个比较。
    #[test]
    fn an_older_version_is_never_installed() {
        let decision = decide(&manifest("0.1.0"), &at("0.2.0"), "windows-x86_64", 0);
        assert_eq!(
            decision,
            Decision::AheadOfChannel {
                version: "0.1.0".to_owned()
            }
        );
    }

    /// 通道之间不需要任何额外逻辑，全靠 SemVer 自己的排序。
    #[test]
    fn a_release_supersedes_its_own_prereleases() {
        let decision = decide(&manifest("0.2.0"), &at("0.2.0-beta.3"), "windows-x86_64", 0);
        assert!(matches!(decision, Decision::Available { .. }));
    }

    #[test]
    fn a_later_beta_supersedes_an_earlier_one() {
        let decision = decide(
            &manifest("0.2.0-beta.10"),
            &at("0.2.0-beta.9"),
            "windows-x86_64",
            0,
        );
        assert!(matches!(decision, Decision::Available { .. }));
    }

    /// 从 beta 切回 stable 的那一刻。用户手上的 0.2.0-beta.1 比稳定通道上的
    /// 0.1.9 新，所以什么都不发生——而界面得说清楚为什么。
    #[test]
    fn switching_from_beta_back_to_stable_does_not_downgrade() {
        let decision = decide(&manifest("0.1.9"), &at("0.2.0-beta.1"), "windows-x86_64", 0);
        assert!(matches!(decision, Decision::AheadOfChannel { .. }));
    }

    #[test]
    fn a_bucket_outside_the_rollout_waits() {
        let mut manifest = manifest("0.2.0");
        manifest.rollout = 30;
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "windows-x86_64", 29),
            Decision::Available { .. }
        ));
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "windows-x86_64", 30),
            Decision::HeldBack { .. }
        ));
    }

    /// 一个写错的 `rollout` 不该有能力拦住安全更新。
    #[test]
    fn a_critical_update_ignores_the_rollout() {
        let mut manifest = manifest("0.2.0");
        manifest.rollout = 0;
        manifest.critical = true;
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "windows-x86_64", 99),
            Decision::Available { critical: true, .. }
        ));
    }

    #[test]
    fn a_version_below_the_minimum_is_sent_to_the_download_page() {
        let mut manifest = manifest("0.3.0");
        manifest.min_version = Some(at("0.2.0"));
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "windows-x86_64", 0),
            Decision::NeedsFullDownload { .. }
        ));
        assert!(matches!(
            decide(&manifest, &at("0.2.0"), "windows-x86_64", 0),
            Decision::Available { .. }
        ));
    }

    #[test]
    fn a_platform_without_a_build_says_so() {
        let decision = decide(&manifest("0.2.0"), &at("0.1.0"), "linux-aarch64", 0);
        assert_eq!(
            decision,
            Decision::NoBuild {
                target: "linux-aarch64".to_owned()
            }
        );
    }

    /// 漏写 `rollout` 的后果必须是「全量发布」。反过来（默认 0）会安静地
    /// 什么都不发生，而运维会以为自己发过版了。
    #[test]
    fn a_manifest_without_a_rollout_reaches_everyone() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"version":"0.2.0","platforms":{"windows-x86_64":{"url":"u","signature":"s"}}}"#,
        )
        .unwrap();
        assert_eq!(manifest.rollout, 100);
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "windows-x86_64", 99),
            Decision::Available { .. }
        ));
    }

    /// 清单里会有 Tauri 更新器要用、而我们不认识的字段。多一个字段不能让
    /// 解析失败——否则给清单加一样东西就会让所有旧客户端停止检查更新。
    #[test]
    fn unknown_fields_in_the_manifest_are_ignored() {
        let manifest: serde_json::Result<Manifest> = serde_json::from_str(
            r#"{"version":"0.2.0","somethingNew":42,
                "platforms":{"windows-x86_64":{"url":"u","signature":"s"}}}"#,
        );
        assert!(manifest.is_ok());
    }

    /// 这一份是 `.github/build-manifest.py` 真正产出的形状，一个字段都不多不少。
    ///
    /// 它钉住的是一条跨语言的契约：清单由 Python 写、由 Rust 读，中间没有编译器。
    /// 改了脚本里的键名、或者改了这边的 `rename_all`，症状都是**所有客户端同时
    /// 停止收到更新**——而那时候能修它的办法，恰好是自更新。
    #[test]
    fn the_manifest_ci_writes_is_the_manifest_we_read() {
        let written = r#"{
          "version": "0.2.0",
          "rollout": 30,
          "critical": false,
          "platforms": {
            "windows-x86_64": {
              "url": "https://dl.fern.huanchengfly.top/release/0.2.0/Fern-windows-x86_64.exe",
              "signature": "untrusted comment: sig\nZmFrZQ=="
            },
            "darwin-aarch64": {
              "url": "https://dl.fern.huanchengfly.top/release/0.2.0/Fern-darwin-universal.app.tar.gz",
              "signature": "untrusted comment: sig\nZmFrZQ=="
            },
            "darwin-x86_64": {
              "url": "https://dl.fern.huanchengfly.top/release/0.2.0/Fern-darwin-universal.app.tar.gz",
              "signature": "untrusted comment: sig\nZmFrZQ=="
            },
            "linux-x86_64": {
              "url": "https://dl.fern.huanchengfly.top/release/0.2.0/Fern-linux-x86_64.AppImage",
              "signature": "untrusted comment: sig\nZmFrZQ=="
            }
          },
          "notes": "修了三个崩溃"
        }"#;

        let manifest: Manifest = serde_json::from_str(written).expect("CI 写的清单读不动了");
        assert_eq!(manifest.rollout, 30);
        assert_eq!(manifest.notes.as_deref(), Some("修了三个崩溃"));
        // 四个键都要在，而且要和 `target()` 算出来的对得上——macOS 的两个架构
        // 指向同一个 universal 包。
        assert_eq!(manifest.platforms.len(), 4);
        for key in [
            "windows-x86_64",
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
        ] {
            assert!(manifest.platforms.contains_key(key), "少了 {key}");
        }
        assert_eq!(
            manifest.platforms["darwin-aarch64"].url,
            manifest.platforms["darwin-x86_64"].url
        );
        // 放量 30 的清单对桶 30 就是「还没轮到」，这是 CI 那一侧唯一能出错而
        // 又完全没有报错的地方。
        assert!(matches!(
            decide(&manifest, &at("0.1.0"), "linux-x86_64", 30),
            Decision::HeldBack { .. }
        ));
    }

    #[test]
    fn a_channel_is_a_path_segment() {
        assert_eq!(
            Channel::Beta.manifest_url("https://dl.fern.huanchengfly.top"),
            "https://dl.fern.huanchengfly.top/beta/manifest.json"
        );
        // 末尾多一个斜杠是配置里最常见的手滑，不该变成一个双斜杠的 404。
        assert_eq!(
            Channel::Stable.manifest_url("https://dl.fern.huanchengfly.top/"),
            "https://dl.fern.huanchengfly.top/stable/manifest.json"
        );
    }

    #[test]
    fn a_bucket_is_always_in_range() {
        for _ in 0..64 {
            assert!(draw_bucket() < 100);
        }
    }
}
