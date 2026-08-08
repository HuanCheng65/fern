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
    store(&session_entry(id), &json)
}

/// 读回来。没有条目返回 `None`——离线账户本来就没有，那是正常状态。
pub fn load_secret(id: &str) -> Result<Option<crate::account::roster::Secret>> {
    Ok(load(&session_entry(id))?.and_then(|json| serde_json::from_str(&json).ok()))
}

/// 删掉一个账户的令牌。没有条目也算成功——目的是「之后读不到」，已经成立了。
pub fn clear_secret(id: &str) -> Result<()> {
    clear(&session_entry(id))
}

/// Windows 凭据管理器给一条凭据的密码划了死线：转成 UTF-16 之后不超过 2560
/// 字节，也就是 1280 个码元。一份微软会话装着 MSA 的 refresh token 和
/// Minecraft 的 JWT，两个加起来轻松翻倍——所以正版登录在 Windows 上必然写不
/// 进去，而 macOS 的 Keychain 没有这个限制，同一份代码在那边一直是好的。
///
/// 留下的余量给分片头，也给将来会话里多出来的字段。
const CHUNK_UNITS: usize = 1200;

/// 一条装不下时，第一条的开头写上总片数。
///
/// JSON 不会以它开头，所以读的时候一眼能分辨这条是分片头还是一份完整的值——
/// 老版本写下的整条凭据（以及所有本来就够短的值）照样按原样读回来，不需要
/// 迁移，也不会因为升级把已经登录的人踢下线。
const CHUNK_MARKER: &str = "fern:chunks=";

/// 第 0 片就用键本身，后面的挂在它下面。
fn chunk_key(key: &str, index: usize) -> String {
    format!("{key}#{index}")
}

/// 按 UTF-16 码元切，且不切开一个字符。
///
/// 令牌是 ASCII，但玩家名不一定——按字节切会留下半个字符，写进去再读回来就
/// 不是原来那串了。
fn split(value: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut units = 0;
    for (index, character) in value.char_indices() {
        let width = character.len_utf16();
        if units + width > CHUNK_UNITS {
            chunks.push(&value[start..index]);
            start = index;
            units = 0;
        }
        units += width;
    }
    chunks.push(&value[start..]);
    chunks
}

/// 拆开分片头。不是分片头就返回 `None`，那说明这条本身就是完整的值。
fn parse_head(value: &str) -> Option<(usize, &str)> {
    let rest = value.strip_prefix(CHUNK_MARKER)?;
    let (count, first) = rest.split_once('\n')?;
    Some((count.parse().ok()?, first))
}

fn store(key: &str, value: &str) -> Result<()> {
    let chunks = split(value);
    if chunks.len() == 1 {
        // 够短的照旧存成一条，和分片之前的格式逐字节一致。
        entry(key)?.set_password(value).map_err(unavailable)?;
    } else {
        let head = format!("{CHUNK_MARKER}{}\n{}", chunks.len(), chunks[0]);
        entry(key)?.set_password(&head).map_err(unavailable)?;
        for (index, chunk) in chunks.iter().enumerate().skip(1) {
            entry(&chunk_key(key, index))?
                .set_password(chunk)
                .map_err(unavailable)?;
        }
    }
    // 上一份可能比这份长。多出来的尾巴不清掉，下次读会把旧数据接在后面。
    prune(key, chunks.len());
    Ok(())
}

fn load(key: &str) -> Result<Option<String>> {
    let head = match entry(key)?.get_password() {
        Ok(value) => value,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(error) => return Err(unavailable(error)),
    };
    let Some((count, first)) = parse_head(&head) else {
        return Ok(Some(head));
    };
    let mut value = first.to_owned();
    for index in 1..count {
        match entry(&chunk_key(key, index))?.get_password() {
            Ok(chunk) => value.push_str(&chunk),
            // 缺一片就是缺一片。交半份出去只会变成一个更难查的错误，不如当作
            // 没登录过，让用户重新登录一次。
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(error) => return Err(unavailable(error)),
        }
    }
    Ok(Some(value))
}

fn clear(key: &str) -> Result<()> {
    prune(key, 1);
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(unavailable(error)),
    }
}

