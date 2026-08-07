//! Download and verification primitives for Fern.
//!
//! The downloader owns network and filesystem behavior while the UI receives
//! only serialized [`DownloadEvent`] values through the core boundary.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    sync::Mutex,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadEvent {
    Status { message: String },
    TaskStarted { total_files: u64, total_bytes: u64 },
    FileDone { path: String, bytes: u64 },
    Progress { done_bytes: u64, speed_bps: u64 },
    TaskFinished { failed: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    pub path: PathBuf,
    pub url: Url,
    pub sha1: String,
    pub size: u64,
}

impl DownloadTask {
    pub fn new(
        path: impl Into<PathBuf>,
        url: &str,
        sha1: impl Into<String>,
        size: u64,
    ) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            url: Url::parse(url).context("invalid download URL")?,
            sha1: sha1.into().to_ascii_lowercase(),
            size,
        })
    }
}

pub trait DownloadSource: Send + Sync {
    fn rewrite(&self, official: &Url) -> Url;
}

#[derive(Debug, Default)]
pub struct OfficialSource;

impl DownloadSource for OfficialSource {
    fn rewrite(&self, official: &Url) -> Url {
        official.clone()
    }
}

#[derive(Debug, Default)]
pub struct BmclapiSource;

impl DownloadSource for BmclapiSource {
    fn rewrite(&self, official: &Url) -> Url {
        let Some(host) = official.host_str() else {
            return official.clone();
        };
        let replacement = match host {
            "libraries.minecraft.net" => "bmclapi2.bangbang93.com",
            "piston-meta.mojang.com" => "bmclapi2.bangbang93.com",
            "launchermeta.mojang.com" => "bmclapi2.bangbang93.com",
            "piston-data.mojang.com" => "bmclapi2.bangbang93.com",
            "resources.download.minecraft.net" => "bmclapi2.bangbang93.com",
            _ => return official.clone(),
        };
        let mut rewritten = official.clone();
        let _ = rewritten.set_host(Some(replacement));
        if host == "libraries.minecraft.net" {
            rewritten.set_path(&format!("/maven{}", official.path()));
        } else if host == "resources.download.minecraft.net" {
            rewritten.set_path(&format!("/assets{}", official.path()));
        }
        rewritten
    }
}

pub fn sha1_matches(bytes: &[u8], expected: &str) -> bool {
    let digest = Sha1::digest(bytes);
    let actual = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    actual.eq_ignore_ascii_case(expected)
}

pub async fn verify_file(path: &Path, expected_sha1: &str, expected_size: u64) -> Result<bool> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(bytes.len() as u64 == expected_size && sha1_matches(&bytes, expected_sha1))
}

pub fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "artifact path escapes data root: {}",
            relative.display()
        ));
    }
    Ok(root.join(relative))
}

/// 单个源试几次。一次网络抖动不该让整轮补全失败——实测 300 个文件里
/// 掉 2 个，重跑一遍就好了，那本来就不该报到用户面前。
const ATTEMPTS_PER_SOURCE: u32 = 3;

/// 超过这个大小才值得断点续传。资源文件普遍几 KB，为它们多读一次磁盘、
/// 多发一个 Range 头是净亏损；client jar 和 Java 运行时才是会断在半路的那些。
const RESUME_THRESHOLD: u64 = 4 * 1024 * 1024;

/// 进度事件的最小间隔。不限流的话每个 chunk 一条，几百个并发文件能把 IPC
/// 打满，而界面上一秒钟刷十次和刷一百次看起来完全一样。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

/// 按域名记的近期成败，用来决定下次先试谁（文档 §2.4）。
///
/// 不做启动时测速：那要在用户还没提出任何请求的时候先打一轮网络，测出来的
/// 数还未必是真正下载时的表现。真实的成败就在手边，用它就够了。
#[derive(Debug, Default)]
struct SourceHealth {
    hosts: Mutex<HashMap<String, HostStats>>,
}

#[derive(Debug, Default, Clone, Copy)]
struct HostStats {
    successes: u32,
    failures: u32,
}

impl SourceHealth {
    fn record(&self, host: &str, ok: bool) {
        let Ok(mut hosts) = self.hosts.lock() else {
            return;
        };
        let stats = hosts.entry(host.to_owned()).or_default();
        if ok {
            stats.successes = stats.successes.saturating_add(1);
        } else {
            stats.failures = stats.failures.saturating_add(1);
        }
        // 只看近期：计数长到一定程度就整体减半，早年的一次失败不该一直压着
        // 一个源。
        if stats.successes + stats.failures > 64 {
            stats.successes /= 2;
            stats.failures /= 2;
        }
    }

