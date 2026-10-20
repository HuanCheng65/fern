//! 整合包（Modrinth 的 `.mrpack`）。
//!
//! 一个 mrpack 就是个 zip：里面一份 `modrinth.index.json` 说清「要哪个游戏
//! 版本、哪个加载器、下哪些文件」，加一个 `overrides/` 目录直接铺进游戏目录
//! （配置、脚本、自带的资源包这些没法从网上按 hash 取的东西）。
//!
//! **装整合包不是「装东西到某个实例」，是「建一个实例」。** 它自带游戏版本和
//! 加载器，往一个已有实例上盖只会得到一个谁也说不清是什么的混合体。所以这里
//! 返回的是一个新建的 InstanceProfile。
//!
//! 安全上有三处关口，都是因为 zip 里的名字和 index 里的路径都来自网络：
//!
//! - `files[].path` 和 zip 条目名一律走 `safe_join`，挡 `../` 和绝对路径。
//! - `downloads[]` 只认 https。它们会被交给下载器，`file://` 之类没有道理。
//! - 加载器版本号进的是版本 id 和目录名，所以按 `是否只含安全字符` 过一遍。

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadTask, safe_join};
use serde::{Deserialize, Serialize};

use super::{
    Source,
    cfpack::{ManualFile, PackFile},
    sideload::Sideload,
};
use crate::{
    DataPaths, InstanceProfile, LoaderKind,
    instance::pack::{PackRecord, PackRef},
};
use std::collections::BTreeMap;

/// 这一版只认得 formatVersion 1。往后的格式没见过就直说，别猜着装。
const SUPPORTED_FORMAT: u32 = 1;

/// index 里的元信息，够界面在装之前先说清「这是什么」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: String,
    /// 要从网上取的文件数。
    pub files: usize,
    /// 哪一家的格式。
    pub source: Source,
}

// ——— index 的 JSON 形状 ———

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    format_version: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    files: Vec<IndexFile>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    path: String,
    #[serde(default)]
    hashes: Hashes,
    #[serde(default)]
    env: Option<Env>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

