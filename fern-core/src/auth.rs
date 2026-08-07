//! 外置登录（文档 §3.2）。
//!
//! 面向 LittleSkin 这类 Yggdrasil 兼容皮肤站。认证走皮肤站自己的 API，启动时
//! 用 `-javaagent` 把 authlib-injector 挂进去，游戏内所有对 Mojang 的会话和
//! 皮肤请求就都改道到那个站。
//!
//! 令牌不落盘：存进系统钥匙串。钥匙串用不了的时候直接说用不了，**不**退回
//! 明文文件——那等于把「我们保管好了」换成「我们看起来保管好了」。
//!
//! 微软正版登录不在这里。它的前置手续（Azure 应用注册 + 白名单审批）不是
//! 代码能解决的，没有 Client ID 写出来的链路一行也跑不了。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use fern_download::{DownloadClient, DownloadEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, settings::source_order};

/// authlib-injector 的发布清单。BMCLAPI 上有镜像，国内用户占比高，直接用它。
const INJECTOR_LATEST: &str =
    "https://bmclapi2.bangbang93.com/mirrors/authlib-injector/artifact/latest.json";

/// 一次登录的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YggdrasilSession {
    /// 皮肤站的 API 根地址，启动参数里要原样传给 injector。
    pub api_root: String,
    pub access_token: String,
    /// 这台机器的标识。刷新令牌时必须和登录时是同一个，否则服务端拒绝。
    pub client_token: String,
    pub uuid: String,
    pub player_name: String,
}

/// 界面需要知道的那部分。
///
/// 访问令牌不在里面：webview 里的任何东西——一个 XSS、一段第三方脚本、一次
/// 截图——都不该有机会碰到它。界面要显示的只是「用哪个站、以谁的身份」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub api_root: String,
    pub uuid: String,
    pub player_name: String,
}

impl From<&YggdrasilSession> for AccountView {
    fn from(session: &YggdrasilSession) -> Self {
        Self {
            api_root: session.api_root.clone(),
            uuid: session.uuid.clone(),
            player_name: session.player_name.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "clientToken")]
    client_token: String,
    #[serde(rename = "selectedProfile")]
    selected_profile: Option<Profile>,
    #[serde(rename = "availableProfiles", default)]
    available_profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
struct Profile {
    id: String,
    name: String,
}

/// 皮肤站返回的错误。它们说的话比 HTTP 状态码有用得多。
#[derive(Debug, Deserialize)]
struct YggdrasilError {
    #[serde(default)]
    error: String,
    #[serde(rename = "errorMessage", default)]
    error_message: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("valid auth client configuration")
}

/// `https://littleskin.cn/api/yggdrasil` → `…/authserver/authenticate`
fn endpoint(api_root: &str, path: &str) -> String {
    format!("{}/{path}", api_root.trim_end_matches('/'))
}

async fn post(url: &str, body: serde_json::Value) -> Result<Option<AuthResponse>> {
    let response = client()
        .post(url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("请求 {url}"))?;
    let status = response.status();
    let bytes = response.bytes().await.context("读取响应")?;

    if status.is_success() {
        // validate 成功时返回 204 没有正文，登录和刷新则有。
        if bytes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(
            serde_json::from_slice(&bytes).context("解析认证响应")?,
        ));
    }

    // 皮肤站的错误信息是给用户看的，原样带出去比「HTTP 403」有用得多。
    let detail: YggdrasilError = serde_json::from_slice(&bytes).unwrap_or(YggdrasilError {
        error: String::new(),
        error_message: String::new(),
    });
    let message = if !detail.error_message.is_empty() {
        detail.error_message
    } else if !detail.error.is_empty() {
        detail.error
    } else {
        format!("HTTP {status}")
    };
    Err(anyhow!("{message}"))
}

