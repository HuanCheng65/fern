//! Download and verification primitives for Fern.
//!
//! The downloader owns network and filesystem behavior while the UI receives
//! only serialized [`DownloadEvent`] values through the core boundary.

use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
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

mod verified;

pub use verified::Verified;

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

/// 网络上跑的那份字节，怎么变成落盘的那份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// LZMA（alone 格式，就是 `.lzma` 那种单文件流）。
    Lzma,
}

/// 真正要传的那一份——当它和要落盘的那一份不是同一堆字节的时候。
///
/// 目前只有一个来源：Mojang 的 Java 运行时清单给多数文件配了一份 lzma 变体。
/// java-runtime-gamma（linux-x64）实测 95.2 MB 的文件只要过 68.2 MB 的网。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wire {
    pub url: Url,
    /// 压缩包自己的 sha1，不是成品的。两个都要验：前者证明传输没坏，后者证明
    /// 解出来的确实是清单说的那份东西。
    pub sha1: String,
    pub size: u64,
    pub codec: Codec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    pub path: PathBuf,
    pub url: Url,
    /// 官方元数据每个文件都有 sha1，所以这是常态。第三方 Maven（Fabric、
    /// Forge 的库）只给一个 URL，那时候只能认「下下来了」。
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// 换一条压缩过的线来传。为 `None` 时下什么就是什么。
    ///
    /// **它不改变这个任务描述的东西**：`path`、`sha1`、`size` 说的始终是成品，
    /// 所以「磁盘上那份算不算数」「账本记的是谁」都不受影响——压缩只是运输方式。
    pub wire: Option<Wire>,
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
            wire: None,
        })
    }

    /// 改走压缩过的那条线。
    ///
    /// `self` 描述的仍然是落盘的那一份——校验、账本、「已经有了吗」问的都是它。
    /// 换掉的只是网络上跑的字节，以及跑完之后多一道解压。
    pub fn via(
        mut self,
        codec: Codec,
        url: &str,
        sha1: impl Into<String>,
        size: u64,
    ) -> Result<Self> {
        self.wire = Some(Wire {
            url: Url::parse(url).context("invalid download URL")?,
            sha1: sha1.into().to_ascii_lowercase(),
            size,
            codec,
        });
        Ok(self)
    }

    /// 网络上要跑多少字节。
    ///
    /// 进度算的是这个数，不是成品大小：压缩过的文件若按解压后的量计入分母、
    /// 按实际收到的量计入分子，进度条永远到不了头。
    fn wire_size(&self) -> Option<u64> {
        match &self.wire {
            Some(wire) => Some(wire.size),
            None => self.size,
        }
    }

    /// 这一次要去请求的地址。
    fn wire_url(&self) -> &Url {
        match &self.wire {
            Some(wire) => &wire.url,
            None => &self.url,
        }
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
            wire: None,
        })
    }

    /// 这一份已经落盘的能不能算数，每次都实打实地读一遍。
    ///
    /// 下载路径上走的不是这一条，是 [`DownloadClient::satisfied`]——它会先问账本。
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
    hex(Sha1::digest(bytes))
}

pub fn sha1_matches(bytes: &[u8], expected: &str) -> bool {
    sha1_hex(bytes).eq_ignore_ascii_case(expected)
}

/// 二十个字节写成四十个小写十六进制字符。
///
/// 上一版是 `map(|byte| format!("{byte:02x}")).collect()`——每个字节一次堆分配。
/// 单看无所谓，但校验路径上每个文件都要走一次，而一个实例有四千个文件。
fn hex(digest: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = digest.as_ref();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// 读一遍算 sha1，用一块固定大小的缓冲。
///
/// 上一版是 `tokio::fs::read` 把整份读进内存再算。一个批次里同时有 64 个任务在
/// 校验，而里面最大的是 client jar 和 Java 运行时那种上百兆的文件——峰值内存
/// 是这个数乘以并发数，且完全没有必要：哈希本来就是流式的。
///
/// 同步实现，因为它整个都是文件系统和 CPU 的活。调用方按批交给阻塞线程
/// （见 [`DownloadClient::reconcile`]），而不是每个文件派一次。
fn hash_on_disk(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; STREAM_BUFFER];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(hasher.finalize()))
}

