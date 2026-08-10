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

use std::sync::{Arc, OnceLock};
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
    /// 同一个地址，但把码也带上了。有的话就用它开浏览器——省掉抄写那一步。
    /// 微软目前不给，但这是 RFC 8628 里的标准字段，给了就该用上。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
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
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// 轮询间隔的上限。
///
/// 服务端说 `slow_down` 就往上加，但不能一路加到用户以为登录卡死了：他此刻
/// 正盯着启动器等它反应过来。
const MAX_INTERVAL: u64 = 15;

/// 催一下那条正在轮询的登录。
///
/// device code flow 里有两个对不上的时钟：用户在浏览器里按下「确认」的那一刻，
/// 我们这边可能才刚睡下，最坏要等满一个轮询间隔才发现。界面上那颗「我已完成
/// 登录」按下时 poke 一下，就把这一觉打断，立刻再问一次。
///
/// 没有人在等的时候 poke 不会丢——`notify_one` 会把这一次记下来，下一次等待
/// 立即返回。所以「按得比轮询快」也不会白按。
#[derive(Clone, Default)]
pub struct Nudge(Arc<tokio::sync::Notify>);

impl Nudge {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用户说他已经登完了。
    pub fn poke(&self) {
        self.0.notify_one();
    }

    /// 睡这么久，或者睡到被 poke 醒——哪个先来算哪个。
    async fn rest(&self, delay: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = self.0.notified() => {}
        }
    }
}

/// 一次轮询问出来的结果。**只有真的没救了才是 `Err`。**
///
/// 这个区分是这一段的全部意义：轮询要跑十几分钟，其间网络抖一下、代理断一次、
/// 服务端 502 一回都太正常了，而那时候用户正在浏览器里输码——把这些当成登录
/// 失败，等于让一次丢包作废整场登录。
enum Progress {
    Done(TokenResponse),
    /// 用户还没点完。这是这条流程的常态，不是错误。
    Pending,
    /// 服务端嫌我们问得太勤。
    SlowDown,
    /// 这一次没问成，下一次再说。
    Hiccup(String),
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

/// 认证用的那个 HTTP 客户端。
///
/// 只建一次。轮询一次登录要发上百个请求，每次现建一个客户端就是每次重来一遍
/// TLS 握手和连接池——那正是「点完了还要等好一会儿」里看不见的那部分。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("valid microsoft auth client")
    })
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
        verification_uri_complete: device
            .verification_uri_complete
            .filter(|uri| !uri.is_empty()),
        expires_in: device.expires_in,
        interval: device.interval.clamp(1, MAX_INTERVAL),
        device_code: device.device_code,
    })
}

/// 第二步到第五步：等用户在浏览器里点完，然后把整条链走到底。
pub async fn finish_login(
    challenge: &DeviceCodeChallenge,
    nudge: &Nudge,
) -> Result<MicrosoftSession> {
    let msa = poll_for_token(challenge, nudge).await?;
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

async fn poll_for_token(challenge: &DeviceCodeChallenge, nudge: &Nudge) -> Result<TokenResponse> {
    let deadline = now_seconds() + challenge.expires_in;
    let mut interval = challenge.interval;
    // 上一次问的时候出了什么事。问通了就清掉——超时那句话要说的是最后发生了
    // 什么，而不是十分钟里曾经断过一次网。
    let mut hiccup: Option<String> = None;
    loop {
        let left = deadline.saturating_sub(now_seconds());
        if left == 0 {
            return Err(match hiccup {
                Some(reason) => anyhow!("登录未能完成：{reason}"),
                None => anyhow!("登录码已过期，请重新发起登录"),
            });
        }
        nudge.rest(Duration::from_secs(interval.min(left))).await;

        match ask_once(challenge).await? {
            Progress::Done(token) => return Ok(token),
            Progress::Pending => hiccup = None,
            Progress::SlowDown => {
                hiccup = None;
                interval = (interval + 5).min(MAX_INTERVAL);
            }
            Progress::Hiccup(reason) => hiccup = Some(reason),
        }
    }
}

/// 问一次「用户点完了吗」。
async fn ask_once(challenge: &DeviceCodeChallenge) -> Result<Progress> {
    let sent = client()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", challenge.device_code.as_str()),
        ])
        .send()
        .await;
    let response = match sent {
        Ok(response) => response,
        Err(error) => return Ok(Progress::Hiccup(format!("{error}"))),
    };
    let status = response.status();
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Progress::Hiccup(format!("{error}"))),
    };
    classify(&bytes, status)
}

