//! 微软正版认证（文档 §3.1）。
//!
//! 五段令牌链，每段一个 HTTP 请求：
//!
//! ```text
//! MSA device code  →  XBL  →  XSTS  →  Minecraft  →  profile
//! ```
//!
//! 用 device code flow 而不是弹浏览器：桌面应用里嵌一个 webview 去接回调，
//! 意味着我们要处理重定向、cookie、以及「用户在那个窗口里输的密码由谁保管」
//! 这类问题。device code 把这些整段甩给系统浏览器——我们只显示八位码，
//! 密码从头到尾不经过 Fern。
//!
//! **在白名单批下来之前，第四段会稳定返回 403。** 那不是 bug，是微软要求
//! 应用先有调用记录才受理审批。这条链现在就该跑，跑出来的 403 正是申请的
//! 前提；批准之后同一段代码不用改。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Fern 在 Azure 上的应用标识。
///
/// 这不是密钥。公共客户端（device code flow）本来就没有 secret，微软的文档
/// 也明说 Client ID 可以公开——所以它就写在这里，而不是藏在环境变量里让每个
/// 想自己编译的人再去注册一个。
const CLIENT_ID: &str = "1fa1c0a1-1e4f-4c12-b66c-06fbfbd96abf";

/// 端点必须用 `consumers` 而不是 `common`。
///
/// 玩家清一色是个人微软账号；用 `common` 时服务端会返回一个没有任何提示性的
/// 错误，查起来能耗掉一整天。
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// `offline_access` 换的是 refresh token，没有它每次启动都要重新登录。
const SCOPE: &str = "XboxLive.signin offline_access";

/// 一次正版登录的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftSession {
    /// 长期有效，是「不用再登录」的凭据。进钥匙串。
    pub refresh_token: String,
    /// Minecraft 的访问令牌，24 小时过期。
    pub access_token: String,
    pub uuid: String,
    pub player_name: String,
    /// 访问令牌的过期时刻（Unix 秒）。
    pub expires_at: u64,
}

impl MicrosoftSession {
    /// 留五分钟余量：正好卡在过期边缘拿去启动，游戏那边照样会被拒。
    fn is_fresh(&self) -> bool {
        now_seconds() + 300 < self.expires_at
    }
}

/// 要展示给用户的那一段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeChallenge {
    /// 八位码，用户要抄到浏览器里。
    pub user_code: String,
    /// 让用户去这个地址输入。
    pub verification_uri: String,
    /// 还剩多少秒。
    pub expires_in: u64,
    /// 轮询间隔，服务端指定。
    #[serde(skip)]
    interval: u64,
    /// 轮询时带的凭据，不给界面看。
    #[serde(skip)]
    device_code: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

#[derive(Debug, Deserialize)]
struct XboxResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}

#[derive(Debug, Deserialize)]
struct Xui {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct XstsError {
    #[serde(rename = "XErr", default)]
    xerr: u64,
}

#[derive(Debug, Deserialize)]
struct MinecraftToken {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("valid microsoft auth client")
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// 第一步：要一个设备码。
///
/// 返回之后界面就该把 `user_code` 显示出来，然后调 [`finish_login`] 去轮询。
pub async fn begin_login() -> Result<DeviceCodeChallenge> {
    let response = client()
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
        .send()
        .await
        .context("向微软申请设备码")?;
    let status = response.status();
    let bytes = response.bytes().await.context("读取设备码响应")?;
    if !status.is_success() {
        return Err(oauth_error(&bytes, status));
    }
    let device: DeviceCodeResponse = serde_json::from_slice(&bytes).context("解析设备码响应")?;
    Ok(DeviceCodeChallenge {
        user_code: device.user_code,
        verification_uri: device.verification_uri,
        expires_in: device.expires_in,
        interval: device.interval.max(1),
        device_code: device.device_code,
    })
}

/// 第二步到第五步：等用户在浏览器里点完，然后把整条链走到底。
pub async fn finish_login(challenge: &DeviceCodeChallenge) -> Result<MicrosoftSession> {
    let msa = poll_for_token(challenge).await?;
    complete_chain(msa.access_token, msa.refresh_token).await
}

/// 静默刷新。整条链重走一遍——Xbox 和 Minecraft 的令牌都不能单独续。
pub async fn ensure_fresh(session: &MicrosoftSession) -> Result<MicrosoftSession> {
    if session.is_fresh() {
        return Ok(session.clone());
    }
    let response = client()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("scope", SCOPE),
            ("grant_type", "refresh_token"),
            ("refresh_token", session.refresh_token.as_str()),
        ])
        .send()
        .await
        .context("刷新微软令牌")?;
    let status = response.status();
    let bytes = response.bytes().await.context("读取刷新响应")?;
    if !status.is_success() {
        return Err(oauth_error(&bytes, status).context("刷新失败，需要重新登录"));
    }
    let token: TokenResponse = serde_json::from_slice(&bytes).context("解析刷新响应")?;
    // 微软会轮换 refresh token；返回空则沿用旧的。
    let refresh = if token.refresh_token.is_empty() {
        session.refresh_token.clone()
    } else {
        token.refresh_token
    };
    complete_chain(token.access_token, refresh).await
}