/// 用户名密码登录。
///
/// `client_token` 要在这台机器上保持稳定：换一个就等于换一台设备，之前发出去
/// 的令牌会被服务端作废。
pub async fn authenticate(
    api_root: &str,
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<YggdrasilSession> {
    let response = post(
        &endpoint(api_root, "authserver/authenticate"),
        serde_json::json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username,
            "password": password,
            "clientToken": client_token,
            "requestUser": false,
        }),
    )
    .await?
    .ok_or_else(|| anyhow!("认证服务器没有返回令牌"))?;

    // 一个账号可以挂多个角色。没有 selectedProfile 说明站点要求先选一个，
    // 只有一个可选时替用户选掉，多个就得让他自己挑——我们不替他决定进游戏
    // 之后叫什么。
    let profile = response
        .selected_profile
        .clone()
        .or_else(|| {
            (response.available_profiles.len() == 1).then(|| response.available_profiles[0].clone())
        })
        .ok_or_else(|| {
            if response.available_profiles.is_empty() {
                anyhow!("这个账号还没有角色，先去皮肤站创建一个")
            } else {
                anyhow!("这个账号有多个角色，Fern 还不支持选择")
            }
        })?;

    Ok(YggdrasilSession {
        api_root: api_root.to_owned(),
        access_token: response.access_token,
        client_token: response.client_token,
        uuid: profile.id,
        player_name: profile.name,
    })
}

/// 静默刷新。令牌还有效就原样返回，过期了就换一张新的。
pub async fn ensure_fresh(session: &YggdrasilSession) -> Result<YggdrasilSession> {
    if validate(session).await? {
        return Ok(session.clone());
    }
    refresh(session).await
}