/// 这一次的响应意味着什么。
///
/// 单独拎出来是为了能测：这段判断决定了「一次网络抖动」和「登录失败」的分界，
/// 而它错的时候没有任何编译期或运行期的迹象——只有用户在半路上被踢出来。
fn classify(bytes: &[u8], status: reqwest::StatusCode) -> Result<Progress> {
    if status.is_success() {
        // 读不懂一份 200 也再试一次：登录码还有效，而放弃的代价是整场重来。
        return Ok(match serde_json::from_slice(bytes) {
            Ok(token) => Progress::Done(token),
            Err(error) => Progress::Hiccup(format!("{error}")),
        });
    }

    let error: OAuthError = serde_json::from_slice(bytes).unwrap_or(OAuthError {
        error: String::new(),
        error_description: String::new(),
    });
    match error.error.as_str() {
        "authorization_pending" => Ok(Progress::Pending),
        "slow_down" => Ok(Progress::SlowDown),
        "expired_token" => Err(anyhow!("登录码已过期，请重新发起登录")),
        "authorization_declined" => Err(anyhow!("登录请求已在浏览器中被拒绝")),
        // 认不出来的那些分两种：服务端自己出了问题，等一等就好；请求本身不对，
        // 再问一万次也是同一个答案。分不出来时按前者算——用户还站在浏览器前面，
        // 而下一次轮询的代价只有几秒。
        _ if status.is_server_error() || error.error.is_empty() => {
            Ok(Progress::Hiccup(format!("{}", oauth_error(bytes, status))))
        }
        _ => Err(oauth_error(bytes, status)),
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
            "该微软账户尚未创建 Xbox 账户。请先在 xbox.com 登录一次以完成创建。".to_owned()
        }
        2148916238 => "未成年账户需先由家庭组中的成年成员添加后方可登录。".to_owned(),
        2148916235 => "该账户所在国家或地区暂不支持 Xbox Live。".to_owned(),
        2148916236 | 2148916237 => "该账户需先完成成人验证。".to_owned(),
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
            "Fern 尚未通过微软的第三方启动器审批，正版登录暂不可用。\
             本次失败记录是提交审批的前提条件。"
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
            "该账户尚无 Minecraft 档案。若游戏通过 Game Pass 获得，\
             请先在官方启动器中进入游戏并设置名称。"
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
            verification_uri_complete: None,
            expires_in: 900,
            interval: 5,
            device_code: "super-secret-device-code".to_owned(),
        };
        let json = serde_json::to_string(&challenge).expect("serialize");
        assert!(json.contains("ABCD-EFGH"));
        assert!(!json.contains("super-secret-device-code"));
    }

    /// 一次丢包不该作废整场登录。
    ///
    /// 轮询要跑十几分钟，其间用户正在浏览器里输码——那段时间里网络抖一下、
    /// 代理断一次、服务端 502 一回都太正常了。这条钉住的是那个分界：只有
    /// 「服务端明确说不」才是 `Err`，其余一律再问一次。
    #[test]
    fn a_network_hiccup_is_not_a_failed_login() {
        let transient = |body: &str, status: reqwest::StatusCode| {
            matches!(
                classify(body.as_bytes(), status),
                Ok(Progress::Hiccup(_)) | Ok(Progress::Pending) | Ok(Progress::SlowDown)
            )
        };
        assert!(transient(
            r#"{"error":"authorization_pending"}"#,
            reqwest::StatusCode::BAD_REQUEST
        ));
        assert!(transient(
            r#"{"error":"slow_down"}"#,
            reqwest::StatusCode::BAD_REQUEST
        ));
        // 服务端自己出问题，等一等就好。
        assert!(transient(
            "<html>bad gateway</html>",
            reqwest::StatusCode::BAD_GATEWAY
        ));
        // 代理塞回来一页 HTML，连 error 字段都没有——同样不能判死刑。
        assert!(transient(
            "<html>captive portal</html>",
            reqwest::StatusCode::FORBIDDEN
        ));

        // 而这两条是真的没救了，再问一万次也是同一个答案。
        for body in [
            r#"{"error":"expired_token"}"#,
            r#"{"error":"authorization_declined"}"#,
            r#"{"error":"invalid_client","error_description":"应用标识不对"}"#,
        ] {
            assert!(classify(body.as_bytes(), reqwest::StatusCode::BAD_REQUEST).is_err());
        }
    }

    #[test]
    fn the_polling_interval_stays_within_reach() {
        // 服务端说慢一点就慢一点，但不能慢到用户以为登录卡死了。
        let mut interval = 5;
        for _ in 0..10 {
            interval = (interval + 5).min(MAX_INTERVAL);
        }
        assert_eq!(interval, MAX_INTERVAL);
        // 服务端给的初始间隔也照这个上限收。
        assert_eq!(60_u64.clamp(1, MAX_INTERVAL), MAX_INTERVAL);
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
