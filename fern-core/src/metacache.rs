//! 元数据缓存。
//!
//! 之前每次启动都是现拉：先拉一遍版本清单，再拉一遍这个版本的 JSON，再拉一遍
//! 资源索引——三次往返换来三份我们上一次已经拿到过、而且根本没有变的东西。
//! 代价是每次启动都得等网络，而没网的时候一个明明已经装好的实例完全打不开。
//!
//! 分类的判据只有一条：**内容会不会变。**
//!
//! | | 例子 | 策略 |
//! |---|---|---|
//! | 不可变 | 版本 JSON、资源索引、运行时文件清单 | 上游连 sha1 一起发布，一个 id 对应的内容就是那一份。本地对得上就永远不必再拉。 |
//! | 可变 | 版本清单、加载器版本列表、运行时索引 | 它们回答的是「现在有哪些」，答案每周都在变。带 TTL，过期去刷。 |
//!
//! 「刷不到就用旧的」是这里最重要的一条。列表旧了几个小时，用户顶多看不到
//! 昨晚发的那个快照；而拉不到就报错，等于网络一抖整个启动器就不能用。旧数据
//! 几乎总比没有数据好——前提是我们知道自己在用旧的。所以退回旧数据时会往
//! 启动器日志里写一行：这件事一定要留痕，否则「为什么新版本没出现」将来会
//! 变成一个完全没有线索的问题。

use std::{
    path::Path,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, sha1_matches};

use crate::DataPaths;

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// 各种「现在有哪些」列表的保鲜期。
///
/// 六小时：快照大约一周一发，正式版更少。设短了等于取消缓存；设长了会变成
/// 「刚发布的版本建不了实例」——所以还有 `Freshness::Force`，界面上的刷新
/// 按钮和「清单里没找到要的版本」都走它。
pub const LISTING_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// 要多新的一份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// 超过这个岁数就去刷新。
    Within(Duration),
    /// 一定去刷新。用户按了刷新，或者我们在旧数据里没找到要找的东西。
    Force,
}

#[derive(Debug, Clone)]
pub struct Cached {
    pub bytes: Vec<u8>,
    /// 这次没有成功刷新过，`bytes` 是从磁盘读的。
    ///
    /// 用途只有一个：在缓存里没找到要找的东西时，判断该不该强制刷一次再找。
    /// 刚拉下来的那份里没有，就是真的没有。
    pub from_cache: bool,
}

/// 可变的那一类：带 TTL，刷不到用旧的。
pub async fn mutable(
    downloader: &DownloadClient,
    paths: &DataPaths,
    slug: &str,
    url: &str,
    freshness: Freshness,
) -> Result<Cached> {
    // slug 里会拼进游戏版本号这种来自网络的字符串，它要变成文件名。
    if !is_safe_slug(slug) {
        return Err(anyhow!("缓存名不能作为文件名：{slug}"));
    }
    let path = paths.cache.join(slug);
    let local = read_with_age(&path).await;

    if let Some((bytes, age)) = &local
        && acceptable(*age, freshness)
    {
        return Ok(Cached {
            bytes: bytes.clone(),
            from_cache: true,
        });
    }

    match downloader.fetch(url).await {
        Ok(bytes) => {
            write_atomic(&path, &bytes).await?;
            Ok(Cached {
                bytes,
                from_cache: false,
            })
        }
        // 连旧的都没有的时候不吞错误：那时确实什么都做不了，说出原因比说
        // 「没有数据」有用。
        Err(error) => match local {
            Some((bytes, age)) => {
                // 留痕。不留的话，「为什么昨晚发的快照没出现」将来是一个
                // 完全没有线索的问题。
                let _ = paths.append_log(&format!(
                    "[metacache] {slug} 刷新失败（{error:#}），改用 {} 分钟前的缓存",
                    age.as_secs() / 60
                ));
                Ok(Cached {
                    bytes,
                    from_cache: true,
                })
            }
            None => Err(error).with_context(|| format!("读取 {url}")),
        },
    }
}

/// 不可变的那一类：本地对得上就直接用。
///
/// `path` 是这份文件本来就该待的地方（`versions/<id>/<id>.json`、
/// `assets/indexes/<id>.json`），而不是另存一份副本。缓存和成品是同一个文件，
/// 才不会出现「缓存里有、成品里没有」这种最难查的分叉。
pub async fn immutable(
    downloader: &DownloadClient,
    path: &Path,
    url: &str,
    sha1: Option<&str>,
    size: Option<u64>,
) -> Result<Vec<u8>> {
    if let Some(bytes) = read_verified(path, sha1, size).await {
        return Ok(bytes);
    }
    let bytes = downloader
        .fetch(url)
        .await
        .with_context(|| format!("读取 {url}"))?;
    verify(&bytes, sha1, size).with_context(|| format!("{url} 校验失败"))?;
    write_atomic(path, &bytes).await?;
    Ok(bytes)
}

fn acceptable(age: Duration, freshness: Freshness) -> bool {
    match freshness {
        Freshness::Within(ttl) => age < ttl,
        Freshness::Force => false,
    }
}