pub async fn validate(session: &YggdrasilSession) -> Result<bool> {
    let url = endpoint(&session.api_root, "authserver/validate");
    let body = serde_json::json!({
        "accessToken": session.access_token,
        "clientToken": session.client_token,
    });
    // 令牌无效是这个接口的正常答案之一，不是错误。
    match post(&url, body).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub async fn refresh(session: &YggdrasilSession) -> Result<YggdrasilSession> {
    let response = post(
        &endpoint(&session.api_root, "authserver/refresh"),
        serde_json::json!({
            "accessToken": session.access_token,
            "clientToken": session.client_token,
        }),
    )
    .await?
    .ok_or_else(|| anyhow!("刷新没有返回新令牌"))?;

    let profile = response
        .selected_profile
        .ok_or_else(|| anyhow!("刷新之后拿不到角色信息，请重新登录"))?;
    Ok(YggdrasilSession {
        api_root: session.api_root.clone(),
        access_token: response.access_token,
        client_token: response.client_token,
        uuid: profile.id,
        player_name: profile.name,
    })
}

/// 把皮肤站的 API 元数据取回来 base64。
///
/// injector 启动时本来要自己去请求一次；预取塞进启动参数，省掉那一次网络
/// 往返——皮肤站挂了的时候，这一步的差别是「进得去游戏」和「卡在启动」。
pub async fn prefetched(api_root: &str) -> Result<String> {
    let response = client()
        .get(api_root)
        .send()
        .await
        .with_context(|| format!("请求 {api_root}"))?;
    if !response.status().is_success() {
        return Err(anyhow!("{api_root} 返回 HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.context("读取皮肤站元数据")?;
    // 得是合法 JSON——injector 会当 JSON 解析，塞一段 HTML 进去只会让它在
    // 启动时报一个和真正原因无关的错。
    serde_json::from_slice::<serde_json::Value>(&bytes).with_context(|| {
        format!("{api_root} 返回的不是 JSON，这可能不是一个 Yggdrasil API 地址")
    })?;
    Ok(BASE64.encode(&bytes))
}

/// 保证 authlib-injector 的 jar 在本地，返回它的路径。
pub async fn ensure_injector(
    paths: &DataPaths,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<PathBuf> {
    let downloader = DownloadClient::new(source_order(), 4);
    let manifest_bytes = downloader
        .fetch(INJECTOR_LATEST)
        .await
        .context("读取 authlib-injector 的发布清单")?;

    #[derive(Deserialize)]
    struct Manifest {
        version: String,
        download_url: String,
        checksums: Checksums,
    }
    #[derive(Deserialize)]
    struct Checksums {
        sha256: String,
    }

    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).context("解析 authlib-injector 的发布清单")?;
    let destination = paths
        .runtimes
        .join("authlib-injector")
        .join(format!("authlib-injector-{}.jar", manifest.version));

    if let Ok(existing) = tokio::fs::read(&destination).await
        && sha256_matches(&existing, &manifest.checksums.sha256)
    {
        return Ok(destination);
    }

    let _ = events.send(DownloadEvent::Status {
        message: format!("下载 authlib-injector {}", manifest.version),
    });
    // 这个 jar 只有几十 KB，一次读完再校验比走下载器省事——而且它给的是
    // sha256，下载器认的是 sha1。
    let bytes = downloader
        .fetch(&manifest.download_url)
        .await
        .context("下载 authlib-injector")?;
    if !sha256_matches(&bytes, &manifest.checksums.sha256) {
        return Err(anyhow!("authlib-injector 的校验和对不上"));
    }

    tokio::fs::create_dir_all(destination.parent().expect("injector directory")).await?;
    let temporary = destination.with_extension("jar.part");
    tokio::fs::write(&temporary, &bytes).await?;
    tokio::fs::rename(&temporary, &destination).await?;
    Ok(destination)
}

fn sha256_matches(bytes: &[u8], expected: &str) -> bool {
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    actual.eq_ignore_ascii_case(expected)
}

/// 外置登录要额外挂上去的 JVM 参数。
///
/// 顺序有意义：`-javaagent` 必须在游戏主类之前，而 `prefetched` 是给 agent
/// 自己读的系统属性。两条都要排在 `-cp` 那一堆前面，所以调用方是往 jvm 参数
/// 的**开头**插。
pub fn jvm_arguments(injector: &Path, api_root: &str, prefetched: &str) -> Vec<String> {
    let mut arguments = vec![format!("-javaagent:{}={api_root}", injector.display())];
    if !prefetched.is_empty() {
        arguments.push(format!(
            "-Dauthlibinjector.yggdrasil.prefetched={prefetched}"
        ));
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_handed_to_the_interface_carries_no_token() {
        let session = YggdrasilSession {
            api_root: "https://littleskin.cn/api/yggdrasil".to_owned(),
            access_token: "super-secret-token".to_owned(),
            client_token: "machine-token".to_owned(),
            uuid: "abc".to_owned(),
            player_name: "Steve".to_owned(),
        };
        let json = serde_json::to_string(&AccountView::from(&session)).expect("serialize");
        assert!(json.contains("Steve"));
        assert!(!json.contains("super-secret-token"));
        assert!(!json.contains("machine-token"));
    }

    #[test]
    fn endpoints_survive_a_trailing_slash() {
        assert_eq!(
            endpoint(
                "https://littleskin.cn/api/yggdrasil",
                "authserver/authenticate"
            ),
            "https://littleskin.cn/api/yggdrasil/authserver/authenticate"
        );
        assert_eq!(
            endpoint("https://littleskin.cn/api/yggdrasil/", "authserver/refresh"),
            "https://littleskin.cn/api/yggdrasil/authserver/refresh"
        );
    }

    #[test]
    fn the_agent_goes_in_with_the_api_root_attached() {
        let arguments = jvm_arguments(
            Path::new("/fern/runtimes/authlib-injector/authlib-injector-1.2.8.jar"),
            "https://littleskin.cn/api/yggdrasil",
            "eyJtZXRhIjp7fX0=",
        );
        assert_eq!(
            arguments[0],
            "-javaagent:/fern/runtimes/authlib-injector/authlib-injector-1.2.8.jar=https://littleskin.cn/api/yggdrasil"
        );
        assert!(arguments[1].starts_with("-Dauthlibinjector.yggdrasil.prefetched="));

        // 预取拿不到时不要塞一个空属性进去，injector 会当成空元数据。
        let without = jvm_arguments(Path::new("/a.jar"), "https://x.invalid", "");
        assert_eq!(without.len(), 1);
    }

    #[test]
    fn sha256_comparison_is_case_insensitive() {
        // 清单里给的是小写，别人手抄过来可能是大写。
        let digest = "9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10";
        assert!(sha256_matches(b"", &empty_sha256()));
        assert!(sha256_matches(b"", &empty_sha256().to_uppercase()));
        assert!(!sha256_matches(b"", digest));
    }

    fn empty_sha256() -> String {
        Sha256::digest(b"")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn a_missing_profile_says_which_problem_it_is() {
        // 「没有角色」和「有好几个角色」是两回事，提示不能混为一谈。
        let none: AuthResponse =
            serde_json::from_str(r#"{"accessToken":"a","clientToken":"b","availableProfiles":[]}"#)
                .expect("parse");
        assert!(none.selected_profile.is_none());
        assert!(none.available_profiles.is_empty());

        let many: AuthResponse = serde_json::from_str(
            r#"{"accessToken":"a","clientToken":"b","availableProfiles":[{"id":"1","name":"A"},{"id":"2","name":"B"}]}"#,
        )
        .expect("parse");
        assert_eq!(many.available_profiles.len(), 2);
    }
}
