//! 皮肤头像。
//!
//! 玩家认自己靠皮肤，不靠一串字母。这在同一个名字下同时挂着正版号、离线号和
//! 某个皮肤站的号时尤其要紧——那是完全合法的状态（见 [`super::roster`] 的去重
//! 键），而三个 Steve 的生成式头像还可能撞：有些皮肤站的 UUID 用的就是离线那
//! 套名字派生算法。
//!
//! 取的是**公开档案**，不是登录态。`sessionserver` 那个接口不要令牌，给一个
//! UUID 就答。所以这条路和刷新令牌完全无关：皮肤过一天自己更新，令牌坏了也不
//! 影响这张脸，而且这里从头到尾一个令牌都不碰。
//!
//! 交给界面的是**整张皮肤 PNG**，头部由 CSS 去裁（8,8 起的 8×8 是头，40,8 起
//! 的那一层是帽子，64×32 的老皮肤这两块也在同样的位置）。在 Rust 里裁要引一个
//! 图像库，而这张图本来就只有几 KB——裁不裁都是同一次传输，那就把它留在本来就
//! 会做这件事的那一层。
//!
//! 拉不到就返回 `None`，永远不返回错误。脸是加分项：它缺席时退回生成式色块，
//! 而把它做成一个会失败的东西，等于让皮肤站抽风变成账户列表打不开。

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::DataPaths;

use super::roster::{AccountKind, AccountRecord};

/// 正版的公开档案。要不到令牌，也不需要。
const MOJANG_SESSION: &str = "https://sessionserver.mojang.com";

/// 档案的保鲜期。皮肤是人偶尔换一次的东西，一天足够新。
const PROFILE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// 一张皮肤最大就 64×64，几 KB。给出这个上限是因为地址来自网络——皮肤站可以
/// 在那里放任何东西，而我们要把它整个读进内存。
const MAX_BYTES: usize = 256 * 1024;

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// 这个账户的皮肤 PNG。
///
/// 离线号没有皮肤，直接是 `None`——不是失败，是这种账户本来就没有这个东西。
pub async fn skin(paths: &DataPaths, record: &AccountRecord) -> Option<Vec<u8>> {
    let session_root = match record.kind {
        AccountKind::Offline => return None,
        AccountKind::Microsoft => MOJANG_SESSION.to_owned(),
        // 外置登录的档案在它自己的站上。同一个名字在两个站是两个人，皮肤当然
        // 也各是各的。
        AccountKind::Authlib => format!(
            "{}/sessionserver",
            record.api_root.as_deref()?.trim_end_matches('/')
        ),
    };
    // Mojang 的档案接口只认不带连字符的那种写法，而两边存下来的可能是任一种。
    let uuid = record.uuid.replace('-', "");
    if uuid.is_empty() || !uuid.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let profile = fetch(
        paths,
        &slug("profile", &record.id),
        &format!("{session_root}/session/minecraft/profile/{uuid}"),
        Some(PROFILE_TTL),
    )
    .await?;
    let url = texture_url(&profile)?;
    // 皮肤本身按地址缓存，不按账户：那串地址是内容的指纹（Mojang 直接用哈希
    // 当文件名），所以换了皮肤就是换了一个文件，不会读到上一张。
    fetch(paths, &slug("skin", &url), &url, None).await
}

/// 档案里那一段 base64 藏着贴图地址。
fn texture_url(profile: &[u8]) -> Option<String> {
    let profile: Profile = serde_json::from_slice(profile).ok()?;
    let encoded = profile
        .properties
        .into_iter()
        .find(|property| property.name == "textures")?
        .value;
    let textures: Textures = serde_json::from_slice(&STANDARD.decode(encoded).ok()?).ok()?;
    let url = textures.textures.skin?.url;
    // 地址来自网络，而它下一步会被当成一个要去取的东西。只走 http(s)。
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

/// 取一份东西，带磁盘缓存。
///
/// `ttl` 为 `None` 表示这份内容由地址唯一确定，本地有就永远不必再问。
///
/// 三条纪律：拉不到时用旧的（皮肤站抽风不该让脸消失）；旧的也没有就返回
/// `None`（不抛错，见模块头）；写盘先落 `.part` 再改名（半张图比没有图更糟，
/// 它会被当成一张真的图缓存下来）。
async fn fetch(paths: &DataPaths, slug: &str, url: &str, ttl: Option<Duration>) -> Option<Vec<u8>> {
    let path = directory(paths).join(slug);
    let local = read_with_age(&path).await;
    if let Some((bytes, age)) = &local
        && ttl.is_none_or(|ttl| *age < ttl)
    {
        return Some(bytes.clone());
    }

    match download(url).await {
        Some(bytes) => {
            let _ = write_atomic(&path, &bytes).await;
            Some(bytes)
        }
        None => local.map(|(bytes, _)| bytes),
    }
}

async fn download(url: &str) -> Option<Vec<u8>> {
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?
        .get(url)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    // 先看它自己声明了多大，再看实际读到多少——两道都要，因为声明可以撒谎。
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    // 空响应体不是一份内容。皮肤站对不认识的 UUID 会回 200 加一个空 body，
    // 照单收下就会在缓存里留一个 0 字节的文件，而它每次都读不出东西、每次都
    // 要重下一遍。
    (!bytes.is_empty() && bytes.len() <= MAX_BYTES).then(|| bytes.to_vec())
}

/// 缓存文件名。
///
/// 一律哈希，不管原文长什么样：账户 id 和贴图地址都可能来自网络，而它们下一步
/// 要变成一个路径。哈希之后不必再写一遍「这里能不能出现斜杠」那种判断。
fn slug(kind: &str, key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    format!("{kind}-{:x}", hasher.finalize())
}

/// 皮肤全放在缓存目录里：它属于「随时可以整个删掉、下次联网自己长回来」的那
/// 一类，和版本清单同一个性质。
fn directory(paths: &DataPaths) -> PathBuf {
    paths.cache.join("skins")
}

async fn read_with_age(path: &Path) -> Option<(Vec<u8>, Duration)> {
    let bytes = tokio::fs::read(path).await.ok()?;
    if !bytes.starts_with(&PNG_MAGIC) && serde_json::from_slice::<Profile>(&bytes).is_err() {
        return None;
    }
    let age = tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .unwrap_or(Duration::MAX);
    Some((bytes, age))
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = path.with_extension("part");
    tokio::fs::write(&part, bytes).await?;
    tokio::fs::rename(&part, path).await
}

/// 交给界面的一张脸。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSkin {
    /// 能直接塞进 `background-image` 的地址。
    pub url: String,
    /// 这张皮肤的帽子层能不能叠。
    ///
    /// **64×32 的老皮肤不能。** 那个年代的格式没有 alpha，帽子层用**纯黑当透明
    /// 键**，所以一张老皮肤的帽子区域整块是不透明的黑——无条件叠上去，玩家看到
    /// 的就是一颗纯黑的头（Notch 自己那张就是）。老皮肤因此宁可不画帽子：少一顶
    /// 帽子，好过整张脸没了。
    pub hat: bool,
}