    /// 失败率分档，0（好）到 4（基本连不上）。没有样本的算 0，新源先得到一次机会。
    ///
    /// 分档而不是直接用比率：偶尔掉一个文件的源和用户在设置里选的源，不该
    /// 因为 2% 的差距就被换掉位置。只有明显更差（失败率上到四分之一）才动。
    fn demerit(&self, host: &str) -> u32 {
        let Ok(hosts) = self.hosts.lock() else {
            return 0;
        };
        let Some(stats) = hosts.get(host) else {
            return 0;
        };
        let total = stats.successes + stats.failures;
        if total == 0 {
            return 0;
        }
        (stats.failures * 1000 / total) / 250
    }
}

#[derive(Clone)]
pub struct DownloadClient {
    client: reqwest::Client,
    sources: Vec<Arc<dyn DownloadSource>>,
    semaphore: Arc<Semaphore>,
    health: Arc<SourceHealth>,
}

impl DownloadClient {
    pub fn new(sources: Vec<Arc<dyn DownloadSource>>, concurrency: usize) -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(45))
                .build()
                .expect("valid download client configuration"),
            sources: if sources.is_empty() {
                vec![Arc::new(OfficialSource)]
            } else {
                sources
            },
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            health: Arc::new(SourceHealth::default()),
        }
    }

    /// 配置的顺序打底，最近老失败的往后挪。稳定排序，所以健康度相同的源
    /// 仍然按用户在设置里选的顺序来。
    fn ordered_sources(&self, url: &Url) -> Vec<Arc<dyn DownloadSource>> {
        let mut sources = self.sources.clone();
        sources.sort_by_key(|source| {
            source
                .rewrite(url)
                .host_str()
                .map(|host| self.health.demerit(host))
                .unwrap_or(0)
        });
        sources
    }

    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        let url = Url::parse(url).context("invalid download URL")?;
        let mut last_error = None;
        for source in self.ordered_sources(&url) {
            let rewritten = source.rewrite(&url);
            let host = rewritten.host_str().unwrap_or_default().to_owned();
            for attempt in 0..ATTEMPTS_PER_SOURCE {
                if attempt > 0 {
                    tokio::time::sleep(backoff(attempt)).await;
                }
                match self.client.get(rewritten.clone()).send().await {
                    Ok(response) if response.status().is_success() => {
                        match response.bytes().await {
                            Ok(bytes) => {
                                self.health.record(&host, true);
                                return Ok(bytes.to_vec());
                            }
                            Err(error) => last_error = Some(error.into()),
                        }
                    }
                    // 404 换个源重试还是 404，别在同一堵墙上撞三次。
                    Ok(response) if response.status().is_client_error() => {
                        self.health.record(&host, false);
                        last_error =
                            Some(anyhow!("download failed with HTTP {}", response.status()));
                        break;
                    }
                    Ok(response) => {
                        last_error =
                            Some(anyhow!("download failed with HTTP {}", response.status()))
                    }
                    Err(error) => last_error = Some(error.into()),
                }
                self.health.record(&host, false);
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("no download source configured")))
    }

    pub async fn download(
        &self,
        task: &DownloadTask,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        self.download_all(vec![task.clone()], events).await
    }

    pub async fn download_all(
        &self,
        tasks: Vec<DownloadTask>,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        let total_bytes = tasks.iter().map(|task| task.size).sum();
        let _ = events.send(DownloadEvent::TaskStarted {
            total_files: tasks.len() as u64,
            total_bytes,
        });
        let started = Instant::now();
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let last_emit = Arc::new(AtomicU64::new(0));
        let mut jobs = tokio::task::JoinSet::new();
        for task in tasks {
            let client = self.clone();
            let events = events.clone();
            let downloaded_bytes = downloaded_bytes.clone();
            let last_emit = last_emit.clone();
            jobs.spawn(async move {
                let result = client
                    .download_one(&task, &downloaded_bytes, started, &last_emit, &events)
                    .await;
                (task, result)
            });
        }

        let mut failed = Vec::new();
        while let Some(joined) = jobs.join_next().await {
            match joined {
                Ok((task, Ok(()))) => {
                    let _ = events.send(DownloadEvent::FileDone {
                        path: task.path.display().to_string(),
                        bytes: task.size,
                    });
                }
                Ok((task, Err(_))) => failed.push(task.path.display().to_string()),
                Err(error) => failed.push(format!("download worker: {error}")),
            }
        }
        let _ = events.send(DownloadEvent::TaskFinished {
            failed: failed.clone(),
        });
        if failed.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("{} files failed to download", failed.len()))
        }
    }

    async fn download_one(
        &self,
        task: &DownloadTask,
        downloaded_bytes: &AtomicU64,
        started: Instant,
        last_emit: &AtomicU64,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .context("download semaphore closed")?;

        // 校验通过即跳过，所以补全天然幂等：「修复文件」就是同一个入口再跑
        // 一遍，不需要单独的一套代码。
        if verify_file(&task.path, &task.sha1, task.size).await? {
            downloaded_bytes.fetch_add(task.size, Ordering::Relaxed);
            emit_progress(downloaded_bytes, started, last_emit, events, true);
            return Ok(());
        }

        if let Some(parent) = task.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let temporary = task.path.with_extension("part");
        let mut last_error = None;
        for source in self.ordered_sources(&task.url) {
            let url = source.rewrite(&task.url);
            let host = url.host_str().unwrap_or_default().to_owned();
            for attempt in 0..ATTEMPTS_PER_SOURCE {
                if attempt > 0 {
                    tokio::time::sleep(backoff(attempt)).await;
                }
                match self
                    .attempt_download(
                        task,
                        &url,
                        &temporary,
                        downloaded_bytes,
                        started,
                        last_emit,
                        events,
                    )
                    .await
                {
                    Ok(true) => {
                        self.health.record(&host, true);
                        return Ok(());
                    }
                    Ok(false) => {
                        // 服务器明确说没有这个文件，换个源，别在同一堵墙上撞三次。
                        self.health.record(&host, false);
                        last_error = Some(anyhow!("{url} 上没有这个文件"));
                        break;
                    }
                    Err(error) => {
                        self.health.record(&host, false);
                        last_error = Some(error);
                    }
                }
            }
        }
        let _ = tokio::fs::remove_file(&temporary).await;
        Err(last_error.unwrap_or_else(|| anyhow!("no download source configured")))
    }

    /// 试一次。`Ok(false)` 表示这个源上没有（4xx），换源；`Err` 表示值得再试。
    #[allow(clippy::too_many_arguments)]
    async fn attempt_download(
        &self,
        task: &DownloadTask,
        url: &Url,
        temporary: &Path,
        downloaded_bytes: &AtomicU64,
        started: Instant,
        last_emit: &AtomicU64,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<bool> {
        // 断点续传：大文件断在半路时，已经落盘的那几十兆没有理由重下。
        // sha1 是整个文件的，所以续传时要先把已有的字节喂进 hasher。
        let resume_from = if task.size >= RESUME_THRESHOLD {
            match tokio::fs::metadata(temporary).await {
                Ok(metadata) if metadata.len() > 0 && metadata.len() < task.size => metadata.len(),
                _ => 0,
            }
        } else {
            0
        };

        let mut request = self.client.get(url.clone());
        if resume_from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
        }
        let mut response = request.send().await?;
        if response.status().is_client_error() {
            return Ok(false);
        }
        if !response.status().is_success() {
            return Err(anyhow!("download failed with HTTP {}", response.status()));
        }

        // 请求了 Range 却收到 200，说明服务器不支持，那就当作从头下。
        let resuming = resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let mut hasher = Sha1::new();
        let mut file = if resuming {
            hasher.update(&tokio::fs::read(temporary).await?);
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(temporary)
                .await?
        } else {
            tokio::fs::File::create(temporary).await?
        };

        // 这一次尝试往全局进度里加了多少——失败要原样退回去，否则进度条会
        // 越走越多，最后停在一个大于总量的数上。
        let mut counted = if resuming { resume_from } else { 0 };
        downloaded_bytes.fetch_add(counted, Ordering::Relaxed);
        let mut received = if resuming { resume_from } else { 0 };

        let outcome = loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    hasher.update(&chunk);
                    received = received.saturating_add(chunk.len() as u64);
                    counted = counted.saturating_add(chunk.len() as u64);
                    downloaded_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    emit_progress(downloaded_bytes, started, last_emit, events, false);
                }
                Ok(None) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        file.flush().await?;

        if let Err(error) = outcome {
            downloaded_bytes.fetch_sub(counted, Ordering::Relaxed);
            return Err(error.into());
        }

        let actual = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if received == task.size && actual.eq_ignore_ascii_case(&task.sha1) {
            if tokio::fs::try_exists(&task.path).await? {
                tokio::fs::remove_file(&task.path).await?;
            }
            tokio::fs::rename(temporary, &task.path).await?;
            emit_progress(downloaded_bytes, started, last_emit, events, true);
            return Ok(true);
        }

        downloaded_bytes.fetch_sub(counted, Ordering::Relaxed);
        // 校验不过说明落盘的那份是脏的，续传只会一直错下去。
        let _ = tokio::fs::remove_file(temporary).await;
        Err(anyhow!("checksum or size mismatch for {}", task.url))
    }
}

