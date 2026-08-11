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
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DownloadEvent {
    /// 一句自由文本的细节。措辞该归界面管，新代码用 `StatusId`；这条留给
    /// 还没搬迁的调用点。
    Status {
        message: String,
    },
    /// 一条文案 id 加参数。下载器只是传输，句子在界面的文案表里。
    StatusId {
        id: String,
        params: Vec<(String, String)>,
    },
    /// 整批重来。上一版是一句拼好的中文，句子搬去了文案表。
    Retrying {
        files: u64,
    },
    TaskStarted {
        total_files: u64,
        total_bytes: u64,
    },
    FileDone {
        path: String,
        bytes: u64,
    },
    /// `total_bytes` 会在批次进行中变大：没有已知大小的文件（第三方 Maven）
    /// 边下边把实际字节同时计入分子和分母，所以「已下载」永远不会超过它。
    Progress {
        done_bytes: u64,
        total_bytes: u64,
        speed_bps: u64,
    },
    TaskFinished {
        failed: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    pub path: PathBuf,
    pub url: Url,
    /// 官方元数据每个文件都有 sha1，所以这是常态。第三方 Maven（Fabric、
    /// Forge 的库）只给一个 URL，那时候只能认「下下来了」。
    pub sha1: Option<String>,
    pub size: Option<u64>,
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
            sha1: Some(sha1.into().to_ascii_lowercase()),
            size: Some(size),
        })
    }

    /// 没有校验和的文件。
    ///
    /// 这是退让，不是常态：拿不到 sha1 就没法判断磁盘上那份是不是完整的，
    /// 「校验文件」对它只能退化成「在不在」。所以只在元数据确实不给的时候用。
    pub fn unverified(path: impl Into<PathBuf>, url: &str) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            url: Url::parse(url).context("invalid download URL")?,
            sha1: None,
            size: None,
        })
    }

    /// 这一份已经落盘的能不能算数。
    pub async fn is_satisfied(&self) -> Result<bool> {
        match (&self.sha1, self.size) {
            (Some(sha1), Some(size)) => verify_file(&self.path, sha1, size).await,
            _ => Ok(tokio::fs::try_exists(&self.path).await?),
        }
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
            "meta.fabricmc.net" => "bmclapi2.bangbang93.com",
            "maven.fabricmc.net" => "bmclapi2.bangbang93.com",
            _ => return official.clone(),
        };
        let mut rewritten = official.clone();
        let _ = rewritten.set_host(Some(replacement));
        // 各条线在镜像上挂在不同的前缀下。
        match host {
            "libraries.minecraft.net" => rewritten.set_path(&format!("/maven{}", official.path())),
            "resources.download.minecraft.net" => {
                rewritten.set_path(&format!("/assets{}", official.path()))
            }
            "meta.fabricmc.net" => rewritten.set_path(&format!("/fabric-meta{}", official.path())),
            "maven.fabricmc.net" => rewritten.set_path(&format!("/maven{}", official.path())),
            _ => {}
        }
        rewritten
    }
}

/// A file's SHA-1 as lowercase hex.
///
/// Content-addressed services key on this exact spelling, so the formatting
/// belongs next to the hashing rather than at every call site.
pub fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn sha1_matches(bytes: &[u8], expected: &str) -> bool {
    sha1_hex(bytes).eq_ignore_ascii_case(expected)
}

pub async fn verify_file(path: &Path, expected_sha1: &str, expected_size: u64) -> Result<bool> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(bytes.len() as u64 == expected_size && sha1_matches(&bytes, expected_sha1))
}

/// 下到一半时先落在哪。
///
/// **追加后缀，不是替换扩展名。** `with_extension("part")` 会把
/// `bin/java.exe` 和 `bin/java.dll` 都写成 `bin/java.part`——同一批里两个任务
/// 抢同一个临时文件，各写各的字节，谁先 rename 谁把对方的半份内容搬进自己的
/// 目的地。Mojang 的运行时清单里这样的同名兄弟每个平台都有（Windows 上正是
/// `java.exe`/`java.dll`），结果就是一句「java.exe 不是有效的 Win32 应用程序」。
///
/// 而校验拦不住它：sha1 是就着下载流现算的，不是回头读磁盘上那份，所以两个
/// 任务都「校验通过」。名字唯一是唯一的出路。
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.to_path_buf().into_os_string();
    name.push(".part");
    PathBuf::from(name)
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