pub async fn verify_file(path: &Path, expected_sha1: &str, expected_size: u64) -> Result<bool> {
    let path = path.to_path_buf();
    let expected_sha1 = expected_sha1.to_owned();
    tokio::task::spawn_blocking(move || {
        // 大小对不上就不用读了。
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.len() == expected_size => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
        Ok(hash_on_disk(&path)?.eq_ignore_ascii_case(&expected_sha1))
    })
    .await?
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

/// 压缩包先落在哪。
///
/// 追加在 `.part` 后面，理由和 [`part_path`] 那条一样：名字唯一是唯一的出路。
fn wire_path(staged: &Path) -> PathBuf {
    let mut name = staged.to_path_buf().into_os_string();
    name.push(".wire");
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

/// 读盘和写盘的缓冲块。
///
/// 写这一头是重点：reqwest 递过来的一块通常几十 KB，而 tokio 的 `File` 每次
/// `write` 都要往阻塞线程池派一次活——一个两百兆的文件就是几千次派发。攒够一
/// 大块再落盘，这个数就降两个数量级。
const STREAM_BUFFER: usize = 512 * 1024;

/// 对账时一个阻塞任务包多少个文件。
///
/// 一个文件一次 `spawn_blocking` 的话，四千个资源文件就是四千次派发，而每次
/// 派发要做的事只是一个 `stat`。按批交出去，派发次数降到几十。
const RECONCILE_BATCH: usize = 256;

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
    /// 这一批发出去过多少个请求，其中多少个是重试。
    ///
    /// 记在这里是因为它已经被传到每一层了，再多穿一个 `Arc` 只是噪声。这两个
    /// 数不进界面，只进 `fern.log`——「慢在哪一段」这个问题，没有数就只能靠猜。
    requests: AtomicU64,
    retries: AtomicU64,
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
            requests: AtomicU64::new(0),
            retries: AtomicU64::new(0),
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

/// 一批下完之后，那行账写到哪去。
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;

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

/// 一批下载的账，写进 `fern.log`。
///
/// 「下载慢」是最难查的那类报告——慢在对账、慢在传输，还是慢在一路重试，进度条
/// 一个也答不出来。这几个数就是为了让下一次「慢」有据可查。
fn describe_batch(
    total_files: u64,
    settled_files: u64,
    settled_bytes: u64,
    failures: &[Failure],
    progress: &BatchProgress,
    checking: Duration,
    downloading: Duration,
) -> String {
    let fetched_bytes = progress
        .done
        .load(Ordering::Relaxed)
        .saturating_sub(settled_bytes);
    let speed = fetched_bytes.saturating_mul(1000) / (downloading.as_millis() as u64).max(1);
    format!(
        "[download] {total_files} 个文件：跳过 {settled_files}（{}），下载 {}（{}），失败 {}；\
         对账 {}，传输 {}（{}/s）；请求 {} 次，其中重试 {} 次",
        amount(settled_bytes),
        total_files
            .saturating_sub(settled_files)
            .saturating_sub(failures.len() as u64),
        amount(fetched_bytes),
        failures.len(),
        moment(checking),
        moment(downloading),
        amount(speed),
        progress.requests.load(Ordering::Relaxed),
        progress.retries.load(Ordering::Relaxed),
    )
}

fn amount(count: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{count} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn moment(elapsed: Duration) -> String {
    if elapsed < Duration::from_secs(1) {
        format!("{} ms", elapsed.as_millis())
    } else {
        format!("{:.1} s", elapsed.as_secs_f64())
    }
}

/// 全局同时开几个文件。
///
/// 一条线上的每个调用点都从同一个客户端分出去，所以这个数是**整台机器上真正
/// 同时开着的连接数**，不是某一处的上限。以前它是十几个各写各的常量：补全游戏
/// 文件 64、准备 Java 64，两条并排跑，加起来 128，谁也没打算要这个数。
pub const DEFAULT_CONCURRENCY: usize = 64;

/// 走不走代理。
///
/// 默认跟随系统，也就是 `HTTP_PROXY` 那几个环境变量——这是加这一项之前唯一
/// 存在的行为。另外两档是为了那些环境变量说了不算的场合：机器上装着一个全局
/// 代理但下载源恰好被它绕远，或者反过来，只有启动器需要走代理。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Proxy {
    #[default]
    System,
    Direct,
    Url(String),
}

/// 用户能调的那几项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub concurrency: usize,
    /// 全局每秒最多多少字节。`None` 是不限。
    ///
    /// 记在共用的那个客户端上，所以它管的是整个启动器，而不是某一支——
    /// 「后台装整合包的时候别把会议卡掉」这句话只有在全局成立才是真的。
    pub rate: Option<u64>,
    pub proxy: Proxy,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            concurrency: DEFAULT_CONCURRENCY,
            rate: None,
            proxy: Proxy::default(),
        }
    }
}

/// 令牌桶。
///
/// 桶的容量取「一秒的量」和「这一块的大小」里较大的那个。后半句不是可有可无：
/// 限速 8 KB/s 而 reqwest 递过来一块 16 KB 时，容量若只有一秒的量，令牌永远
/// 攒不到这一块要的数，`take` 会原地转圈。
struct RateLimiter {
    rate: u64,
    bucket: Mutex<(f64, Instant)>,
}

impl RateLimiter {
    fn new(rate: u64) -> Self {
        Self {
            rate: rate.max(1),
            bucket: Mutex::new((0.0, Instant::now())),
        }
    }