/// 界面要的那一步。
pub async fn of_record(paths: &DataPaths, record: &AccountRecord) -> Option<AccountSkin> {
    let bytes = skin(paths, record).await?;
    if !bytes.starts_with(&PNG_MAGIC) {
        return None;
    }
    Some(AccountSkin {
        hat: height(&bytes).is_some_and(|height| height >= 64),
        url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
    })
}

/// PNG 的高度，从 IHDR 里读。为此引一个图像库不值得——这是文件开头固定位置上
/// 的一个大端 u32。
fn height(bytes: &[u8]) -> Option<u32> {
    bytes
        .get(20..24)
        .map(|field| u32::from_be_bytes([field[0], field[1], field[2], field[3]]))
}

#[derive(Deserialize)]
struct Profile {
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct Textures {
    textures: TextureSet,
}

#[derive(Deserialize)]
struct TextureSet {
    #[serde(rename = "SKIN")]
    skin: Option<Texture>,
}

#[derive(Deserialize)]
struct Texture {
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 档案里那一段 base64 是这个接口唯一的产出，解错了整条链就断在这里。
    #[test]
    fn the_texture_url_comes_out_of_the_encoded_property() {
        let textures = STANDARD.encode(
            br#"{"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/abc"}}}"#,
        );
        let profile =
            format!(r#"{{"id":"x","properties":[{{"name":"textures","value":"{textures}"}}]}}"#);
        assert_eq!(
            texture_url(profile.as_bytes()).as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
    }

    /// 地址来自皮肤站，而我们拿它去发请求。非 http(s) 的一律不认。
    #[test]
    fn a_texture_url_that_is_not_http_is_refused() {
        let textures = STANDARD.encode(br#"{"textures":{"SKIN":{"url":"file:///etc/passwd"}}}"#);
        let profile =
            format!(r#"{{"id":"x","properties":[{{"name":"textures","value":"{textures}"}}]}}"#);
        assert_eq!(texture_url(profile.as_bytes()), None);
    }

    /// 没有皮肤的档案是常态（没设置过皮肤的正版号），不该当成坏数据。
    #[test]
    fn a_profile_without_a_skin_is_not_an_error() {
        let textures = STANDARD.encode(br#"{"textures":{}}"#);
        let profile =
            format!(r#"{{"id":"x","properties":[{{"name":"textures","value":"{textures}"}}]}}"#);
        assert_eq!(texture_url(profile.as_bytes()), None);
        assert_eq!(texture_url(b"{}"), None);
    }

    /// 缓存名里不能出现来自网络的字符——它是一个路径。
    #[test]
    fn a_cache_name_never_carries_anything_from_the_network() {
        let name = slug("skin", "https://evil.example/../../etc/passwd");
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
    }

    /// 老皮肤的帽子层是不透明的纯黑（那个年代拿黑当透明键），叠上去整颗头就
    /// 黑了。判据只有高度：64×32 是老的，64×64 才有真的 alpha。
    #[test]
    fn a_legacy_skin_does_not_get_its_hat_layer() {
        let png = |width: u32, tall: u32| {
            let mut bytes = PNG_MAGIC.to_vec();
            bytes.extend_from_slice(&[0, 0, 0, 13]);
            bytes.extend_from_slice(b"IHDR");
            bytes.extend_from_slice(&width.to_be_bytes());
            bytes.extend_from_slice(&tall.to_be_bytes());
            bytes
        };
        assert_eq!(height(&png(64, 32)), Some(32));
        assert_eq!(height(&png(64, 64)), Some(64));
        // 截断的头读不出高度，那时不叠帽子——猜错的代价是一颗黑头。
        assert_eq!(height(b"\x89PNG"), None);
    }
}