/// 整批重来几轮。
///
/// 单文件的重试解决不了成片的失败：网线被拔掉十秒、Wi-Fi 切换、对端限流，
/// 这类事件会在同一时刻打掉几十个文件，而它们各自的三次尝试都发生在那十秒
/// 之内。「重跑一遍就好了」既然是真的，就该由我们跑，而不是报一句「12 个
/// 文件下载失败」让用户自己再点一次。
const BATCH_ROUNDS: u32 = 3;

/// 超过这个大小才值得断点续传。资源文件普遍几 KB，为它们多读一次磁盘、
/// 多发一个 Range 头是净亏损；client jar 和 Java 运行时才是会断在半路的那些。
const RESUME_THRESHOLD: u64 = 4 * 1024 * 1024;

/// 进度事件的最小间隔。不限流的话每个 chunk 一条，几百个并发文件能把 IPC
/// 打满，而界面上一秒钟刷十次和刷一百次看起来完全一样。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

/// 一个批次的进度账本。
///
/// `total` 不是常量：开工时先记上所有已知大小之和，没有已知大小的文件
/// （第三方 Maven 的库）在下载中把实际字节**同时**计入 `done` 和 `total`。
/// 这条纪律保证「已下载 ≤ 总量」在任何时刻成立——上一版把未知大小的文件排除
/// 在分母外、字节却照计进分子，批次一结束分子必然超过分母。
struct BatchProgress {
    done: AtomicU64,
    total: AtomicU64,
    started: Instant,
    last_emit_ms: AtomicU64,
    /// 上次发事件时的 `done`，用来算窗口速度。
    last_done: AtomicU64,
    /// 平滑后的速度。全程平均值不行：批次开头一批本地命中会把速度顶到
    /// 几 GB/s，之后又一路虚高。
    speed: AtomicU64,
}

impl BatchProgress {
    fn new(known_total: u64) -> Self {
        Self {
            done: AtomicU64::new(0),
            total: AtomicU64::new(known_total),
            started: Instant::now(),
            last_emit_ms: AtomicU64::new(0),
            last_done: AtomicU64::new(0),
            speed: AtomicU64::new(0),
        }
    }

    /// 限流后的进度事件。`force` 用在文件收尾这种「这一下必须看得见」的时刻。
    fn emit(&self, events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>, force: bool) {
        let now = self.started.elapsed().as_millis() as u64;
        let previous = self.last_emit_ms.load(Ordering::Relaxed);
        if !force {
            if now.saturating_sub(previous) < PROGRESS_INTERVAL.as_millis() as u64 {
                return;
            }
            // 输的那些线程这一轮就不发了，不必重试——下一个 chunk 马上还会来。
            if self
                .last_emit_ms
                .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
            {
                return;
            }
        } else {
            self.last_emit_ms.store(now, Ordering::Relaxed);
        }

        let done = self.done.load(Ordering::Relaxed);
        let elapsed = now.saturating_sub(previous);
        // 回退（重试）会让 done 变小，那不是速度，跳过这一窗。窗口太窄时
        // 算出来的数抖得没法看，也跳过。
        let last = self.last_done.swap(done, Ordering::Relaxed);
        if done >= last && elapsed >= PROGRESS_INTERVAL.as_millis() as u64 {
            let instant = (done - last).saturating_mul(1000) / elapsed.max(1);
            let smoothed = |previous: u64| {
                if previous == 0 {
                    instant
                } else {
                    (previous.saturating_mul(7) + instant.saturating_mul(3)) / 10
                }
            };
            let previous_speed = self.speed.load(Ordering::Relaxed);
            self.speed
                .store(smoothed(previous_speed), Ordering::Relaxed);
        }

        let _ = events.send(DownloadEvent::Progress {
            done_bytes: done,
            total_bytes: self.total.load(Ordering::Relaxed),
            speed_bps: self.speed.load(Ordering::Relaxed),
        });
    }
}

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