/// 本地那一份，验过才算数。
///
/// 上游没给 sha1 的时候（Mojang 清单里有些老版本就是这样）只能认「文件在」。
/// 这不算妥协：那份文件是我们自己写下去的，而且它就是启动时真正会读的那一份，
/// 再去要求一个我们拿不到的校验值只会让它永远缓存不上。
async fn read_verified(path: &Path, sha1: Option<&str>, size: Option<u64>) -> Option<Vec<u8>> {
    let bytes = tokio::fs::read(path).await.ok()?;
    verify(&bytes, sha1, size).ok()?;
    Some(bytes)
}

fn verify(bytes: &[u8], sha1: Option<&str>, size: Option<u64>) -> Result<()> {
    if let Some(expected) = size
        && bytes.len() as u64 != expected
    {
        return Err(anyhow!(
            "大小不符：期望 {expected} 字节，实际 {}",
            bytes.len()
        ));
    }
    if let Some(expected) = sha1
        && !sha1_matches(bytes, expected)
    {
        return Err(anyhow!("sha1 与 {expected} 不符"));
    }
    Ok(())
}

/// 用文件自己的修改时间当「什么时候拉的」。
///
/// 不另写一个时间戳文件：那样缓存目录里每份数据都成了两个文件，而且两者迟早
/// 会不同步。mtime 被备份还原之类的事情弄乱了，最坏也只是多刷一次。
async fn read_with_age(path: &Path) -> Option<(Vec<u8>, Duration)> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let modified = tokio::fs::metadata(path).await.ok()?.modified().ok()?;
    // 时钟往回跳过的话 duration_since 会失败，当成刚拉的——宁可多用一会儿，
    // 也不要因为系统时间不对就把缓存判成永远过期。
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    Some((bytes, age))
}

fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 128
        && !slug.contains("..")
        && !slug.starts_with('.')
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// 先写 `.part` 再改名。
///
/// 中途断电留下的是一个没人读的临时文件，而不是一份读得出来、内容却只有一半
/// 的版本 JSON——后者会让下一次启动带着半份元数据跑，错得毫无线索。
async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension("part");
    tokio::fs::write(&temporary, bytes).await?;
    if tokio::fs::try_exists(path).await? {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::source_order;

    /// 解析得了但连不上的地址。用它是为了证明「命中缓存时一次网络都没发」：
    /// 真去请求了，测试就会失败而不是变慢。
    const UNREACHABLE: &str = "https://127.0.0.1:1/never";

    fn client() -> DownloadClient {
        DownloadClient::new(source_order(), 2)
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fern-metacache-{name}-{}", std::process::id()))
    }

    #[tokio::test]
    async fn an_immutable_file_that_matches_its_hash_is_never_fetched_again() {
        let root = temp("immutable");
        let path = root.join("versions/1.21.1/1.21.1.json");
        let body = br#"{"id":"1.21.1"}"#;
        write_atomic(&path, body).await.expect("write local copy");
        let digest = "35df22dcdde9c22f9bf00d5c022a38933507def4";

        let bytes = immutable(
            &client(),
            &path,
            UNREACHABLE,
            Some(digest),
            Some(body.len() as u64),
        )
        .await
        .expect("本地对得上就不该碰网络");
        assert_eq!(bytes, body);

        std::fs::remove_dir_all(root).expect("clean up");
    }

    #[tokio::test]
    async fn an_immutable_file_that_does_not_match_is_not_served() {
        let root = temp("corrupt");
        let path = root.join("indexes/17.json");
        write_atomic(&path, b"truncated").await.expect("write");

        // 大小对不上就当没有：宁可去拉一次（这里必然失败），也不能把半份
        // 索引交出去——那会让补全少下几千个文件却报告成功。
        let result = immutable(&client(), &path, UNREACHABLE, None, Some(9999)).await;
        assert!(result.is_err());

        std::fs::remove_dir_all(root).expect("clean up");
    }

    #[tokio::test]
    async fn a_listing_within_its_ttl_is_served_without_the_network() {
        let root = temp("mutable");
        let paths = DataPaths::new(&root);
        write_atomic(&paths.cache.join("listing.json"), b"[1,2,3]")
            .await
            .expect("write");

        let cached = mutable(
            &client(),
            &paths,
            "listing.json",
            UNREACHABLE,
            Freshness::Within(LISTING_TTL),
        )
        .await
        .expect("刚写下的副本远在 TTL 之内，不该联网");
        assert_eq!(cached.bytes, b"[1,2,3]");
        assert!(cached.from_cache);

        std::fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn freshness_decides_only_by_age() {
        let hour = Duration::from_secs(3600);
        assert!(acceptable(hour, Freshness::Within(hour * 6)));
        assert!(!acceptable(hour * 7, Freshness::Within(hour * 6)));
        // 强制刷新连刚拉的都不认——它是「我就是要最新的」那一下。
        assert!(!acceptable(Duration::ZERO, Freshness::Force));
    }

    #[test]
    fn slugs_never_climb_out_of_the_cache_directory() {
        for slug in ["version_manifest_v2.json", "loader-fabric-1.21.1.json"] {
            assert!(is_safe_slug(slug), "{slug} 应当合法");
        }
        // 加载器的 slug 里拼着游戏版本号，那是来自网络的字符串。
        for slug in ["", "../settings.json", "a/b", ".hidden", "a..b"] {
            assert!(!is_safe_slug(slug), "{slug} 不该被当成缓存名");
        }
    }
}