async fn poll_for_token(challenge: &DeviceCodeChallenge) -> Result<TokenResponse> {
    let deadline = now_seconds() + challenge.expires_in;
    let mut interval = challenge.interval;
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if now_seconds() >= deadline {
            return Err(anyhow!("这个登录码已经过期了，重新开始一次"));
        }

        let response = client()
            .post(TOKEN_URL)
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", challenge.device_code.as_str()),
            ])
            .send()
            .await
            .context("轮询微软令牌")?;
        let status = response.status();
        let bytes = response.bytes().await.context("读取令牌响应")?;
        if status.is_success() {
            return serde_json::from_slice(&bytes).context("解析令牌响应");
        }

        let error: OAuthError = serde_json::from_slice(&bytes).unwrap_or(OAuthError {
            error: String::new(),
            error_description: String::new(),
        });
        match error.error.as_str() {
            // 用户还没点完，这是正常状态而不是错误。
            "authorization_pending" => continue,
            // 服务端嫌我们问得太勤，照做。
            "slow_down" => interval += 5,
            "expired_token" => return Err(anyhow!("这个登录码已经过期了，重新开始一次")),
            "authorization_declined" => return Err(anyhow!("你在浏览器里拒绝了这次登录")),
            _ => return Err(oauth_error(&bytes, status)),
        }
    }
}

/// XBL → XSTS → Minecraft → profile。
async fn complete_chain(msa_token: String, refresh_token: String) -> Result<MicrosoftSession> {
    let (xbl_token, _) = xbox_live(&msa_token).await?;
    let (xsts_token, user_hash) = xsts(&xbl_token).await?;
    let minecraft = minecraft_login(&xsts_token, &user_hash).await?;
    let profile = minecraft_profile(&minecraft.access_token).await?;

    Ok(MicrosoftSession {
        refresh_token,
        access_token: minecraft.access_token,
        uuid: profile.id,
        player_name: profile.name,
        // 微软给的是 86400；万一没给，按一天算。
        expires_at: now_seconds()
            + if minecraft.expires_in > 0 {
                minecraft.expires_in
            } else {
                86400
            },
    })
}

async fn xbox_live(msa_token: &str) -> Result<(String, String)> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            // 前缀 `d=` 是必须的，少了它服务端只会说请求无效。
            "RpsTicket": format!("d={msa_token}"),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    });
    let response = client()
        .post(XBL_URL)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .context("向 Xbox Live 认证")?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Xbox Live 拒绝了这次认证（HTTP {}）",
            response.status()
        ));
    }
    let xbox: XboxResponse = response.json().await.context("解析 Xbox Live 响应")?;
    let hash = xbox
        .display_claims
        .xui
        .first()
        .map(|xui| xui.uhs.clone())
        .ok_or_else(|| anyhow!("Xbox Live 没有返回用户标识"))?;
    Ok((xbox.token, hash))
}

async fn xsts(xbl_token: &str) -> Result<(String, String)> {
    let body = serde_json::json!({
        "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT",
    });
    let response = client()
        .post(XSTS_URL)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .context("向 XSTS 换票")?;
    let status = response.status();
    let bytes = response.bytes().await.context("读取 XSTS 响应")?;
    if !status.is_success() {
        let detail: XstsError = serde_json::from_slice(&bytes).unwrap_or(XstsError { xerr: 0 });
        return Err(anyhow!("{}", xsts_message(detail.xerr, status)));
    }
    let xbox: XboxResponse = serde_json::from_slice(&bytes).context("解析 XSTS 响应")?;
    let hash = xbox
        .display_claims
        .xui
        .first()
        .map(|xui| xui.uhs.clone())
        .ok_or_else(|| anyhow!("XSTS 没有返回用户标识"))?;
    Ok((xbox.token, hash))
}

/// XSTS 的错误码要翻成人话。
///
/// 这两条是玩家最常撞上的，而原始响应里只有一串数字——照原样抛出去，用户
/// 完全不知道该干什么。
fn xsts_message(xerr: u64, status: reqwest::StatusCode) -> String {
    match xerr {
        2148916233 => {
            "这个微软账号还没有 Xbox 账号。去 xbox.com 登录一次创建好，再回来登录。".to_owned()
        }
        2148916238 => {
            "这是一个未成年账户，需要先由家庭组里的成年人把它加进去，才能单独登录。".to_owned()
        }
        2148916235 => "微软账号所在的国家/地区暂不支持 Xbox Live。".to_owned(),
        2148916236 | 2148916237 => "这个账号需要先完成成人验证。".to_owned(),
        0 => format!("XSTS 拒绝了这次认证（HTTP {status}）"),
        other => format!("XSTS 拒绝了这次认证（错误码 {other}）"),
    }
}

