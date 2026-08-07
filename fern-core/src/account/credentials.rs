//! 令牌的保管处。
//!
//! 访问令牌等同于账号：拿到它就能以这个玩家的身份进服务器。所以它进系统
//! 钥匙串（macOS 的 Keychain、Windows 的凭据管理器、Linux 的 Secret Service），
//! 不进 `settings.json`——那是一份用户会打开、会备份、会贴给别人的文件。
//!
//! 钥匙串用不了的时候（比如没跑桌面会话的 Linux）直接说用不了，**不**退回
//! 明文文件。那等于把「我们保管好了」换成「我们看起来保管好了」，而用户不会
//! 知道差别。宁可让他看到一句「这台机器上用不了外置登录」。

use anyhow::{Context, Result};

use crate::account::yggdrasil::YggdrasilSession;

const SERVICE: &str = "fern-launcher";
/// 单账户时代的两个固定键。只有迁移还会读它们（见 accounts.rs），读得出来
/// 就搬到 `session-<id>` 并删掉——令牌不该同时躺在两个地方。
const YGGDRASIL_ENTRY: &str = "yggdrasil-session";
const MICROSOFT_ENTRY: &str = "microsoft-session";
/// 这台机器的标识，不是秘密，但和令牌绑在一起才有意义，放在同一处。
const CLIENT_TOKEN_ENTRY: &str = "client-token";

/// 一个账户一条。键里带的是账户 id，不是名字——名字会改，id 不会。
fn session_entry(id: &str) -> String {
    format!("session-{id}")
}

/// 存一个账户的令牌。
pub fn store_secret(id: &str, secret: &crate::account::roster::Secret) -> Result<()> {
    let json = serde_json::to_string(secret).context("序列化登录信息")?;
    entry(&session_entry(id))?
        .set_password(&json)
        .map_err(unavailable)
}

/// 读回来。没有条目返回 `None`——离线账户本来就没有，那是正常状态。
pub fn load_secret(id: &str) -> Result<Option<crate::account::roster::Secret>> {
    let entry = entry(&session_entry(id))?;
    match entry.get_password() {
        Ok(json) => Ok(serde_json::from_str(&json).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(unavailable(error)),
    }
}

/// 删掉一个账户的令牌。没有条目也算成功——目的是「之后读不到」，已经成立了。
pub fn clear_secret(id: &str) -> Result<()> {
    match entry(&session_entry(id))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(unavailable(error)),
    }
}

fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(unavailable)
}

/// 钥匙串用不了时给一句能照做的话。
///
/// keyring 抛上来的原文（「Secret Service: no result found」）会让人以为是
/// 没登录过，而实际原因通常是这台机器根本没跑桌面会话——两件事该说的话
/// 完全不同。
fn unavailable(error: keyring::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "无法访问系统钥匙串（{error}）。\
         桌面环境通常是密钥环未解锁；服务器或远程会话可能未安装密钥环服务。"
    )
}

/// 读回来。没登录过返回 `None`，而不是错误——那是正常状态。
pub fn load_session() -> Result<Option<YggdrasilSession>> {
    let entry = entry(YGGDRASIL_ENTRY)?;
    match entry.get_password() {
        Ok(json) => Ok(serde_json::from_str(&json).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(unavailable(error)),
    }
}

/// 退出登录。没有条目也算成功——目的是「之后读不到」，那已经成立了。
pub fn clear_session() -> Result<()> {
    match entry(YGGDRASIL_ENTRY)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(unavailable(error)),
    }
}

pub fn load_microsoft_session() -> Result<Option<crate::account::microsoft::MicrosoftSession>> {
    let entry = entry(MICROSOFT_ENTRY)?;
    match entry.get_password() {
        Ok(json) => Ok(serde_json::from_str(&json).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(unavailable(error)),
    }
}

pub fn clear_microsoft_session() -> Result<()> {
    match entry(MICROSOFT_ENTRY)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(unavailable(error)),
    }
}

/// 这台机器的 client token，第一次用时生成。
///
/// 必须稳定：换一个就等于换一台设备，服务端会把之前发出去的令牌作废，用户
/// 表现为「每次启动都要重新登录」。
pub fn client_token() -> Result<String> {
    let entry = entry(CLIENT_TOKEN_ENTRY)?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => return Ok(token),
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(unavailable(error)),
    }
    let token = generate_client_token();
    entry.set_password(&token).map_err(unavailable)?;
    Ok(token)
}

/// 生成一个够用的随机标识。
///
/// 这不是密钥，只是一个「哪台机器」的标签，服务端拿它做等值比较。所以不必
/// 引一个随机数库：进程地址、时间、进程号混一把哈希就足够不撞。
fn generate_client_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stack = &nanos as *const _ as usize;
    let seed = format!("{nanos}-{}-{stack}", std::process::id());
    let digest: [u8; 16] = md5::Md5::digest(seed.as_bytes()).into();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

use md5::Digest;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_stable_length_and_do_not_repeat() {
        let first = generate_client_token();
        let second = generate_client_token();
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second, "两次生成撞在一起说明种子没有熵");
    }
}