/// 限流后的进度事件。`force` 用在文件收尾这种「这一下必须看得见」的时刻。
fn emit_progress(
    downloaded_bytes: &AtomicU64,
    started: Instant,
    last_emit: &AtomicU64,
    events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    force: bool,
) {
    let elapsed = started.elapsed();
    let now = elapsed.as_millis() as u64;
    if !force {
        let previous = last_emit.load(Ordering::Relaxed);
        if now.saturating_sub(previous) < PROGRESS_INTERVAL.as_millis() as u64 {
            return;
        }
        // 输的那些线程这一轮就不发了，不必重试——下一个 chunk 马上还会来。
        if last_emit
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
    } else {
        last_emit.store(now, Ordering::Relaxed);
    }

    let done_bytes = downloaded_bytes.load(Ordering::Relaxed);
    let speed_bps = (done_bytes as f64 / elapsed.as_secs_f64().max(0.001)) as u64;
    let _ = events.send(DownloadEvent::Progress {
        done_bytes,
        speed_bps,
    });
}

/// 退避：200ms、400ms、800ms。抖动通常一两百毫秒就过去了，等太久不如换源。
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(200u64 << attempt.min(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verifies_content_by_size_and_sha1() {
        let root = std::env::temp_dir().join(format!("fern-download-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create test directory");
        let path = root.join("client.jar");
        tokio::fs::write(&path, b"fern")
            .await
            .expect("write fixture");
        assert!(
            verify_file(&path, "654edb122a04602f918500d59b1d6fc37b9d0c01", 4)
                .await
                .expect("verify fixture")
        );
        assert!(
            !verify_file(&path, "0000000000000000000000000000000000000000", 4)
                .await
                .expect("verify mismatch")
        );
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove test directory");
    }

    #[test]
    fn rejects_paths_that_escape_the_data_root() {
        assert!(safe_join(Path::new("/fern"), Path::new("assets/client.jar")).is_ok());
        assert!(safe_join(Path::new("/fern"), Path::new("../outside")).is_err());
        assert!(safe_join(Path::new("/fern"), Path::new("/outside")).is_err());
    }

    #[test]
    fn unhealthy_hosts_sink_to_the_back_of_the_source_list() {
        let client =
            DownloadClient::new(vec![Arc::new(OfficialSource), Arc::new(BmclapiSource)], 4);
        let url = Url::parse("https://libraries.minecraft.net/com/mojang/test.jar").unwrap();

        // 没有样本时保持配置顺序：新源先得到一次机会。
        let order = client.ordered_sources(&url);
        assert_eq!(
            order[0].rewrite(&url).host_str(),
            Some("libraries.minecraft.net")
        );

        for _ in 0..5 {
            client.health.record("libraries.minecraft.net", false);
        }
        client.health.record("bmclapi2.bangbang93.com", true);
        let order = client.ordered_sources(&url);
        assert_eq!(
            order[0].rewrite(&url).host_str(),
            Some("bmclapi2.bangbang93.com")
        );

        // 恢复之后要能回来，否则一次网络抖动会永久改掉用户选的源。
        for _ in 0..40 {
            client.health.record("libraries.minecraft.net", true);
        }
        let order = client.ordered_sources(&url);
        assert_eq!(
            order[0].rewrite(&url).host_str(),
            Some("libraries.minecraft.net")
        );
    }

    #[test]
    fn backoff_grows_but_stays_short_enough_to_be_worth_waiting() {
        assert_eq!(backoff(1), Duration::from_millis(400));
        assert_eq!(backoff(2), Duration::from_millis(800));
        assert!(backoff(9) <= Duration::from_millis(3200));
    }

    #[test]
    fn progress_events_are_throttled_but_never_swallow_a_completion() {
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let done = AtomicU64::new(1024);
        let last_emit = AtomicU64::new(0);
        // 让「已经跑了一会儿」成立，否则首次调用的 now 和初始值都是 0，
        // 分不出「还没发过」和「刚发过」。
        let started = Instant::now() - Duration::from_millis(500);

        emit_progress(&done, started, &last_emit, &events, false);
        emit_progress(&done, started, &last_emit, &events, false);
        emit_progress(&done, started, &last_emit, &events, true);
        drop(events);

        let mut count = 0;
        while received.try_recv().is_ok() {
            count += 1;
        }
        // 两条限流掉一条，加上强制的那一条。
        assert_eq!(count, 2);
    }

    #[test]
    fn rewrites_known_mirrors() {
        let source = BmclapiSource;
        let official = Url::parse("https://libraries.minecraft.net/com/mojang/test.jar").unwrap();
        let mirror = source.rewrite(&official);
        assert_eq!(mirror.host_str(), Some("bmclapi2.bangbang93.com"));
        assert_eq!(mirror.path(), "/maven/com/mojang/test.jar");
    }
}