#[derive(Debug, Default, Deserialize)]
struct Hashes {
    #[serde(default)]
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct Env {
    #[serde(default)]
    client: String,
}

/// 从 dependencies 里认出加载器。键名是格式定的，不是我们能选的。
fn loader_from(dependencies: &std::collections::HashMap<String, String>) -> (LoaderKind, String) {
    for (key, kind) in [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("neoforge", LoaderKind::NeoForge),
        ("forge", LoaderKind::Forge),
    ] {
        if let Some(version) = dependencies.get(key) {
            return (kind, version.clone());
        }
    }
    (LoaderKind::Vanilla, String::new())
}

/// 版本号会进版本 id 和目录名，所以只放行安全字符。
fn is_safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

fn read_index(archive_path: &Path) -> Result<Index> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("打开 {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("这个文件不是有效的 zip")?;
    let mut entry = archive
        .by_name("modrinth.index.json")
        .context("压缩包里没有 modrinth.index.json，它可能不是一个 mrpack")?;
    let mut text = String::new();
    entry.read_to_string(&mut text).context("读取 index")?;
    let index: Index = serde_json::from_str(&text).context("解析 modrinth.index.json")?;
    if index.format_version != SUPPORTED_FORMAT {
        return Err(anyhow!(
            "不支持的整合包格式版本 {}（本启动器支持 {SUPPORTED_FORMAT}）",
            index.format_version
        ));
    }
    Ok(index)
}

/// 一个整合包，两种格式之一。
enum Pack {
    Modrinth(Index),
    CurseForge(super::cfpack::Manifest),
}

impl Pack {
    fn source(&self) -> Source {
        match self {
            Self::Modrinth(_) => Source::Modrinth,
            Self::CurseForge(_) => Source::CurseForge,
        }
    }
}

/// 认出是哪种整合包。认格式靠里面的清单文件，不靠扩展名——CurseForge 的
/// 就叫 `.zip`，用户也可能把 mrpack 改了名。
fn read_pack(archive_path: &Path) -> Result<Pack> {
    let has_index = {
        let file = std::fs::File::open(archive_path)
            .with_context(|| format!("打开 {}", archive_path.display()))?;
        let archive = zip::ZipArchive::new(file).context("这个文件不是有效的 zip")?;
        archive.index_for_name("modrinth.index.json").is_some()
    };
    if has_index {
        return Ok(Pack::Modrinth(read_index(archive_path)?));
    }
    match super::cfpack::read(archive_path)? {
        Some(manifest) => Ok(Pack::CurseForge(manifest)),
        None => Err(anyhow!(
            "压缩包里既没有 modrinth.index.json，也没有 manifest.json，它可能不是整合包"
        )),
    }
}

/// 先看一眼里面是什么，不动磁盘。
pub fn inspect(archive_path: &Path) -> Result<PackSummary> {
    summarize(archive_path, &read_pack(archive_path)?)
}

fn summarize(archive_path: &Path, pack: &Pack) -> Result<PackSummary> {
    let fallback = || {
        archive_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "整合包".to_owned())
    };
    match pack {
        Pack::Modrinth(index) => {
            let game_version = index
                .dependencies
                .get("minecraft")
                .cloned()
                .ok_or_else(|| anyhow!("整合包没有写明 Minecraft 版本"))?;
            let (loader, loader_version) = loader_from(&index.dependencies);
            Ok(PackSummary {
                name: if index.name.is_empty() {
                    fallback()
                } else {
                    index.name.clone()
                },
                version: index.version_id.clone(),
                summary: index.summary.clone(),
                game_version,
                loader,
                loader_version,
                files: index.files.len(),
                source: Source::Modrinth,
            })
        }
        Pack::CurseForge(manifest) => {
            if manifest.game_version().is_empty() {
                return Err(anyhow!("整合包没有写明 Minecraft 版本"));
            }
            let (loader, loader_version) = manifest.loader();
            Ok(PackSummary {
                name: if manifest.name.is_empty() {
                    fallback()
                } else {
                    manifest.name.clone()
                },
                version: manifest.version.clone(),
                summary: if manifest.author.is_empty() {
                    String::new()
                } else {
                    format!("作者：{}", manifest.author)
                },
                game_version: manifest.game_version().to_owned(),
                loader,
                loader_version,
                files: manifest.wanted().len(),
                source: Source::CurseForge,
            })
        }
    }
}

/// 客户端用不上的文件不下。整合包的 index 里常带一堆纯服务端的模组。
fn wanted(file: &IndexFile) -> bool {
    file.env
        .as_ref()
        .is_none_or(|env| env.client != "unsupported")
}

/// 要放进游戏目录的文件和 overrides 的前缀，两种格式翻成同一个形状。
///
/// CurseForge 的清单里只有 id，这一步要联网；手动下载的那几个也在这一步去找。
async fn files_of(pack: &Pack, sideload: &Sideload) -> Result<(Vec<PackFile>, Vec<String>)> {
    match pack {
        Pack::Modrinth(index) => {
            let mut files = Vec::new();
            for file in index.files.iter().filter(|file| wanted(file)) {
                let Some(url) = file
                    .downloads
                    .iter()
                    .find(|url| crate::is_external_url(url))
                else {
                    return Err(anyhow!("{} 没有可用的 https 下载地址", file.path));
                };
                let path = file.path.replace('\\', "/");
                files.push(PackFile {
                    title: path.clone(),
                    file_name: path.rsplit('/').next().unwrap_or_default().to_owned(),
                    path,
                    url: Some(url.clone()),
                    sha1: Some(file.hashes.sha1.clone()).filter(|hash| !hash.is_empty()),
                    size: file.file_size,
                    page: None,
                    local: None,
                });
            }
            Ok((
                files,
                vec!["overrides/".to_owned(), "client-overrides/".to_owned()],
            ))
        }
        Pack::CurseForge(manifest) => Ok((
            super::cfpack::resolve(manifest, sideload).await?,
            vec![manifest.overrides_prefix()],
        )),
    }
}