async fn minecraft_login(xsts_token: &str, user_hash: &str) -> Result<MinecraftToken> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={user_hash};{xsts_token}"),
    });
    let response = client()
        .post(MC_LOGIN_URL)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .context("向 Minecraft 换令牌")?;
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        // 白名单没批下来时就停在这里。说清楚是审批问题，不是账号问题——
        // 否则用户会去反复检查自己的密码。
        return Err(anyhow!(
            "微软还没有把 Fern 加进第三方启动器白名单，正版登录暂时用不了。\
             这一步的失败记录正是申请审批的前提，申请已在流程中。"
        ));
    }
    if !status.is_success() {
        return Err(anyhow!("Minecraft 拒绝了这次认证（HTTP {status}）"));
    }
    response.json().await.context("解析 Minecraft 令牌")
}

async fn minecraft_profile(access_token: &str) -> Result<MinecraftProfile> {
    let response = client()
        .get(MC_PROFILE_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .context("读取 Minecraft 档案")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // Game Pass 用户没在官方启动器里初始化过时会走到这里。
        return Err(anyhow!(
            "这个账号还没有 Minecraft 档案。如果是通过 Game Pass 拿到的游戏，\
             先在官方启动器里进一次游戏、设置好名字，再回来登录。"
        ));
    }
    if !response.status().is_success() {
        return Err(anyhow!("读取档案失败（HTTP {}）", response.status()));
    }
    response.json().await.context("解析 Minecraft 档案")
}

fn oauth_error(bytes: &[u8], status: reqwest::StatusCode) -> anyhow::Error {
    let detail: OAuthError = serde_json::from_slice(bytes).unwrap_or(OAuthError {
        error: String::new(),
        error_description: String::new(),
    });
    if !detail.error_description.is_empty() {
        // 微软的描述里带一串换行和关联 ID，第一行才是给人看的。
        let first = detail
            .error_description
            .lines()
            .next()
            .unwrap_or(&detail.error_description);
        anyhow!("{first}")
    } else if !detail.error.is_empty() {
        anyhow!("{}", detail.error)
    } else {
        anyhow!("HTTP {status}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_id_is_a_real_guid() {
        // 打错一位的话，第一步就会失败，而报错和「打错了」毫无关系。
        assert_eq!(CLIENT_ID.len(), 36);
        assert_eq!(CLIENT_ID.matches('-').count(), 4);
        assert!(CLIENT_ID.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn endpoints_use_the_consumers_tenant() {
        // 用 common 会返回一个没有提示性的错误，这条最难自己查出来。
        assert!(DEVICE_CODE_URL.contains("/consumers/"));
        assert!(TOKEN_URL.contains("/consumers/"));
    }

    #[test]
    fn the_scope_asks_for_a_refresh_token() {
        // 少了 offline_access，每次启动都要重新扫码。
        assert!(SCOPE.contains("offline_access"));
        assert!(SCOPE.contains("XboxLive.signin"));
    }

    #[test]
    fn xsts_error_codes_become_something_the_user_can_act_on() {
        assert!(xsts_message(2148916233, reqwest::StatusCode::UNAUTHORIZED).contains("xbox.com"));
        assert!(xsts_message(2148916238, reqwest::StatusCode::UNAUTHORIZED).contains("家庭组"));
        // 不认识的码也不能只抛一个裸数字。
        let unknown = xsts_message(42, reqwest::StatusCode::UNAUTHORIZED);
        assert!(unknown.contains("42") && unknown.contains("XSTS"));
    }

    #[test]
    fn a_challenge_never_serialises_its_device_code() {
        // user_code 是给人念的，device_code 是凭据——只有前者能进 webview。
        let challenge = DeviceCodeChallenge {
            user_code: "ABCD-EFGH".to_owned(),
            verification_uri: "https://microsoft.com/link".to_owned(),
            expires_in: 900,
            interval: 5,
            device_code: "super-secret-device-code".to_owned(),
        };
        let json = serde_json::to_string(&challenge).expect("serialize");
        assert!(json.contains("ABCD-EFGH"));
        assert!(!json.contains("super-secret-device-code"));
    }

    #[test]
    fn a_token_is_stale_before_it_actually_expires() {
        let mut session = MicrosoftSession {
            refresh_token: "r".to_owned(),
            access_token: "a".to_owned(),
            uuid: "u".to_owned(),
            player_name: "Steve".to_owned(),
            expires_at: now_seconds() + 3600,
        };
        assert!(session.is_fresh());
        // 还剩一分钟就该去刷新了：正好卡在边缘拿去启动，游戏那边照样会拒。
        session.expires_at = now_seconds() + 60;
        assert!(!session.is_fresh());
    }
}
