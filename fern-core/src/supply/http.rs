//! 两家共用的请求方式：重试、拒绝、报错的说法。
//!
//! 差别只在两处——叫什么名字（报错里要说清是哪一家拒绝了）和要不要带密钥。
//! 其余的都一样：装一个带依赖的模组要发 N+1 次请求，任何一次抖动都会让整个
//! 安装失败，所以 GET 要重试；而服务端明确的拒绝重试也没用。

use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

/// 一家上游。
pub(super) struct Upstream {
    /// 报错时说的名字。
    pub name: &'static str,
    /// CurseForge 要求每个请求都带 `x-api-key`；Modrinth 不用。
    pub key: Option<&'static str>,
}

/// 每个请求试几次。TLS 握手在不稳的网络上说断就断。
const ATTEMPTS: u32 = 3;

enum Refusal {
    /// 再试也是这个结果。
    Final(anyhow::Error),
    /// 网络层面的抖动，值得再来一次。
    Retryable(anyhow::Error),
}

/// 一个进程一个客户端：连接池和 TLS 会话在请求之间复用。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // Modrinth 按 User-Agent 做速率限制，匿名调用会被降级甚至拒绝。
            .user_agent(super::modrinth::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("valid supply client")
    })
}

impl Upstream {
    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let request = client()
            .request(method, url)
            .header("accept", "application/json");
        match self.key {
            Some(key) => request.header("x-api-key", key),
            None => request,
        }
    }

    pub async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let mut last_error = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(300u64 << attempt)).await;
            }
            match self.attempt_get(url).await {
                Ok(bytes) => {
                    return serde_json::from_slice(&bytes)
                        .with_context(|| format!("解析 {} 的响应", self.name));
                }
                // 服务端明确的拒绝重试没有意义，直接抛出去。
                Err(Refusal::Final(error)) => return Err(error),
                Err(Refusal::Retryable(error)) => last_error = Some(error),
            }
        }
        Err(last_error
            .unwrap_or_else(|| anyhow!("请求 {url} 失败"))
            .context(format!("请求 {url}")))
    }

    /// 带 JSON 请求体的那一种：按 hash 批量查、按 id 批量取。
    ///
    /// 不做重试：POST 在语义上不保证幂等，而这些接口失败了就退回「不知道」，
    /// 比反复敲上游好。
    pub async fn post<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &impl Serialize,
    ) -> Result<T> {
        let response = self
            .request(reqwest::Method::POST, url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("请求 {url}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("{} 返回 HTTP {status}", self.name));
        }
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("读取 {} 的响应", self.name))?;
        serde_json::from_slice(&bytes).with_context(|| format!("解析 {} 的响应", self.name))
    }

    async fn attempt_get(&self, url: &str) -> std::result::Result<Vec<u8>, Refusal> {
        let response = match self.request(reqwest::Method::GET, url).send().await {
            Ok(response) => response,
            Err(error) => return Err(Refusal::Retryable(error.into())),
        };
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(Refusal::Final(anyhow!("请求过于频繁，稍后再试")));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(Refusal::Final(anyhow!("{} 上没有这个条目", self.name)));
        }
        if status.is_client_error() {
            return Err(Refusal::Final(anyhow!("{} 返回 HTTP {status}", self.name)));
        }
        if !status.is_success() {
            return Err(Refusal::Retryable(anyhow!(
                "{} 返回 HTTP {status}",
                self.name
            )));
        }
        match response.bytes().await {
            Ok(bytes) => Ok(bytes.to_vec()),
            Err(error) => Err(Refusal::Retryable(error.into())),
        }
    }
}