/// 这个整合包里有哪些文件要手动下载、找到了没有。mrpack 没有这回事。
pub async fn manual_files(archive_path: &Path, sideload: &Sideload) -> Result<Vec<ManualFile>> {
    let pack = read_pack(archive_path)?;
    if matches!(pack, Pack::Modrinth(_)) {
        return Ok(Vec::new());
    }
    let (files, _) = files_of(&pack, sideload).await?;
    Ok(files.iter().filter_map(PackFile::manual).collect())
}

/// 建一个实例并把整合包铺进去。
///
/// 名字可以由调用方给（用户在导入时改过），不给就用清单里的。
///
/// `upstream` 是补给站装的时候已经知道的上游身份；从本地文件导入时为空，那就拿
/// 包体的哈希去上游反查一次（见 [`crate::supply::identify_archive`]）。
pub async fn install(
    paths: &DataPaths,
    archive_path: &Path,
    name: Option<&str>,
    upstream: Option<PackRef>,
    sideload: &Sideload,
    job: &crate::Job,
) -> Result<InstanceProfile> {
    // 三步：读包、下文件、铺自带的配置。下载那一步之外的两步都没有字节数
    // 可报，但它们都要花时间——分步说，起码看得出卡在哪一段。
    job.expect(3);
    job.step("读取整合包");
    let pack = read_pack(archive_path)?;
    let summary = summarize(archive_path, &pack)?;
    if summary.loader != LoaderKind::Vanilla && !is_safe_version(&summary.loader_version) {
        return Err(anyhow!("整合包写的加载器版本无法使用"));
    }
    // 要下什么在建实例之前问清楚：缺一个手动下载的文件就不开工，留一个下了
    // 一半的实例比直接说缺什么更糟。
    let (files, prefixes) = files_of(&pack, sideload).await?;
    let missing = files
        .iter()
        .filter(|file| file.url.is_none() && file.local.is_none())
        .count();
    if missing > 0 {
        return Err(anyhow!(
            "还有 {missing} 个文件需要从 CurseForge 网站手动下载"
        ));
    }

    let profile = crate::create_instance_with_loader(
        paths,
        name.unwrap_or(&summary.name),
        &summary.game_version,
        summary.loader,
        (summary.loader != LoaderKind::Vanilla).then_some(summary.loader_version.as_str()),
    )?;
    let game = paths.game_directory(profile.id.as_str());

    // 出了岔子就把这个半成品实例删掉。留一个下了一半的实例在曲库里，比直接
    // 失败更糟——它看起来是好的，点启动才发现不是。
    match lay_out(&game, archive_path, &files, prefixes, job).await {
        Ok(overrides) => {
            // 记一笔谁放进来的。整合包自带的 overrides 不进来源日志：那里面绝大
            // 多数是配置，本来就天天变，记下来只会让日志淹掉真正的信号。它们的
            // 哈希记在整合包的记录里（下面），那是另一个问题要用的。
            //
            // 铺完了才读得到版本号。三百个 jar 各开一次 zip 读一个小文件，
            // 比刚才那趟下载便宜得多。
            let arrivals = files
                .iter()
                .filter_map(|file| {
                    let target = game.join(&file.path);
                    let sha1 = file
                        .sha1
                        .clone()
                        .or_else(|| crate::backup::sha1_of(&target).ok())?;
                    Some(crate::instance::origin::Entry {
                        file: file.path.clone(),
                        sha1,
                        version: crate::instance::integrity::declared_version(&target),
                        origin: crate::instance::origin::Origin::Modpack {
                            source: pack.source(),
                            name: summary.name.clone(),
                            version: summary.version.clone(),
                        },
                    })
                })
                .collect::<Vec<_>>();

            // 这个实例是哪个整合包的哪个版本、铺下来的是什么。以后判断有没有
            // 新版、哪些文件是包带来的，都从这份记录出发（见 `instance::pack`）。
            let upstream = match upstream {
                Some(known) => Some(known),
                None => crate::supply::identify_archive(pack.source(), archive_path).await,
            };
            let record = PackRecord {
                source: pack.source(),
                upstream,
                name: summary.name.clone(),
                version: summary.version.clone(),
                installed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs()),
                files: arrivals
                    .iter()
                    .map(|entry| (entry.file.clone(), entry.sha1.clone()))
                    .collect(),
                overrides,
            };
            if let Err(error) = crate::instance::pack::write(paths, profile.id.as_str(), &record) {
                let _ = paths.append_log(&format!(
                    "[modpack] instance={} record={error:#}",
                    profile.id.as_str()
                ));
            }

            crate::instance::origin::record(paths, profile.id.as_str(), arrivals);
            Ok(profile)
        }
        Err(error) => {
            let _ = crate::delete_instance(paths, profile.id.as_str());
            Err(error)
        }
    }
}