    async fn take(&self, bytes: u64) {
        loop {
            let wait = {
                let Ok(mut bucket) = self.bucket.lock() else {
                    return;
                };
                let (tokens, last) = &mut *bucket;
                let now = Instant::now();
                let capacity = self.rate.max(bytes) as f64;
                *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * self.rate as f64)
                    .min(capacity);
                *last = now;
                if *tokens >= bytes as f64 {
                    *tokens -= bytes as f64;
                    return;
                }
                Duration::from_secs_f64((bytes as f64 - *tokens) / self.rate as f64)
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[derive(Clone)]
pub struct DownloadClient {
    client: reqwest::Client,
    sources: Vec<Arc<dyn DownloadSource>>,
    /// 全局闸门，所有分支共用一个。
    semaphore: Arc<Semaphore>,
    /// 这一支自己的上限，比全局更紧。见 [`Self::lane`]。
    lane: Option<Arc<Semaphore>>,
    /// 全局限速。和闸门一样挂在共用的那个客户端上，各支共用一个桶。
    limiter: Option<Arc<RateLimiter>>,
    health: Arc<SourceHealth>,
    /// 验过的文件。默认是关着的一本空账，[`Self::with_verified`] 才给它落盘位置。
    verified: Arc<Verified>,
    /// 不信账本，每个文件都实打实读一遍。见 [`Self::rechecking`]。
    recheck: bool,
    /// 一批下完之后把账写到哪。默认哪也不写。见 [`Self::with_log`]。
    log: Option<LogSink>,
}

impl DownloadClient {
    pub fn new(sources: Vec<Arc<dyn DownloadSource>>, concurrency: usize) -> Self {
        Self::configured(
            sources,
            &Network {
                concurrency,
                ..Network::default()
            },
        )
    }

    /// 全进程共用的那一个，按用户在设置里定的那几项配好。
    ///
    /// 连接池、源健康度、全局闸门和限速桶都在这里，所以每一处调用都从它分支
    /// 出去（[`Self::lane`]），而不是各建各的：各建各的意味着每个阶段重新握一遍
    /// TLS，也意味着没有任何地方回答得了「现在一共开着多少条」。
    pub fn configured(sources: Vec<Arc<dyn DownloadSource>>, network: &Network) -> Self {
        Self {
            client: Self::http(&network.proxy),
            sources: if sources.is_empty() {
                vec![Arc::new(OfficialSource)]
            } else {
                sources
            },
            semaphore: Arc::new(Semaphore::new(network.concurrency.max(1))),
            lane: None,
            limiter: network.rate.map(|rate| Arc::new(RateLimiter::new(rate))),
            health: Arc::new(SourceHealth::default()),
            verified: Arc::new(Verified::default()),
            recheck: false,
            log: None,
        }
    }

    fn http(proxy: &Proxy) -> reqwest::Client {
        let builder = reqwest::Client::builder()
            // HTTP/2 的初始窗口是 64 KB。在一条延迟高的线路上，那意味着每收
            // 64 KB 就要空等一个来回——比 HTTP/1.1 还慢。打开自适应窗口，让它
            // 按实测的带宽时延积自己长。**开 h2 就必须开这个**，否则「优化」
            // 会变成大文件下载的回退。
            .http2_adaptive_window(true)
            .connect_timeout(std::time::Duration::from_secs(10))
            // 读超时，不是总超时。总超时是按「一次请求最多花多久」设的，
            // 而这里最大的两个文件是 client jar 和 Java 运行时——两百多兆
            // 在一条普通的家用带宽上本来就要跑几分钟。之前设的 45 秒总
            // 超时会把它们**每一次**都掐死在半路，表现出来正是「有几个
            // 文件总是失败」。真正该管的是「卡住不动」，那是读超时。
            .read_timeout(std::time::Duration::from_secs(30));
        let builder = match proxy {
            Proxy::System => builder,
            Proxy::Direct => builder.no_proxy(),
            // 地址填错了不该让启动器连不上网：退回跟随系统，设置界面负责说清楚
            // 这个地址不合法。
            Proxy::Url(url) => match reqwest::Proxy::all(url) {
                Ok(proxy) => builder.proxy(proxy),
                Err(_) => builder,
            },
        };
        builder
            .build()
            .expect("valid download client configuration")
    }

    /// 分出一支，最多同时开 `concurrency` 个文件。
    ///
    /// 共用连接池、源健康度和账本，只是自己再紧一道：元数据那种「顺带一两个
    /// 请求」的活不该在补全整个实例的时候抢满全局配额。全局闸门仍然在外面，
    /// 各支加起来也超不过它。
    pub fn lane(&self, concurrency: usize) -> Self {
        Self {
            lane: Some(Arc::new(Semaphore::new(concurrency.max(1)))),
            ..self.clone()
        }
    }

    /// 记住验过的文件，别每一轮都把整个 assets 目录重算一遍哈希。
    ///
    /// 账本由调用方持有并在整个进程里共用一本——补全游戏文件和准备 Java 是并排
    /// 跑的两条线，各记各的会互相覆盖。
    pub fn with_verified(mut self, verified: Arc<Verified>) -> Self {
        self.verified = verified;
        self
    }

    /// 不认账本的那一份。
    ///
    /// 平常那一遍靠「大小和修改时间都没变」跳过重算，代价是内容被原地改坏、
    /// 大小和时间戳却没动的文件它看不出来。用户点「校验」正是在说「我不信磁盘上
    /// 那份」，所以那条路必须真的把每个文件读一遍。
    pub fn rechecking(mut self) -> Self {
        self.recheck = true;
        self
    }

    /// 每批下完写一行账。
    ///
    /// 「下载慢」是最难查的那类报告：慢在对账、慢在握手、慢在传输，还是慢在
    /// 重试，光看进度条一个也分不出来。所以一批结束时把这几个数记下来——它是
    /// 诊断，不是功能，写不进去就算了。
    pub fn with_log(mut self, log: LogSink) -> Self {
        self.log = Some(log);
        self
    }

    /// 收了闭包而不是字符串：没人接这行账的时候，连拼都不必拼。
    fn note(&self, line: impl FnOnce() -> String) {
        if let Some(log) = &self.log {
            log(&line());
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
        let known_total = tasks.iter().filter_map(|task| task.wire_size()).sum();
        let total_files = tasks.len() as u64;
        let _ = events.send(DownloadEvent::TaskStarted {
            total_files,
            total_bytes: known_total,
        });
        let progress = Arc::new(BatchProgress::new(known_total));

        // 先整批对一遍账，磁盘上已经算数的当场结掉，只把真要下的送进下载路径。
        let checking = Instant::now();
        let (mut pending, settled_bytes) = self.reconcile(tasks, &progress, events).await;
        let settled_files = total_files - pending.len() as u64;
        let checked_in = checking.elapsed();
        // 目录先一次性建出来。以前是每个任务自己 `create_dir_all`——四千个资源
        // 文件只落在 256 个前缀目录下，其余三千七百多次全是白跑的往返。
        ensure_directories(&pending).await;

        let downloading = Instant::now();
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

        // 这一批学到的东西落盘。失败的批次也要存——已经验过的那些文件不会因为
        // 别的文件没下下来就变得不算数了。
        self.verified.save().await;

        let transferred_in = downloading.elapsed();
        self.note(|| {
            describe_batch(
                total_files,
                settled_files,
                settled_bytes,
                &failures,
                &progress,
                checked_in,
                transferred_in,
            )
        });

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
                        wire: None,
                    },
                    retryable: false,
                    reason: format!("下载任务异常结束：{error}"),
                }),
            }
        }
        failures
    }

    /// 整批对账，返回还要下的那些，以及已经算数的那些占多少字节。
    ///
    /// 这一遍以前长在 [`Self::download_one`] 里，而且是在拿到信号量**之后**。
    /// 对一个已经装好的实例，四千个任务全都只是在 stat 和读盘，却要排在一道
    /// 为网络设的闸门后面——磁盘的活和网络的活不该抢同一份配额。顺带还省掉了
    /// 四千次 `spawn_blocking`：现在一次派发管 256 个文件。
    async fn reconcile(
        &self,
        tasks: Vec<DownloadTask>,
        progress: &BatchProgress,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> (Vec<DownloadTask>, u64) {
        let mut jobs = tokio::task::JoinSet::new();
        let mut batch = Vec::with_capacity(RECONCILE_BATCH);
        for task in tasks {
            batch.push(task);
            if batch.len() == RECONCILE_BATCH {
                let client = self.clone();
                let batch = std::mem::replace(&mut batch, Vec::with_capacity(RECONCILE_BATCH));
                jobs.spawn_blocking(move || client.settle(batch));
            }
        }
        if !batch.is_empty() {
            let client = self.clone();
            jobs.spawn_blocking(move || client.settle(batch));
        }

        let mut pending = Vec::new();
        let mut settled_bytes = 0;
        while let Some(joined) = jobs.join_next().await {
            // 对账的任务自己没了，那 256 个文件的状态就无从知道。**不能跳过**：
            // 跳过等于它们既没被跳过也没进下载队列，这一批会报成功，而那些文件
            // 一个也没下。`settle` 里没有会 panic 的东西，走到这里就是真出了
            // 事，那就当场炸掉——静默地少下几百个文件是更坏的结局。
            let batch = joined.expect("对账任务异常结束");
            for (task, settled) in batch {
                if !settled {
                    pending.push(task);
                    continue;
                }
                // 大小未知的文件对分母的贡献是 0（它没被计入过），对分子也
                // 只能是 0——两边都不动，账才是平的。
                let bytes = task.wire_size().unwrap_or(0);
                settled_bytes += bytes;
                progress.done.fetch_add(bytes, Ordering::Relaxed);
                let _ = events.send(DownloadEvent::FileDone {
                    path: task.path.display().to_string(),
                    bytes: task.size.unwrap_or(0),
                });
            }
        }
        progress.emit(events, true);
        (pending, settled_bytes)
    }

    /// 一批文件各自算不算数。同步的：整个都是文件系统和 CPU 的活。
    fn settle(&self, tasks: Vec<DownloadTask>) -> Vec<(DownloadTask, bool)> {
        tasks
            .into_iter()
            .map(|task| {
                // 读不出来的当作「不算数」，交给下载路径去覆盖。这比报一句
                // 「读取已有文件失败」有用：用户要的是文件对，不是一句解释。
                let settled = self.settled(&task).unwrap_or(false);
                (task, settled)
            })
            .collect()
    }

    /// 磁盘上那份算不算数。
    ///
    /// 和 [`DownloadTask::is_satisfied`] 回答的是同一个问题，区别只在于要不要把
    /// 文件整个读一遍：大小对不上就直接否掉，大小对得上且账本认得这个
    /// `路径|大小|修改时间`，就用上次算出来的哈希。真读了的那些顺手记进账本。
    fn settled(&self, task: &DownloadTask) -> Result<bool> {
        let (Some(expected), Some(size)) = (task.sha1.as_deref(), task.size) else {
            // 没有校验和的任务只能问「在不在」，账本对它没有意义。
            return Ok(task.path.try_exists()?);
        };
        let metadata = match std::fs::metadata(&task.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        // 大小不对就不用往下看了，连读都不必读。
        if metadata.len() != size {
            return Ok(false);
        }
        // 点了「校验」就是在说「我不信磁盘上那份」，账本说什么都不算。
        if !self.recheck
            && let Some(known) = self.verified.recall(&task.path, &metadata)
        {
            return Ok(known.eq_ignore_ascii_case(expected));
        }
        let actual = hash_on_disk(&task.path)?;
        self.verified.remember(&task.path, &metadata, &actual);
        Ok(actual.eq_ignore_ascii_case(expected))
    }

    async fn download_one(
        &self,
        task: &DownloadTask,
        progress: &BatchProgress,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> std::result::Result<(), Refusal> {
        // 先过自己那一支，再过全局。反过来的话，一支拿满了全局配额却卡在自己
        // 的上限上，那些握着全局名额的任务什么也没干，别的支一个也进不来。
        let _lane = match &self.lane {
            Some(lane) => Some(
                lane.acquire()
                    .await
                    .map_err(|error| Refusal::fatal(format!("下载信号量已关闭：{error}")))?,
            ),
            None => None,
        };
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|error| Refusal::fatal(format!("下载信号量已关闭：{error}")))?;

        // 磁盘上已经算数的那些在进这道闸之前就被摘掉了（见 [`Self::reconcile`]），
        // 所以站在这里的都是真要下的。补全仍然天然幂等：「修复文件」就是同一个
        // 入口再跑一遍，对账那一步会把已经对的全部跳过。
        let temporary = part_path(&task.path);
        let mut last_error = None;
        // 每个源都明确回答「我这里没有」时，重试整批也不会有别的结果。
        let mut all_missing = true;
        for source in self.ordered_sources(task.wire_url()) {
            let url = source.rewrite(task.wire_url());
            let host = url.host_str().unwrap_or_default().to_owned();
            for attempt in 0..ATTEMPTS_PER_SOURCE {
                if attempt > 0 {
                    tokio::time::sleep(backoff(attempt)).await;
                    progress.retries.fetch_add(1, Ordering::Relaxed);
                }
                progress.requests.fetch_add(1, Ordering::Relaxed);
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
        if task.wire.is_some() {
            let _ = tokio::fs::remove_file(wire_path(&temporary)).await;
        }
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
        let unsized_task = task.wire_size().is_none();
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
        let wire_size = task.wire_size();
        let unsized_task = wire_size.is_none();
        // 网络上跑的那份先落在哪。压缩过的要多占一个位置：`<成品>.part` 留给解
        // 出来的那份，压缩包自己落在 `<成品>.part.wire`。
        let landing = match task.wire {
            Some(_) => wire_path(temporary),
            None => temporary.to_path_buf(),
        };
        // 断点续传：大文件断在半路时，已经落盘的那几十兆没有理由重下。
        // sha1 是整个文件的，所以续传时要先把已有的字节喂进 hasher。
        // 不知道总大小就没法判断落盘的那截是「断了一半」还是「已经下完」，
        // 续传的前提不成立，老实从头下。
        let resume_from = match wire_size {
            Some(size) if size >= RESUME_THRESHOLD => match tokio::fs::metadata(&landing).await {
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
        let file = if resuming {
            hasher.update(&tokio::fs::read(&landing).await?);
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&landing)
                .await?
        } else {
            match tokio::fs::File::create(&landing).await {
                Ok(file) => file,
                // 目录在整批开工前就建好了（见 `ensure_directories`）。真走到
                // 这里说明它中途没了，补一次再来——不为此在每个文件上都付一次
                // `create_dir_all` 的钱。
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(parent) = landing.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::File::create(&landing).await?
                }
                Err(error) => return Err(error.into()),
            }
        };
        // tokio 的 `File` 每次 `write` 都往阻塞线程池派一次活，而 reqwest 递
        // 过来的一块通常只有几十 KB。攒够一整块再落盘。
        let mut file = tokio::io::BufWriter::with_capacity(STREAM_BUFFER, file);

        if resuming {
            *counted = resume_from;
            progress.done.fetch_add(resume_from, Ordering::Relaxed);
        }
        let mut received = if resuming { resume_from } else { 0 };

        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    // 收下了才拦，但拦得住：不去取下一块，reqwest 就不再从
                    // 套接字上读，背压一路传到对端。
                    if let Some(limiter) = &self.limiter {
                        limiter.take(chunk.len() as u64).await;
                    }
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

        // 传过来的这一份先自证清白。压缩过的那些，这一步验的是压缩包自己——
        // 成品的 sha1 要等解出来才谈得上。
        let arrived = hex(hasher.finalize());
        let (expected_sha1, expected_size) = match &task.wire {
            Some(wire) => (Some(wire.sha1.as_str()), Some(wire.size)),
            None => (task.sha1.as_deref(), task.size),
        };
        if !(expected_size.is_none_or(|size| received == size)
            && expected_sha1.is_none_or(|sha1| arrived.eq_ignore_ascii_case(sha1)))
        {
            // 校验不过说明落盘的那份是脏的，续传只会一直错下去。
            let _ = tokio::fs::remove_file(&landing).await;
            return Err(anyhow!("checksum or size mismatch for {url}"));
        }

        // 压缩过的还要再解一道。解出来的那份才是要落到目的地的东西，也才是
        // 校验和账本说的那个 sha1。
        let produced = match &task.wire {
            None => arrived,
            Some(wire) => {
                let (codec, from, to) = (wire.codec, landing.clone(), temporary.to_path_buf());
                // 解压是 CPU 的活，不该按下载那个并发数来开：64 个一起解不会更
                // 快，而 LZMA 的解码器每个都要留一份字典大小的缓冲，内存峰值
                // 就成了「并发数 × 字典大小」。
                let _expanding = expanders().acquire().await;
                let expanded =
                    tokio::task::spawn_blocking(move || expand(codec, &from, &to)).await?;
                let _ = tokio::fs::remove_file(&landing).await;
                let (sha1, size) = expanded?;
                if !(task.size.is_none_or(|expected| size == expected)
                    && task
                        .sha1
                        .as_deref()
                        .is_none_or(|expected| sha1.eq_ignore_ascii_case(expected)))
                {
                    let _ = tokio::fs::remove_file(temporary).await;
                    return Err(anyhow!("checksum or size mismatch for {}", task.url));
                }
                sha1
            }
        };

        replace(temporary, &task.path).await?;
        // 哈希是就着流现算的，刚落成的这一份就是它。记下来，紧接着的下一次启动
        // 才不用把刚下的四千个文件再读一遍。没有声明校验和的任务不记：账本只在
        // 有期望值可比的时候派得上用场。
        if task.sha1.is_some() {
            self.verified.note(&task.path, &produced).await;
        }
        progress.emit(events, true);
        Ok(true)
    }
}

/// 挪到目的地。
///
/// Unix 的 rename 直接覆盖，Windows 上目标存在才会失败。所以先试、失败了再删了
/// 重来——而不是每个文件都先 stat 一次问「目的地在不在」。四千个资源文件，那就是
/// 四千次白跑的往返，且其中绝大多数的答案是「不在」。
async fn replace(temporary: &Path, destination: &Path) -> Result<()> {
    if tokio::fs::rename(temporary, destination).await.is_ok() {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(destination).await;
    tokio::fs::rename(temporary, destination).await?;
    Ok(())
}

/// 把这一批要写进的目录一次性建出来。
///
/// 以前是每个任务自己 `create_dir_all`。资源文件只落在 256 个前缀目录下，四千
/// 个任务里有三千七百多次是在重复建同一批目录，每一次都是一趟阻塞线程的往返。
async fn ensure_directories(tasks: &[DownloadTask]) {
    let parents: HashSet<PathBuf> = tasks
        .iter()
        .filter_map(|task| task.path.parent().map(Path::to_path_buf))
        .collect();
    if parents.is_empty() {
        return;
    }
    // 建不出来的不在这里报错：这里只知道是哪个目录，下载路径知道是哪个文件。
    let _ = tokio::task::spawn_blocking(move || {
        for parent in parents {
            let _ = std::fs::create_dir_all(parent);
        }
    })
    .await;
}

/// 同时解几个压缩包。
///
/// 全进程一道，和下载那道闸各管各的：下载卡在带宽上，解压卡在核数上，用同一个
/// 数字管两件事，两边都不对。
fn expanders() -> &'static Semaphore {
    static EXPANDERS: std::sync::OnceLock<Semaphore> = std::sync::OnceLock::new();
    EXPANDERS.get_or_init(|| {
        Semaphore::new(
            std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4),
        )
    })
}

/// 解压，同时把解出来的字节算进 sha1。
///
/// 两件事必须在一遍里做完：解完再读一遍磁盘算哈希，是为同一个答案付两次钱，
/// 而这个答案的分母是一份两百兆的 Java 运行时。
fn expand(codec: Codec, from: &Path, to: &Path) -> Result<(String, u64)> {
    let mut reader = std::io::BufReader::with_capacity(STREAM_BUFFER, std::fs::File::open(from)?);
    let mut writer = Tally {
        inner: std::io::BufWriter::with_capacity(STREAM_BUFFER, std::fs::File::create(to)?),
        hasher: Sha1::new(),
        bytes: 0,
    };
    match codec {
        Codec::Lzma => lzma_rs::lzma_decompress(&mut reader, &mut writer)
            .map_err(|error| anyhow!("解压失败：{error}"))?,
    }
    writer.inner.flush()?;
    Ok((hex(writer.hasher.finalize()), writer.bytes))
}

/// 一边往下游写，一边把写过去的字节算进哈希。
struct Tally<W: Write> {
    inner: W,
    hasher: Sha1,
    bytes: u64,
}

impl<W: Write> Write for Tally<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
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

    /// 账本省掉的那一遍读，以及它省不掉的那一遍。
    ///
    /// 这个测试把交易本身写下来：认「大小和修改时间都没变」，就一定认不出原地
    /// 改坏、大小和时间戳却没动的文件。所以「校验」那条路不认账本。
    #[tokio::test]
    async fn the_ledger_is_trusted_until_someone_asks_for_a_recheck() {
        let root = std::env::temp_dir().join(format!("fern-ledger-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&root).await.expect("create root");
        let path = root.join("client.jar");
        tokio::fs::write(&path, b"fern").await.expect("write");

        let task = DownloadTask::new(
            &path,
            "https://example.invalid/client.jar",
            "654edb122a04602f918500d59b1d6fc37b9d0c01",
            4,
        )
        .expect("build task");
        let client = DownloadClient::new(vec![Arc::new(OfficialSource)], 4)
            .with_verified(Verified::at(root.join("verified.json")));
        assert!(client.settled(&task).expect("first pass"));

        // 内容原地换成同样长的另一份，再把修改时间按回原样。
        let stamp = std::fs::metadata(&path)
            .expect("stat")
            .modified()
            .expect("mtime");
        tokio::fs::write(&path, b"leaf").await.expect("overwrite");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(stamp)
            .expect("restore mtime");

        assert!(
            client.settled(&task).expect("cached pass"),
            "大小和修改时间都没变，账本就认上次那个哈希"
        );
        assert!(
            !client.clone().rechecking().settled(&task).expect("recheck"),
            "点了校验就得真读一遍，账本说什么都不算"
        );

        tokio::fs::remove_dir_all(root).await.expect("clean up");
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
    async fn flaky_server(refusals: usize, body: impl Into<Vec<u8>>, status: &'static str) -> Url {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let body = body.into();
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
                let _ = stream.write_all(&body).await;
                let _ = stream.flush().await;
            }
        });
        Url::parse(&format!("http://{address}/client.jar")).expect("server url")
    }

    fn lzma_blob(body: &[u8]) -> Vec<u8> {
        let mut packed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::BufReader::new(body), &mut packed)
            .expect("compress fixture");
        packed
    }

    /// 桶要拦得住，也要拦得过去。
    ///
    /// 后半句是这个测试真正在守的东西：一块比一秒的额度还大的数据，如果桶的
    /// 容量只按「一秒的量」算，令牌永远攒不够，`take` 会一直转下去。
    #[tokio::test]
    async fn the_bucket_slows_things_down_without_ever_wedging() {
        let limiter = RateLimiter::new(1_000_000);
        let started = Instant::now();
        limiter.take(300_000).await;
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "限速没有生效，只花了 {:?}",
            started.elapsed()
        );

        let limiter = RateLimiter::new(100_000);
        let started = Instant::now();
        limiter.take(110_000).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(1) && elapsed < Duration::from_secs(5),
            "一块超过一秒额度的数据该等一秒多一点就过去，实际 {elapsed:?}"
        );
    }

    /// 一个会记下「同时有几个请求压在身上」的服务器。
    ///
    /// 每个请求先压住 60ms 再答，不然并发根本来不及重叠，量出来的峰值只会是 1。
    async fn counting_server(body: &'static [u8]) -> (Url, Arc<AtomicU64>) {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("local address");
        let peak = Arc::new(AtomicU64::new(0));
        let reported = peak.clone();
        tokio::spawn(async move {
            let live = Arc::new(AtomicU64::new(0));
            while let Ok((mut stream, _)) = listener.accept().await {
                let (peak, live) = (peak.clone(), live.clone());
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                    let _ = stream.flush().await;
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        (
            Url::parse(&format!("http://{address}/")).expect("server url"),
            reported,
        )
    }

    /// 分支各有各的上限，但全局那道闸在它们外面。
    ///
    /// 这正是把十几个各写各的并发数收成一个客户端要换来的东西：以前补全游戏
    /// 文件 64、准备 Java 64，两条并排跑就是 128，没有任何地方拦得住。
    #[tokio::test]
    async fn lanes_are_bounded_by_themselves_and_by_the_shared_limit() {
        const BODY: &[u8] = b"fern";
        const SHA1: &str = "654edb122a04602f918500d59b1d6fc37b9d0c01";
        let root = scratch("lanes");
        let (events, _received) = tokio::sync::mpsc::unbounded_channel();

        let batch = |base: &Url, tag: &str| {
            (0..6)
                .map(|i| {
                    let name = format!("{tag}{i}");
                    DownloadTask::new(
                        root.join(&name),
                        base.join(&name).expect("task url").as_str(),
                        SHA1,
                        BODY.len() as u64,
                    )
                    .expect("build task")
                })
                .collect::<Vec<_>>()
        };

        // 两支各允许 4 个，全局只允许 3 个。
        let (base, peak) = counting_server(BODY).await;
        let shared = DownloadClient::new(vec![Arc::new(OfficialSource)], 3);
        let (one, two) = (shared.lane(4), shared.lane(4));
        let (first, second) = tokio::join!(
            one.download_all(batch(&base, "a"), &events),
            two.download_all(batch(&base, "b"), &events),
        );
        first.expect("first lane");
        second.expect("second lane");
        let peak = peak.load(Ordering::SeqCst);
        assert!(peak > 1, "该并发的没并发起来，峰值只有 {peak}");
        assert!(peak <= 3, "两支加起来越过了全局那道闸，峰值到了 {peak}");

        // 反过来，全局宽松而分支收紧时，收紧的那一个说了算。
        let (base, peak) = counting_server(BODY).await;
        let shared = DownloadClient::new(vec![Arc::new(OfficialSource)], 16);
        shared
            .lane(1)
            .download_all(batch(&base, "c"), &events)
            .await
            .expect("single-file lane");
        assert_eq!(peak.load(Ordering::SeqCst), 1, "这一支只该一个一个来");

        std::fs::remove_dir_all(root).ok();
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

    /// 压缩过的那条线：网上跑的是压缩包，落盘的是解出来的成品，两头各验各的。
    ///
    /// `raw` 那个地址在这个测试里根本连不上——压缩变体在的时候就不该去碰它。
    #[tokio::test]
    async fn a_compressed_task_lands_the_file_its_manifest_describes() {
        const BODY: &[u8] = b"a java runtime file, repetitive enough to be worth packing";
        let packed = lzma_blob(BODY);
        let url = flaky_server(0, packed.clone(), "200 OK").await;
        let root = scratch("compressed");
        // 父目录还不存在：整批开工前那一次性的建目录也一并验了。
        let path = root.join("lib").join("modules");

        let task = DownloadTask::new(
            &path,
            "https://127.0.0.1:1/never",
            sha1_hex(BODY),
            BODY.len() as u64,
        )
        .expect("build task")
        .via(
            Codec::Lzma,
            url.as_str(),
            sha1_hex(&packed),
            packed.len() as u64,
        )
        .expect("attach the compressed variant");

        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        DownloadClient::new(vec![Arc::new(OfficialSource)], 4)
            .download_all(vec![task], &events)
            .await
            .expect("download");
        drop(events);

        assert_eq!(std::fs::read(&path).expect("expanded file"), BODY);
        // 压缩包和临时文件都不该留在磁盘上。
        assert!(!part_path(&path).exists(), "临时文件没清掉");
        assert!(!wire_path(&part_path(&path)).exists(), "压缩包没清掉");

        // 进度按网上真跑的字节算。分母若用解压后的大小，进度条永远到不了头。
        let mut last = None;
        while let Ok(event) = received.try_recv() {
            if let DownloadEvent::Progress {
                done_bytes,
                total_bytes,
                ..
            } = event
            {
                last = Some((done_bytes, total_bytes));
            }
        }
        assert_eq!(last, Some((packed.len() as u64, packed.len() as u64)));
        std::fs::remove_dir_all(root).ok();
    }

    /// 解压要在同一遍里算出成品的哈希和大小——解完再读一遍磁盘，是为同一个答案
    /// 付两次钱，而这个答案的分母是一份两百兆的运行时。
    #[test]
    fn expanding_reports_what_it_produced() {
        const BODY: &[u8] = b"the bytes the manifest promised";
        let root = scratch("expand");
        let packed = root.join("modules.lzma");
        let produced = root.join("modules");
        std::fs::write(&packed, lzma_blob(BODY)).expect("write fixture");

        let (sha1, size) = expand(Codec::Lzma, &packed, &produced).expect("expand");
        assert_eq!(std::fs::read(&produced).expect("output"), BODY);
        assert_eq!(sha1, sha1_hex(BODY));
        assert_eq!(size, BODY.len() as u64);

        // 不是压缩包的东西要报错，而不是写出半份垃圾还说成功。
        std::fs::write(&packed, b"not lzma at all").expect("write garbage");
        assert!(expand(Codec::Lzma, &packed, &produced).is_err());

        std::fs::remove_dir_all(root).ok();
    }

    /// 对账那一遍在下载那道闸的**外面**，而且认账：已经算数的文件一个请求都不发。
    #[tokio::test]
    async fn files_that_already_check_out_never_reach_the_network() {
        const BODY: &[u8] = b"fern";
        const SHA1: &str = "654edb122a04602f918500d59b1d6fc37b9d0c01";
        let root = scratch("settled");
        let mut tasks = Vec::new();
        for index in 0..8 {
            let path = root.join(format!("library-{index}.jar"));
            std::fs::write(&path, BODY).expect("write");
            // 解析得了但连不上的地址：真发了请求，测试会失败而不是变慢。
            tasks.push(
                DownloadTask::new(path, "https://127.0.0.1:1/never", SHA1, BODY.len() as u64)
                    .expect("build task"),
            );
        }

        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        DownloadClient::new(vec![Arc::new(OfficialSource)], 4)
            .download_all(tasks, &events)
            .await
            .expect("everything was already on disk");
        drop(events);

        let done = std::iter::from_fn(|| received.try_recv().ok())
            .filter(|event| matches!(event, DownloadEvent::FileDone { .. }))
            .count();
        assert_eq!(done, 8, "跳过的文件也要报完成，不然界面的计数停在半路");
        std::fs::remove_dir_all(root).ok();
    }

    /// 目的地上已经有一份别的东西时，下完要把它换掉。
    ///
    /// 上一版是「先 stat 问在不在、在就删、再 rename」。现在是「先 rename、失败
    /// 了再删了重来」——省下的那一次 stat 要乘以四千个资源文件，而覆盖这件事
    /// 必须照样成立。
    #[tokio::test]
    async fn a_finished_download_replaces_whatever_was_in_the_way() {
        const BODY: &[u8] = b"fern";
        let url = flaky_server(0, BODY, "200 OK").await;
        let root = scratch("replace");
        let path = root.join("client.jar");
        std::fs::write(&path, b"a stale file of some other length").expect("write stale");

        let task = DownloadTask::new(
            &path,
            url.as_str(),
            "654edb122a04602f918500d59b1d6fc37b9d0c01",
            BODY.len() as u64,
        )
        .expect("build task");
        let (events, _received) = tokio::sync::mpsc::unbounded_channel();
        DownloadClient::new(vec![Arc::new(OfficialSource)], 4)
            .download_all(vec![task], &events)
            .await
            .expect("download");

        assert_eq!(std::fs::read(&path).expect("replaced file"), BODY);
        std::fs::remove_dir_all(root).ok();
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