/// 从第 `from` 片开始往后删。分片是连着的，碰到第一个空位就到头了。
///
/// 永远不碰第 0 片——那是键本身，删掉就等于把这条凭据整个删了。
fn prune(key: &str, from: usize) {
    for index in from.max(1).. {
        let Ok(entry) = entry(&chunk_key(key, index)) else {
            return;
        };
        if entry.delete_credential().is_err() {
            return;
        }
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
    Ok(load(YGGDRASIL_ENTRY)?.and_then(|json| serde_json::from_str(&json).ok()))
}

/// 退出登录。没有条目也算成功——目的是「之后读不到」，那已经成立了。
pub fn clear_session() -> Result<()> {
    clear(YGGDRASIL_ENTRY)
}

pub fn load_microsoft_session() -> Result<Option<crate::account::microsoft::MicrosoftSession>> {
    Ok(load(MICROSOFT_ENTRY)?.and_then(|json| serde_json::from_str(&json).ok()))
}

pub fn clear_microsoft_session() -> Result<()> {
    clear(MICROSOFT_ENTRY)
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

    /// Windows 凭据管理器的实际上限，写死在这里当作回归基准。
    const WINDOWS_LIMIT_UNITS: usize = 2560 / 2;

    #[test]
    fn a_microsoft_session_fits_the_windows_credential_limit_once_split() {
        // 真实量级：MSA 的 refresh token 和 Minecraft 的 JWT 各自上千字符，
        // 合起来必然超过 Windows 的一条上限——这正是正版登录只在那边报错的原因。
        let session = crate::account::roster::Secret::Microsoft(
            crate::account::microsoft::MicrosoftSession {
                refresh_token: "r".repeat(1800),
                access_token: "a".repeat(1400),
                uuid: "0".repeat(32),
                player_name: "Steve".to_owned(),
                expires_at: 0,
            },
        );
        let json = serde_json::to_string(&session).expect("serialize");
        assert!(
            json.encode_utf16().count() > WINDOWS_LIMIT_UNITS,
            "样本不够长"
        );

        let chunks = split(&json);
        assert!(chunks.len() > 1);
        // 第 0 片还要带上分片头，也得算进去。
        let head = format!("{CHUNK_MARKER}{}\n{}", chunks.len(), chunks[0]);
        assert!(head.encode_utf16().count() <= WINDOWS_LIMIT_UNITS);
        for chunk in &chunks[1..] {
            assert!(chunk.encode_utf16().count() <= WINDOWS_LIMIT_UNITS);
        }
        assert_eq!(chunks.concat(), json);
    }

    #[test]
    fn splitting_never_cuts_a_character_in_half() {
        // 玩家名不保证是 ASCII。按字节切会留下半个字符，读回来就不是原来那串。
        // 一个 ASCII 打头，后面全是占两个码元的字符：这样切点必然落在一个
        // 字符中间，按字节切的实现会在这里裂开。
        let value = format!("x{}", "🌿".repeat(CHUNK_UNITS));
        let chunks = split(&value);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), value);
        for chunk in &chunks {
            assert!(chunk.encode_utf16().count() <= CHUNK_UNITS);
        }
    }

    #[test]
    fn a_short_value_is_stored_exactly_as_before() {
        // 够短的值不带头，格式和分片之前逐字节一致——升级不该让已经登录的人
        // 被登出，读旧条目走的就是这条路。
        let json = r#"{"kind":"yggdrasil","accessToken":"short"}"#;
        assert_eq!(split(json), vec![json]);
        assert!(parse_head(json).is_none());
    }

    #[test]
    fn a_chunk_head_round_trips() {
        let head = format!("{CHUNK_MARKER}3\n{{\"kind\"");
        let (count, first) = parse_head(&head).expect("是分片头");
        assert_eq!(count, 3);
        assert_eq!(first, "{\"kind\"");
        // 数字读不出来的头不能当成分片，否则会把它当片数去拼一堆不存在的片。
        assert!(parse_head(&format!("{CHUNK_MARKER}x\n{{")).is_none());
        assert!(parse_head(CHUNK_MARKER).is_none());
    }

    #[test]
    fn chunk_keys_stay_under_the_key_they_belong_to() {
        let key = session_entry("abc");
        assert_eq!(key, "session-abc");
        assert_eq!(chunk_key(&key, 1), "session-abc#1");
    }

    #[test]
    fn generated_tokens_are_stable_length_and_do_not_repeat() {
        let first = generate_client_token();
        let second = generate_client_token();
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second, "两次生成撞在一起说明种子没有熵");
    }
}