async fn lay_out(
    game: &Path,
    archive_path: &Path,
    files: &[PackFile],
    prefixes: Vec<String>,
    job: &crate::Job,
) -> Result<BTreeMap<String, String>> {
    let events = &job.downloads();
    tokio::fs::create_dir_all(game).await?;

    let mut tasks = Vec::new();
    for file in files {
        let destination = safe_join(game, Path::new(&file.path))
            .with_context(|| format!("整合包里的路径不安全：{}", file.path))?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // 手动下载的那一份已经按 sha1 认过了。
        if let Some(local) = &file.local {
            tokio::fs::copy(local, &destination)
                .await
                .with_context(|| format!("复制 {}", local.display()))?;
            continue;
        }
        let Some(url) = &file.url else {
            return Err(anyhow!("{} 没有可用的下载地址", file.path));
        };
        tasks.push(match file.sha1.as_deref() {
            Some(sha1) if file.size > 0 => DownloadTask::new(destination, url, sha1, file.size)?,
            _ => DownloadTask::unverified(destination, url)?,
        });
    }

    job.step(format!("下载整合包的 {} 个文件", tasks.len()));
    if !tasks.is_empty() {
        crate::data::downloader::client(8)
            .download_all(tasks, events)
            .await?;
    }

    job.step("展开整合包自带的文件");
    // 解压是同步的，而且大包的 overrides 有几十兆。摆在 async 函数里直接调，
    // 占住的是跑下载的那条运行时的一个 worker——交给阻塞线程池。
    let archive = archive_path.to_path_buf();
    let destination = game.to_path_buf();
    tokio::task::spawn_blocking(move || extract_overrides(&archive, &destination, &prefixes))
        .await?
}

/// 把整合包的包体取到手。
///
/// 和建实例分开：CurseForge 的整合包可能要先手动下几个文件——有时连包体本身
/// 都要——那要在建实例之前停下来问用户。装到一半失败时这个文件也还在缓存里，
/// 重来一次不用再下一遍。
pub async fn fetch(
    paths: &DataPaths,
    source: Source,
    version_id: &str,
    sideload: &Sideload,
    job: &crate::Job,
) -> Result<crate::supply::Fetched> {
    job.step("下载整合包");
    let cache = paths.root.join("cache/modpacks");
    crate::supply::fetch_release_file(source, version_id, &cache, sideload, &job.downloads()).await
}

/// 从补给站装一个整合包：取包体，再照本地文件那条路走。
pub async fn install_from_supply(
    paths: &DataPaths,
    source: Source,
    version_id: &str,
    name: Option<&str>,
    sideload: &Sideload,
    job: &crate::Job,
) -> Result<InstanceProfile> {
    // 取包体是 install 那三步之前的一步。
    job.expect(1);
    let fetched = fetch(paths, source, version_id, sideload, job).await?;
    let archive = fetched
        .path
        .ok_or_else(|| anyhow!("这个整合包的作者仅允许从 CurseForge 网站下载"))?;
    install(paths, &archive, name, Some(fetched.upstream), sideload, job).await
}