/// 一个文件没下下来，以及再试一次有没有意义。
struct Refusal {
    reason: String,
    /// 上游明确说没有这个文件时是 `false`：重试多少次还是没有。
    retryable: bool,
}

impl Refusal {
    fn retryable(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            retryable: true,
        }
    }

    fn fatal(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            retryable: false,
        }
    }
}

/// 一轮跑完之后没成的那一个。
struct Failure {
    task: DownloadTask,
    reason: String,
    retryable: bool,
}

/// 失败要说清楚是哪些文件、为什么。
///
/// 上一版只报一个数（「12 个文件下载失败」）。那句话对用户和对排障的人都没有
/// 用：既不知道缺的是资源还是库，也不知道是断网、404 还是校验不过——而这三种
/// 的处理方式完全不同。
fn describe_failures(failures: &[Failure]) -> String {
    /// 列几个。全列出来的话，一次断网能刷几百行。
    const SHOWN: usize = 3;

    let mut lines = vec![format!("{} 个文件没有下载成功", failures.len())];
    for failure in failures.iter().take(SHOWN) {
        let name = failure
            .task
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| failure.task.path.display().to_string());
        lines.push(format!("{name}（{}）", failure.reason));
    }
    if failures.len() > SHOWN {
        lines.push(format!("另有 {} 个", failures.len() - SHOWN));
    }
    lines.join("；")
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
                // 读超时，不是总超时。总超时是按「一次请求最多花多久」设的，
                // 而这里最大的两个文件是 client jar 和 Java 运行时——两百多兆
                // 在一条普通的家用带宽上本来就要跑几分钟。之前设的 45 秒总
                // 超时会把它们**每一次**都掐死在半路，表现出来正是「有几个
                // 文件总是失败」。真正该管的是「卡住不动」，那是读超时。
                .read_timeout(std::time::Duration::from_secs(30))
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
        self.fetch_checked(url, None, None).await
    }

    /// Fetch a metadata blob and validate it before accepting a source.
    ///
    /// Some mirrors return semantically equivalent JSON with different
    /// serialization, so a successful HTTP response still needs to pass the
    /// publisher's checksum before that source is considered usable.
    pub async fn fetch_verified(
        &self,
        url: &str,
        expected_sha1: Option<&str>,
        expected_size: Option<u64>,
    ) -> Result<Vec<u8>> {
        self.fetch_checked(url, expected_sha1, expected_size).await
    }

    async fn fetch_checked(
        &self,
        url: &str,
        expected_sha1: Option<&str>,
        expected_size: Option<u64>,
    ) -> Result<Vec<u8>> {
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
                                let bytes = bytes.to_vec();
                                let size_ok =
                                    expected_size.is_none_or(|size| bytes.len() as u64 == size);
                                let sha1_ok =
                                    expected_sha1.is_none_or(|sha1| sha1_matches(&bytes, sha1));
                                if size_ok && sha1_ok {
                                    self.health.record(&host, true);
                                    return Ok(bytes);
                                }
                                self.health.record(&host, false);
                                last_error =
                                    Some(anyhow!("checksum or size mismatch for {}", rewritten));
                                // A deterministic mismatch means this source's representation
                                // differs from the publisher's bytes; retry the next source.
                                break;
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
        // 分母先记已知的部分。没有已知大小的任务在下载中把实际字节同时计入
        // 分子和分母（见 BatchProgress），所以这个数只会往上长。
        let known_total = tasks.iter().filter_map(|task| task.size).sum();
        let _ = events.send(DownloadEvent::TaskStarted {
            total_files: tasks.len() as u64,
            total_bytes: known_total,
        });
        let progress = Arc::new(BatchProgress::new(known_total));

        let mut pending = tasks;
        let mut failures = Vec::new();
        for round in 0..BATCH_ROUNDS {
            if round > 0 {
                // 说出来。上一版这一段是完全静默的，用户看到的是进度条卡了
                // 几秒然后蹦出一句「12 个文件下载失败」。
                let _ = events.send(DownloadEvent::Retrying {
                    files: pending.len() as u64,
                });
                // 成片的失败多半来自一次短暂的断网，等一下比立刻重试有用。
                tokio::time::sleep(Duration::from_millis(800u64 << round.min(3))).await;
            }
            failures = self.run_round(pending, &progress, events).await;
            // 上游明确说没有的东西，重试多少次都还是没有。
            if failures.is_empty() || failures.iter().all(|failure| !failure.retryable) {
                break;
            }
            pending = failures
                .iter()
                .filter(|failure| failure.retryable)
                .map(|failure| failure.task.clone())
                .collect();
        }

        let _ = events.send(DownloadEvent::TaskFinished {
            failed: failures
                .iter()
                .map(|failure| failure.task.path.display().to_string())
                .collect(),
        });
        if failures.is_empty() {
            return Ok(());
        }
        Err(anyhow!("{}", describe_failures(&failures)))
    }

    /// 跑一轮，返回没成的那些。
    async fn run_round(
        &self,
        tasks: Vec<DownloadTask>,
        progress: &Arc<BatchProgress>,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Vec<Failure> {
        let mut jobs = tokio::task::JoinSet::new();
        for task in tasks {
            let client = self.clone();
            let events = events.clone();
            let progress = progress.clone();
            jobs.spawn(async move {
                let result = client.download_one(&task, &progress, &events).await;
                (task, result)
            });
        }

        let mut failures = Vec::new();
        while let Some(joined) = jobs.join_next().await {
            match joined {
                Ok((task, Ok(()))) => {
                    let _ = events.send(DownloadEvent::FileDone {
                        path: task.path.display().to_string(),
                        bytes: task.size.unwrap_or(0),
                    });
                }
                Ok((task, Err(error))) => failures.push(Failure {
                    task,
                    retryable: error.retryable,
                    reason: error.reason,
                }),
                // 任务自己没了（panic 或被取消）。它下的那个文件是哪个已经
                // 无从知道，但这件事必须留痕，不能当成功。
                Err(error) => failures.push(Failure {
                    task: DownloadTask {
                        path: PathBuf::from("<未完成的下载任务>"),
                        url: Url::parse("about:blank").expect("valid placeholder url"),
                        sha1: None,
                        size: None,
                    },
                    retryable: false,
                    reason: format!("下载任务异常结束：{error}"),
                }),
            }
        }
        failures
    }

    async fn download_one(
        &self,
        task: &DownloadTask,
        progress: &BatchProgress,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> std::result::Result<(), Refusal> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|error| Refusal::fatal(format!("下载信号量已关闭：{error}")))?;

        // 校验通过即跳过，所以补全天然幂等：「修复文件」就是同一个入口再跑
        // 一遍，不需要单独的一套代码。
        match task.is_satisfied().await {
            Ok(true) => {
                // 大小未知的文件对分母的贡献是 0（它没被计入过），对分子也
                // 只能是 0——两边都不动，账才是平的。
                progress
                    .done
                    .fetch_add(task.size.unwrap_or(0), Ordering::Relaxed);
                progress.emit(events, true);
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => return Err(Refusal::retryable(format!("读取已有文件失败：{error}"))),
        }

        if let Some(parent) = task.path.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            return Err(Refusal::retryable(format!(
                "创建目录 {} 失败：{error}",
                parent.display()
            )));
        }
        let temporary = part_path(&task.path);
        let mut last_error = None;
        // 每个源都明确回答「我这里没有」时，重试整批也不会有别的结果。
        let mut all_missing = true;
        for source in self.ordered_sources(&task.url) {
            let url = source.rewrite(&task.url);
            let host = url.host_str().unwrap_or_default().to_owned();
            for attempt in 0..ATTEMPTS_PER_SOURCE {
                if attempt > 0 {
                    tokio::time::sleep(backoff(attempt)).await;
                }
                match self
                    .attempt_download(task, &url, &temporary, progress, events)
                    .await
                {
                    Ok(true) => {
                        self.health.record(&host, true);
                        return Ok(());
                    }
                    Ok(false) => {
                        // 服务器明确说没有这个文件，换个源，别在同一堵墙上撞三次。
                        self.health.record(&host, false);
                        last_error = Some(format!("{host} 上没有这个文件"));
                        break;
                    }
                    Err(error) => {
                        self.health.record(&host, false);
                        all_missing = false;
                        last_error = Some(format!("{host}：{error}"));
                    }
                }
            }
        }
        let _ = tokio::fs::remove_file(&temporary).await;
        let reason = last_error.unwrap_or_else(|| "没有可用的下载源".to_owned());
        if all_missing {
            Err(Refusal::fatal(reason))
        } else {
            Err(Refusal::retryable(reason))
        }
    }

    /// 试一次。`Ok(false)` 表示这个源上没有（4xx），换源；`Err` 表示值得再试。
    ///
    /// 账目纪律：这一次尝试往进度里加过的每一个字节，只要文件最终没落成，
    /// 就必须原样退回去。上一版只在两条失败路径上退（流断、校验不过），写盘
    /// 失败、rename 失败那些 `?` 直接带着账跑了——重试一次，同一批字节就被
    /// 计了两遍，进度条最后停在一个大于总量的数上。所以这里把整个过程包在
    /// 一个内层里，**任何**失败都走同一个退账出口。
    async fn attempt_download(
        &self,
        task: &DownloadTask,
        url: &Url,
        temporary: &Path,
        progress: &BatchProgress,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<bool> {
        // 这一次尝试往账本里计了多少。大小未知的文件同时计进分母，退账时
        // 也要从分母退——见 BatchProgress。
        let mut counted = 0u64;
        let unsized_task = task.size.is_none();
        let outcome = self
            .attempt_inner(task, url, temporary, progress, events, &mut counted)
            .await;
        if !matches!(outcome, Ok(true)) {
            progress.done.fetch_sub(counted, Ordering::Relaxed);
            if unsized_task {
                progress.total.fetch_sub(counted, Ordering::Relaxed);
            }
        }
        outcome
    }

    async fn attempt_inner(
        &self,
        task: &DownloadTask,
        url: &Url,
        temporary: &Path,
        progress: &BatchProgress,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
        counted: &mut u64,
    ) -> Result<bool> {
        let unsized_task = task.size.is_none();
        // 断点续传：大文件断在半路时，已经落盘的那几十兆没有理由重下。
        // sha1 是整个文件的，所以续传时要先把已有的字节喂进 hasher。
        // 不知道总大小就没法判断落盘的那截是「断了一半」还是「已经下完」，
        // 续传的前提不成立，老实从头下。
        let resume_from = match task.size {
            Some(size) if size >= RESUME_THRESHOLD => match tokio::fs::metadata(temporary).await {
                Ok(metadata) if metadata.len() > 0 && metadata.len() < size => metadata.len(),
                _ => 0,
            },
            _ => 0,
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

        if resuming {
            *counted = resume_from;
            progress.done.fetch_add(resume_from, Ordering::Relaxed);
        }
        let mut received = if resuming { resume_from } else { 0 };

        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    hasher.update(&chunk);
                    received = received.saturating_add(chunk.len() as u64);
                    *counted = counted.saturating_add(chunk.len() as u64);
                    progress
                        .done
                        .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    if unsized_task {
                        progress
                            .total
                            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    }
                    progress.emit(events, false);
                }
                Ok(None) => break,
                Err(error) => return Err(error.into()),
            }
        }
        file.flush().await?;

        let actual = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let size_ok = task.size.is_none_or(|size| received == size);
        let sha1_ok = task
            .sha1
            .as_deref()
            .is_none_or(|sha1| actual.eq_ignore_ascii_case(sha1));
        if size_ok && sha1_ok {
            if tokio::fs::try_exists(&task.path).await? {
                tokio::fs::remove_file(&task.path).await?;
            }
            tokio::fs::rename(temporary, &task.path).await?;
            progress.emit(events, true);
            return Ok(true);
        }

        // 校验不过说明落盘的那份是脏的，续传只会一直错下去。
        let _ = tokio::fs::remove_file(temporary).await;
        Err(anyhow!("checksum or size mismatch for {}", task.url))
    }
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

    #[tokio::test]
    async fn an_unverified_task_only_asks_whether_the_file_is_there() {
        let root = std::env::temp_dir().join(format!("fern-unverified-{}", std::process::id()));
        tokio::fs::create_dir_all(&root).await.expect("create root");
        let path = root.join("loader.jar");

        let task = DownloadTask::unverified(&path, "https://maven.example.invalid/loader.jar")
            .expect("build task");
        assert!(!task.is_satisfied().await.expect("check missing"));

        tokio::fs::write(&path, b"anything")
            .await
            .expect("write file");
        assert!(task.is_satisfied().await.expect("check present"));

        // 有校验和的仍然按内容判断，不因为这条改动松掉。
        let verified =
            DownloadTask::new(&path, "https://maven.example.invalid/loader.jar", "00", 8)
                .expect("build task");
        assert!(!verified.is_satisfied().await.expect("check content"));

        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    /// 只差扩展名的两个文件不能共用一个临时文件。共用了，同一批里它们会互相
    /// 覆盖，而各自的 sha1 照样对得上——见 [`part_path`]。
    #[test]
    fn files_that_differ_only_in_extension_get_their_own_temporary() {
        let bin = Path::new("/fern/runtimes/jre-legacy/bin");
        assert_ne!(
            part_path(&bin.join("java.exe")),
            part_path(&bin.join("java.dll"))
        );
        assert_eq!(part_path(&bin.join("java.exe")), bin.join("java.exe.part"));
        // 没有扩展名的也照样加后缀，不会变成「就是它自己」。
        assert_eq!(part_path(&bin.join("java")), bin.join("java.part"));
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
        let mut progress = BatchProgress::new(4096);
        // 让「已经跑了一会儿」成立，否则首次调用的 now 和初始值都是 0，
        // 分不出「还没发过」和「刚发过」。
        progress.started = Instant::now() - Duration::from_millis(500);
        progress.done.store(1024, Ordering::Relaxed);

        progress.emit(&events, false);
        progress.emit(&events, false);
        progress.emit(&events, true);
        drop(events);

        let mut count = 0;
        while received.try_recv().is_ok() {
            count += 1;
        }
        // 两条限流掉一条，加上强制的那一条。
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn bytes_of_unsized_files_grow_the_total_instead_of_overflowing_it() {
        // 第三方 Maven 的库没有已知大小。上一版把它们排除在分母外、下载的
        // 字节却计进分子，于是「已下载」跑到「总量」前面去了。
        const BODY: &[u8] = b"maven artifact bytes";
        let url = flaky_server(0, BODY, "200 OK").await;
        let root = scratch("unsized");
        let present = root.join("present.jar");
        std::fs::write(&present, b"fern").expect("write present");
        // 一个已经在磁盘上的已知大小文件，加一个要真下载的未知大小文件。
        let known = DownloadTask::new(
            &present,
            url.as_str(),
            "654edb122a04602f918500d59b1d6fc37b9d0c01",
            4,
        )
        .expect("build known task");
        let unsized_task =
            DownloadTask::unverified(root.join("library.jar"), url.as_str()).expect("build task");

        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let client = DownloadClient::new(vec![Arc::new(OfficialSource)], 4);
        client
            .download_all(vec![known, unsized_task], &events)
            .await
            .expect("download");
        drop(events);

        let mut last = None;
        while let Ok(event) = received.try_recv() {
            if let DownloadEvent::Progress {
                done_bytes,
                total_bytes,
                ..
            } = event
            {
                assert!(
                    done_bytes <= total_bytes,
                    "已下载（{done_bytes}）超过了总量（{total_bytes}）"
                );
                last = Some((done_bytes, total_bytes));
            }
        }
        let (done, total) = last.expect("progress was reported");
        assert_eq!(done, total, "批次结束时两个数必须相等");
        assert_eq!(total, 4 + BODY.len() as u64);
        std::fs::remove_dir_all(root).ok();
    }

    /// 一个前 `refusals` 次连上来就直接断开、之后正常应答的服务器。
    ///
    /// 重试这件事，只有真的收到第二次请求才算验过。
    async fn flaky_server(refusals: usize, body: &'static [u8], status: &'static str) -> Url {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("local address");
        tokio::spawn(async move {
            let mut seen = 0usize;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer).await;
                if seen < refusals {
                    seen += 1;
                    // 不回任何东西，直接断开：这就是网络抖动的样子。
                    continue;
                }
                seen += 1;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
                let _ = stream.flush().await;
            }
        });
        Url::parse(&format!("http://{address}/client.jar")).expect("server url")
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fern-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create scratch");
        root
    }

    #[tokio::test]
    async fn a_batch_retries_the_files_that_failed() {
        // 单个源的三次尝试全部用光，整批还要再来一轮——成片的失败往往是一次
        // 短暂的断网，而那几十个文件各自的三次尝试都发生在那几秒之内。
        const BODY: &[u8] = b"fern";
        let url = flaky_server(ATTEMPTS_PER_SOURCE as usize, BODY, "200 OK").await;
        let root = scratch("retry");
        let path = root.join("client.jar");
        let task = DownloadTask::new(
            &path,
            url.as_str(),
            "654edb122a04602f918500d59b1d6fc37b9d0c01",
            BODY.len() as u64,
        )
        .expect("build task");

        let (events, _received) = tokio::sync::mpsc::unbounded_channel();
        let client = DownloadClient::new(vec![Arc::new(OfficialSource)], 4);
        client
            .download_all(vec![task], &events)
            .await
            .expect("second round succeeds");
        assert_eq!(std::fs::read(&path).expect("downloaded file"), BODY);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn a_missing_file_is_reported_by_name_instead_of_being_hammered() {
        // 上游说没有就是没有，重试整批不会有别的结果。而报出来的那句话要说得出
        // 是哪个文件、为什么——「12 个文件下载失败」对谁都没有用。
        let url = flaky_server(0, b"", "404 Not Found").await;
        let root = scratch("missing");
        let task =
            DownloadTask::new(root.join("absent.jar"), url.as_str(), "00", 4).expect("build task");

        let (events, _received) = tokio::sync::mpsc::unbounded_channel();
        let client = DownloadClient::new(vec![Arc::new(OfficialSource)], 4);
        let started = Instant::now();
        let error = client
            .download_all(vec![task], &events)
            .await
            .expect_err("nothing to download");
        let message = format!("{error}");
        assert!(message.contains("absent.jar"), "{message}");
        assert!(message.contains("没有这个文件"), "{message}");
        // 没有退避、没有第二轮：确定的失败不该让用户多等。
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "重试了不该重试的"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_failure_report_names_files_and_reasons() {
        let failure = |name: &str, reason: &str| Failure {
            task: DownloadTask::unverified(
                PathBuf::from("/fern/libraries").join(name),
                "https://example.invalid/x.jar",
            )
            .expect("build task"),
            reason: reason.to_owned(),
            retryable: true,
        };
        let one = describe_failures(&[failure("asm.jar", "连接超时")]);
        assert!(one.contains("asm.jar（连接超时）"), "{one}");

        // 一次断网能打掉几百个文件，全列出来就没人看了。
        let many: Vec<Failure> = (0..9)
            .map(|i| failure(&format!("a{i}.jar"), "连接超时"))
            .collect();
        let report = describe_failures(&many);
        assert!(report.contains("9 个文件没有下载成功"), "{report}");
        assert!(report.contains("另有 6 个"), "{report}");
    }

    #[test]
    fn rewrites_known_mirrors() {
        let source = BmclapiSource;
        let official = Url::parse("https://libraries.minecraft.net/com/mojang/test.jar").unwrap();
        let mirror = source.rewrite(&official);
        assert_eq!(mirror.host_str(), Some("bmclapi2.bangbang93.com"));
        assert_eq!(mirror.path(), "/maven/com/mojang/test.jar");

        let fabric = source
            .rewrite(&Url::parse("https://meta.fabricmc.net/v2/versions/loader/1.21.1").unwrap());
        assert_eq!(fabric.host_str(), Some("bmclapi2.bangbang93.com"));
        assert_eq!(fabric.path(), "/fabric-meta/v2/versions/loader/1.21.1");

        // 没镜像的域名原样放行，别把请求送到一个不存在的路径上。
        let quilt = source.rewrite(&Url::parse("https://meta.quiltmc.org/v3/versions").unwrap());
        assert_eq!(quilt.host_str(), Some("meta.quiltmc.org"));
        assert_eq!(quilt.path(), "/v3/versions");
    }
}