/// 把 overrides 铺进游戏目录，返回铺下去的每个文件的 sha1。
///
/// mrpack 有两层：`overrides/` 先铺，`client-overrides/` 后铺——后者按格式定义
/// 就是用来盖前者的，所以同一个路径记的是后铺的那一份。CurseForge 只有一层，
/// 目录名写在清单里。
///
/// 哈希边写边算：这些文件刚从 zip 里流过一遍，再读一遍磁盘是白读。
fn extract_overrides(
    archive_path: &Path,
    game: &Path,
    prefixes: &[String],
) -> Result<BTreeMap<String, String>> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut written = BTreeMap::new();

    for prefix in prefixes {
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            // 用 enclosed_name 而不是 name：它已经拒了绝对路径和 `..`，
            // 这里再走一次 safe_join 是第二道。
            let Some(entry_path) = entry.enclosed_name() else {
                continue;
            };
            let Ok(relative) = entry_path.strip_prefix(prefix) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            let destination: PathBuf = safe_join(game, relative)?;
            if entry.is_dir() {
                std::fs::create_dir_all(&destination)?;
                continue;
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let target = std::fs::File::create(&destination)
                .with_context(|| format!("写入 {}", destination.display()))?;
            let mut hashing = Hashing::new(target);
            std::io::copy(&mut entry, &mut hashing)?;
            written.insert(
                relative.to_string_lossy().replace('\\', "/"),
                hashing.finish(),
            );
        }
    }
    Ok(written)
}

/// 写的同时算 sha1。
struct Hashing<W> {
    inner: W,
    hasher: sha1::Sha1,
}

impl<W: std::io::Write> Hashing<W> {
    fn new(inner: W) -> Self {
        use sha1::Digest;
        Self {
            inner,
            hasher: sha1::Sha1::new(),
        }
    }

    fn finish(self) -> String {
        use sha1::Digest;
        format!("{:x}", self.hasher.finalize())
    }
}

impl<W: std::io::Write> std::io::Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use sha1::Digest;
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn pack(directory: &Path, index_json: &str, overrides: &[(&str, &str)]) -> PathBuf {
        std::fs::create_dir_all(directory).expect("create dir");
        let path = directory.join("pack.mrpack");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create"));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("modrinth.index.json", options)
            .expect("start");
        writer.write_all(index_json.as_bytes()).expect("write");
        for (name, body) in overrides {
            writer.start_file(*name, options).expect("start");
            writer.write_all(body.as_bytes()).expect("write");
        }
        writer.finish().expect("finish");
        path
    }

    fn root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fern-mrpack-{tag}-{}", std::process::id()))
    }

    #[test]
    fn reads_the_version_and_loader_out_of_the_index() {
        let directory = root("inspect");
        let path = pack(
            &directory,
            r#"{
              "formatVersion": 1,
              "name": "苦力怕乐园",
              "versionId": "1.2.0",
              "summary": "一句话",
              "files": [{"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"]}],
              "dependencies": {"minecraft":"1.20.1","fabric-loader":"0.15.11"}
            }"#,
            &[],
        );

        let summary = inspect(&path).expect("inspect");
        assert_eq!(summary.name, "苦力怕乐园");
        assert_eq!(summary.game_version, "1.20.1");
        assert_eq!(summary.loader, LoaderKind::Fabric);
        assert_eq!(summary.loader_version, "0.15.11");
        assert_eq!(summary.files, 1);

        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn a_curseforge_pack_is_recognised_by_its_manifest() {
        let directory = root("curseforge");
        std::fs::create_dir_all(&directory).expect("create dir");
        let path = directory.join("pack.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create"));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("manifest.json", options).expect("start");
        writer
            .write_all(
                br#"{"manifestType":"minecraftModpack","manifestVersion":1,
                     "name":"All the Mods","version":"1.0","author":"ATM",
                     "minecraft":{"version":"1.20.1",
                                  "modLoaders":[{"id":"forge-47.2.0","primary":true}]},
                     "files":[{"projectID":238222,"fileID":4712866,"required":true}],
                     "overrides":"overrides"}"#,
            )
            .expect("write");
        writer.finish().expect("finish");

        let summary = inspect(&path).expect("inspect");
        assert_eq!(summary.source, Source::CurseForge);
        assert_eq!(summary.name, "All the Mods");
        assert_eq!(summary.game_version, "1.20.1");
        assert_eq!(summary.loader, LoaderKind::Forge);
        assert_eq!(summary.loader_version, "47.2.0");
        assert_eq!(summary.files, 1);

        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn a_zip_with_neither_manifest_is_not_a_pack() {
        let directory = root("neither");
        std::fs::create_dir_all(&directory).expect("create dir");
        let other = directory.join("other.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&other).expect("create"));
        writer
            .start_file("readme.txt", zip::write::SimpleFileOptions::default())
            .expect("start");
        writer.write_all(b"hi").expect("write");
        writer.finish().expect("finish");
        let error = inspect(&other).expect_err("should refuse");
        assert!(format!("{error:#}").contains("不是整合包"));
        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn a_future_format_version_is_refused_rather_than_guessed_at() {
        let directory = root("format");
        let path = pack(
            &directory,
            r#"{"formatVersion": 2, "dependencies": {}}"#,
            &[],
        );
        let error = inspect(&path).expect_err("should refuse");
        assert!(format!("{error:#}").contains("格式版本"));
        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn overrides_cannot_escape_the_game_directory() {
        let directory = root("escape");
        let path = pack(
            &directory,
            r#"{"formatVersion":1,"dependencies":{"minecraft":"1.20.1"}}"#,
            &[
                ("overrides/config/ok.toml", "fine"),
                // zip 里塞一个爬出去的名字是整合包最经典的攻击面。
                ("overrides/../../escaped.txt", "nope"),
            ],
        );
        let game = directory.join("game");
        std::fs::create_dir_all(&game).expect("create game");

        let written = extract_overrides(
            &path,
            &game,
            &["overrides/".to_owned(), "client-overrides/".to_owned()],
        )
        .expect("extract");
        assert!(game.join("config/ok.toml").is_file());
        // 铺下去的那一刻的 sha1，留给以后分辨「包里的配置」和「用户改过的配置」。
        {
            use sha1::Digest;
            assert_eq!(
                written.get("config/ok.toml"),
                Some(&format!("{:x}", sha1::Sha1::digest(b"fine")))
            );
        }
        assert_eq!(written.len(), 1);
        assert!(!directory.join("escaped.txt").exists());

        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn server_only_files_are_not_downloaded() {
        let entry = |client: Option<&str>| IndexFile {
            path: "mods/a.jar".to_owned(),
            hashes: Hashes::default(),
            env: client.map(|client| Env {
                client: client.to_owned(),
            }),
            downloads: Vec::new(),
            file_size: 0,
        };
        // 整合包的 index 里常带一堆纯服务端的模组，下下来只是白占磁盘。
        assert!(!wanted(&entry(Some("unsupported"))));
        assert!(wanted(&entry(Some("required"))));
        assert!(wanted(&entry(Some("optional"))));
        // 没写 env 就是两边都要。
        assert!(wanted(&entry(None)));
    }

    /// 对着真的 Modrinth 装一个整合包，看记录是不是写对了。要联网，默认不跑：
    ///
    /// ```text
    /// cargo test -p fern-core --lib supply::modpack::tests::live -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn live_a_local_mrpack_is_recorded_with_its_upstream() {
        use crate::supply::{SearchQuery, Sideload};

        let root = std::env::temp_dir().join(format!("fern-pack-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.clone());

        let hits = crate::supply::search(
            Source::Modrinth,
            &SearchQuery {
                query: "fabulously optimized".to_owned(),
                kind: crate::supply::ResourceKind::Modpack,
                ..SearchQuery::default()
            },
        )
        .await
        .expect("search");
        let pack = &hits.hits[0];
        let list = crate::supply::versions(Source::Modrinth, &pack.project_id, None)
            .await
            .expect("versions");
        let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
        let job = crate::Job::begin(&events, "live", Vec::new());
        let fetched = fetch(
            &paths,
            Source::Modrinth,
            &list[0].id,
            &Sideload::default(),
            &job,
        )
        .await
        .expect("fetch");
        let archive = fetched.path.clone().expect("archive");

        // 当成本地文件导入：不给上游身份，要它自己按 sha1 认出来。
        let profile = install(&paths, &archive, None, None, &Sideload::default(), &job)
            .await
            .expect("install");
        job.done(None);

        let record = crate::instance::pack::read(&paths, profile.id.as_str()).expect("record");
        println!(
            "{} {} upstream={:?} files={} overrides={}",
            record.name,
            record.version,
            record.upstream,
            record.files.len(),
            record.overrides.len()
        );
        assert_eq!(record.upstream, Some(fetched.upstream));
        assert!(!record.files.is_empty());
        assert!(record.files.keys().all(|path| path.contains('/')));
        assert!(!record.overrides.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }
}
