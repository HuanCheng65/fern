//! 作业：一件耗时的、界面要能一直看见的事。
//!
//! 上一版只有一条不带身份的下载事件流，于是「谁在下东西」这个问题没有答案：
//! 补全游戏文件和装一个模组同时发生时，两份进度互相覆盖；装模组的进度压根
//! 没人显示，因为界面里唯一那个进度条长在启动按钮上。
//!
//! 作业解决的就是身份问题。每件事有一个 id，从头到尾贴在它自己的每一条事件
//! 上；界面据此把它们分开，也据此知道某个实例、某个项目上现在有什么在跑。
//!
//! **进度分两轴，不压成一个百分比。** 一次补全里，装 Forge 那一条支线要在本地
//! 跑一个第三方安装器，它根本没有百分比可言；硬给它编一个就是骗人。所以纵轴是
//! 「第几步 / 共几步」，横轴才是字节数——没有字节数的那些就老实说不知道。
//!
//! **步是阶段，细节是注脚。** 上一版把下载器的每一句话（「读取资源索引」）都
//! 当成阶段名顶上去，而且顶上去就不撤——整批下载的几分钟里界面一直写着一件
//! 早就做完的事。现在阶段名只由 `step()` 改，细节走 `Note`，说完就撤。
//!
//! **字节是一本账，不是最后一批的读数。** 一次作业里可以有好几条下载流（补全
//! 一条、装加载器一条、刷新账户一条），每条流在账本上有自己的一页，界面看到
//! 的是合计。上一版谁最后开工谁说了算，分母来回跳。
//!
//! 什么该是作业，判据三条，缺一不可：生命周期比发起它的界面长；有中间状态
//! 可说；失败了需要被接住。搜索、读详情、改个设置都不满足——那些是 async 加
//! 一个局部的加载状态就够了的事。

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::Instant,
};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use fern_download::DownloadEvent;

use crate::LauncherEvent;

/// 一句给用户看的话：要么是文案 id 加参数（句子在界面的文案表里），要么是
/// 一段现成的文本。
///
/// 后者是搬迁期的退路，不是常态——面向用户的新文案不写在 Rust 里（见
/// AGENTS.md）。启动链路已经全部走 id；还没搬的调用点传字符串，编译不拦，
/// 界面原样显示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JobText {
    Message {
        id: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        params: BTreeMap<String, String>,
    },
    Plain(String),
}

impl JobText {
    pub fn id(id: impl Into<String>) -> Self {
        Self::Message {
            id: id.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Self::Message { params, .. } = &mut self {
            params.insert(key.into(), value.into());
        }
        self
    }

    /// 撤下细节用的空文本。
    fn empty() -> Self {
        Self::Plain(String::new())
    }
}

impl From<&str> for JobText {
    fn from(text: &str) -> Self {
        Self::Plain(text.to_owned())
    }
}

impl From<String> for JobText {
    fn from(text: String) -> Self {
        Self::Plain(text)
    }
}

/// Job 事件里会用到的全部文案 id。
///
/// 这份清单进 `message_ids()`（见 lib.rs），也就是进了与界面的契约：文案表里
/// 少一条是编译错误。新用一个 id 必须同时写进这里——否则界面只能把 id 本身
/// 显示出来。
pub(crate) const TEXT_IDS: &[&str] = &[
    "job.note.adopt-progress",
    "job.note.asset-index",
    "job.note.authlib",
    "job.note.downloading",
    "job.note.forge-core",
    "job.note.forge-libraries",
    "job.note.forge-processor",
    "job.note.java-adoptium-download",
    "job.note.java-adoptium-query",
    "job.note.java-download",
    "job.note.java-extract",
    "job.note.java-prepare",
    "job.note.legacy-assets",
    "job.note.loader-inspect",
    "job.note.loader-profile",
    "job.note.retry",
    "job.stage.adopt",
    "job.stage.download-files",
    "job.stage.prepare-launch",
    "job.stage.resolve-version",
    "job.track.account",
    "job.track.download",
    "job.track.install-loader",
    "job.track.java-runtime",
    "job.track.mods",
    "job.track.natives",
    "job.track.snapshot",
];

/// 作业的一生。类型标签 snake_case、数据字段 camelCase，和别的事件同一条规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum JobEvent {
    /// 开工。`subjects` 是这件事干在谁身上——实例 id、项目 id，可以都有。
    /// 界面靠它把作业挂到对应的页面上，而不必认识作业的种类。
    Started {
        id: String,
        title: String,
        subjects: Vec<String>,
    },
    /// 到第几步了。`of` 为 0 表示总步数还不知道。
    Stage {
        id: String,
        label: JobText,
        index: u8,
        of: u8,
    },
    /// 阶段内的一条并行支线开始了。支线各自有名字，完成后从界面上消失；
    /// 阶段在所有支线汇合后才推进。
    Track {
        id: String,
        track: u32,
        label: JobText,
    },
    /// 此刻的细节，说完就撤（空文本表示撤下）。它只是注脚，永远不改阶段名。
    Note {
        id: String,
        track: u32,
        message: JobText,
    },
    /// 一条支线收工。
    TrackDone { id: String, track: u32 },
    /// 字节账本的合计：这次作业到现在一共下了多少、还知道要下多少。
    /// `total` 为 0 表示不定量。
    ///
    /// `tracks` 是账本上有名字的那几页，界面据此给每条支线画自己的进度——
    /// 只有合计的话，三条并排的支线在界面上就是三行没有长度的字。匿名支线
    /// （下载器的那些桥）不在里面：它们在界面上本来就不占一行。
    Bytes {
        id: String,
        done: u64,
        total: u64,
        speed: u64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tracks: Vec<TrackBytes>,
    },
    /// 收工。`error` 有值就是失败了——失败的作业不会自己消失。
    Done { id: String, error: Option<String> },
}

/// 一条有名字的支线在账本上那一页。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackBytes {
    pub track: u32,
    pub done: u64,
    pub total: u64,
}

/// 作业 id 只要在这一次运行里唯一：界面重载就全忘了，作业也活不过进程。
fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("job-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// 进度事件的最小间隔。几条下载流同时记账时，不限流会把 IPC 打满。
const BYTES_INTERVAL_MS: u64 = 100;

/// Job 与它的支线共享的那部分：身份、事件口、字节账本。
struct Shared {
    id: String,
    events: UnboundedSender<LauncherEvent>,
    /// 每条支线在账本上的一页。支线完成后这一页留着，所以合计在整个作业里
    /// 单调向上，不会因为一批下完就往回跳。
    ledger: Mutex<Vec<Page>>,
    started: Instant,
    last_emit_ms: AtomicU64,
    last_done: AtomicU64,
    speed: AtomicU64,
    next_track: AtomicU32,
}

/// 账本上的一页。`named` 决定它要不要单独报出去——匿名支线在界面上不占一行。
#[derive(Debug, Clone, Copy, Default)]
struct Page {
    done: u64,
    total: u64,
    named: bool,
}

impl Shared {
    fn send(&self, event: JobEvent) {
        let _ = self.events.send(LauncherEvent::Job(event));
    }

    fn note(&self, track: u32, message: JobText) {
        self.send(JobEvent::Note {
            id: self.id.clone(),
            track,
            message,
        });
    }

    /// 记一页账，并把合计发出去。
    fn record(&self, track: u32, done: u64, total: u64, force: bool) {
        {
            let Ok(mut ledger) = self.ledger.lock() else {
                return;
            };
            if let Some(page) = ledger.get_mut(track as usize) {
                page.done = done;
                page.total = total;
            }
        }
        self.emit_bytes(force);
    }

    fn emit_bytes(&self, force: bool) {
        let now = self.started.elapsed().as_millis() as u64;
        let previous = self.last_emit_ms.load(Ordering::Relaxed);
        if !force {
            if now.saturating_sub(previous) < BYTES_INTERVAL_MS {
                return;
            }
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

        let (done, total, tracks) = {
            let Ok(ledger) = self.ledger.lock() else {
                return;
            };
            let mut tracks = Vec::new();
            let mut done = 0u64;
            let mut total = 0u64;
            for (index, page) in ledger.iter().enumerate() {
                done += page.done;
                total += page.total;
                if page.named && page.total > 0 {
                    tracks.push(TrackBytes {
                        track: index as u32,
                        done: page.done,
                        total: page.total,
                    });
                }
            }
            (done, total, tracks)
        };

        // 速度从合计的增量算，平滑一下。全程平均不行：开头一批本地命中的
        // 文件会把速度顶到几 GB/s。回退（重试退账）不是速度，跳过那一窗。
        let elapsed = now.saturating_sub(previous);
        let last = self.last_done.swap(done, Ordering::Relaxed);
        if force && done == last {
            // 一批下完了，而这一刻没有任何字节在动。留着上一个读数的话，界面
            // 上会挂着一行永远不变的「12 MB/s」——那正是「卡住没反馈」的样子，
            // 而这时候真正在干活的是别的支线，该由它们说话。
            self.speed.store(0, Ordering::Relaxed);
        } else if done >= last && elapsed >= BYTES_INTERVAL_MS {
            let instant = (done - last).saturating_mul(1000) / elapsed.max(1);
            let previous_speed = self.speed.load(Ordering::Relaxed);
            let smoothed = if previous_speed == 0 {
                instant
            } else {
                (previous_speed.saturating_mul(7) + instant.saturating_mul(3)) / 10
            };
            self.speed.store(smoothed, Ordering::Relaxed);
        }

        self.send(JobEvent::Bytes {
            id: self.id.clone(),
            done,
            total,
            speed: self.speed.load(Ordering::Relaxed),
            tracks,
        });
    }
}

/// 一个正在进行的作业。
///
/// 拿着它的那一方负责推进度；它被丢掉的时候如果还没收工，会自己发一条失败的
/// `Done`。界面是纯投影，没有超时也没有心跳——所以「开了工却没有下文」的作业
/// 必须由这一侧兜住，否则岛上会永远挂着一个不动的东西。
pub struct Job {
    shared: Arc<Shared>,
    /// 总步数。开工时未必知道（补全要读完实例配置才知道用不用装加载器），
    /// 所以是跑起来之后才填的。
    of: Arc<AtomicU8>,
    index: Arc<AtomicU8>,
    finished: AtomicBool,
}

impl Job {
    pub fn begin(
        events: &UnboundedSender<LauncherEvent>,
        title: impl Into<String>,
        subjects: Vec<String>,
    ) -> Self {
        let id = next_id();
        let job = Self {
            shared: Arc::new(Shared {
                id: id.clone(),
                events: events.clone(),
                ledger: Mutex::new(Vec::new()),
                started: Instant::now(),
                last_emit_ms: AtomicU64::new(0),
                last_done: AtomicU64::new(0),
                speed: AtomicU64::new(0),
                next_track: AtomicU32::new(0),
            }),
            of: Arc::new(AtomicU8::new(0)),
            index: Arc::new(AtomicU8::new(0)),
            finished: AtomicBool::new(false),
        };
        job.shared.send(JobEvent::Started {
            id,
            title: title.into(),
            subjects,
        });
        job
    }

    pub fn id(&self) -> &str {
        &self.shared.id
    }

    /// 往总步数里添上自己这一段。
    ///
    /// 和 `begin` 分开，是因为步数常常要开工之后才知道——补全得先读到实例
    /// 配置，才知道要不要多一步装加载器。在这之前发出去的 `Stage` 里 `of`
    /// 是 0，界面照实只说这一步在做什么，不编一个假的分母。
    ///
    /// 累加而不是覆盖，是为了让一次点击串起来的几段各报各的：启动那条路上
    /// 启动报 1、补全报 4，合起来就是 5 步，谁都不必知道别人有几步。
    pub fn expect(&self, steps: u8) {
        self.of.fetch_add(steps, Ordering::Relaxed);
    }

    /// 进入下一步。
    pub fn step(&self, label: impl Into<JobText>) {
        let index = self.index.fetch_add(1, Ordering::Relaxed) + 1;
        self.shared.send(JobEvent::Stage {
            id: self.shared.id.clone(),
            label: label.into(),
            index,
            of: self.of.load(Ordering::Relaxed),
        });
    }

    /// 开一条有名字的支线。它出现在界面上，完成后消失；并行的几件事各开
    /// 各的，互不覆盖。
    pub fn track(&self, label: impl Into<JobText>) -> Track {
        self.open_track(Some(label.into()))
    }

    /// 只在此刻的细节。走当前作业的匿名支线，不新增一行。
    pub fn note(&self, message: impl Into<JobText>) {
        self.shared.note(u32::MAX, message.into());
    }

    fn open_track(&self, label: Option<JobText>) -> Track {
        let index = self.shared.next_track.fetch_add(1, Ordering::Relaxed);
        let announced = label.is_some();
        if let Ok(mut ledger) = self.shared.ledger.lock() {
            // 并行开线时拿号和拿锁的先后可能错开，只许把账本变长。
            if ledger.len() <= index as usize {
                ledger.resize((index as usize) + 1, Page::default());
            }
            ledger[index as usize].named = announced;
        }
        if let Some(label) = label {
            self.shared.send(JobEvent::Track {
                id: self.shared.id.clone(),
                track: index,
                label,
            });
        }
        Track {
            shared: self.shared.clone(),
            index,
            announced,
            finished: AtomicBool::new(false),
        }
    }

    /// 收工。成功传 `None`，失败传原因。
    pub fn done(&self, error: Option<String>) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }
        self.shared.send(JobEvent::Done {
            id: self.shared.id.clone(),
            error,
        });
    }

    /// 照结果收工。命令层拿到的就是一个 `Result`，不必自己拆一遍。
    pub fn finish<T, E: std::fmt::Display>(&self, outcome: &Result<T, E>) {
        self.done(outcome.as_ref().err().map(|error| error.to_string()));
    }

    /// 一条把下载事件记到这个作业账上的通道（匿名支线）。
    ///
    /// 界面上不多一行，字节照记。给并行的、值得有名字的下载流用
    /// [`Job::track`] 加 [`Track::downloads`]。
    pub fn downloads(&self) -> UnboundedSender<DownloadEvent> {
        self.open_track(None).into_downloads()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.done(Some("任务没有正常结束".to_owned()));
    }
}

/// 阶段里的一条支线。
///
/// 丢掉即收工——支线的失败不单独上报，该失败的是作业本身；这里只负责让
/// 界面上那一行消失。
pub struct Track {
    shared: Arc<Shared>,
    index: u32,
    announced: bool,
    finished: AtomicBool,
}

impl Track {
    /// 这条支线此刻的细节。
    pub fn note(&self, message: impl Into<JobText>) {
        self.shared.note(self.index, message.into());
    }

    /// 收工。之后这条支线从界面上消失，但它的账留在合计里。
    pub fn done(&self) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }
        if self.announced {
            self.shared.send(JobEvent::TrackDone {
                id: self.shared.id.clone(),
                track: self.index,
            });
        }
    }

    /// 一条把下载事件记到这条支线账上的通道。
    ///
    /// 下载器只认得 [`DownloadEvent`]，也不该去认识作业——桥搭在这里一次，
    /// 比让下载器知道自己在为谁干活好。翻译规则：
    ///
    /// - `Status` / `StatusId` / `Retrying` 是细节，翻成 `Note`，永不碰阶段名；
    /// - `TaskStarted` 开一批新账（同一条通道可以连着跑好几批，账目累加），
    ///   并把「检查并下载 N 个文件」写成细节；
    /// - `TaskFinished` 把这批的细节撤下来；
    /// - `Progress` 记账；`FileDone` 太碎，不理。
    pub fn downloads(&self) -> UnboundedSender<DownloadEvent> {
        Track {
            shared: self.shared.clone(),
            index: self.index,
            announced: false,
            finished: AtomicBool::new(false),
        }
        .into_downloads()
    }

    fn into_downloads(self) -> UnboundedSender<DownloadEvent> {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = self.shared.clone();
        let track = self.index;
        tokio::spawn(async move {
            // 同一条通道跑好几批时（补全流程里至少两批），前几批的读数折进
            // 底账，当前这批的读数摞在上面——合计永远不回跳。
            let mut done_base = 0u64;
            let mut total_base = 0u64;
            let mut current = (0u64, 0u64);
            let mut opened = false;
            while let Some(event) = receiver.recv().await {
                match event {
                    DownloadEvent::Status { message } => shared.note(track, JobText::from(message)),
                    DownloadEvent::StatusId { id, params } => shared.note(
                        track,
                        JobText::Message {
                            id,
                            params: params.into_iter().collect(),
                        },
                    ),
                    DownloadEvent::Retrying { files } => shared.note(
                        track,
                        JobText::id("job.note.retry").arg("count", files.to_string()),
                    ),
                    DownloadEvent::TaskStarted {
                        total_files,
                        total_bytes,
                    } => {
                        if opened {
                            done_base += current.0;
                            total_base += current.1;
                        }
                        opened = true;
                        current = (0, total_bytes);
                        shared.record(track, done_base, total_base + current.1, true);
                        shared.note(
                            track,
                            JobText::id("job.note.downloading")
                                .arg("count", total_files.to_string()),
                        );
                    }
                    DownloadEvent::Progress {
                        done_bytes,
                        total_bytes,
                        ..
                    } => {
                        current = (done_bytes, total_bytes);
                        shared.record(track, done_base + current.0, total_base + current.1, false);
                    }
                    DownloadEvent::TaskFinished { .. } => {
                        shared.record(track, done_base + current.0, total_base + current.1, true);
                        // 这批说的话已经过时了。
                        shared.note(track, JobText::empty());
                    }
                    DownloadEvent::FileDone { .. } => {}
                }
            }
        });
        sender
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        self.done();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<LauncherEvent>) -> Vec<JobEvent> {
        let mut events = Vec::new();
        while let Ok(LauncherEvent::Job(event)) = receiver.try_recv() {
            events.push(event);
        }
        events
    }

    #[test]
    fn steps_count_up_and_carry_the_total_once_it_is_known() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", vec!["abc".to_owned()]);
        job.step(JobText::id("job.stage.resolve-version"));
        // 每一段各报各的，加起来才是总数。
        job.expect(1);
        job.expect(2);
        job.step(JobText::id("job.stage.download-files"));
        job.done(None);

        let events = drain(&mut receiver);
        assert_eq!(
            events[0],
            JobEvent::Started {
                id: job.id().to_owned(),
                title: "准备 Sundial".to_owned(),
                subjects: vec!["abc".to_owned()],
            }
        );
        // 步数还不知道时 of 是 0，界面照实只说这一步在干什么。
        assert!(matches!(
            &events[1],
            JobEvent::Stage {
                index: 1,
                of: 0,
                ..
            }
        ));
        assert!(matches!(
            &events[2],
            JobEvent::Stage {
                index: 2,
                of: 3,
                ..
            }
        ));
        assert!(matches!(&events[3], JobEvent::Done { error: None, .. }));
    }

    /// 界面按 `payload.payload` 两层拆这条事件。少一层多一层编译期都看不见，
    /// 只会表现成「进度条永远不动」——那正是最难查的一类。
    #[test]
    fn job_events_reach_the_frontend_in_the_shape_it_destructures() {
        let value = serde_json::to_value(LauncherEvent::Job(JobEvent::Bytes {
            id: "job-1".to_owned(),
            done: 41,
            total: 50,
            speed: 900,
            tracks: vec![TrackBytes {
                track: 2,
                done: 41,
                total: 50,
            }],
        }))
        .expect("serialize");
        assert_eq!(value["type"], "job");
        assert_eq!(value["payload"]["type"], "bytes");
        assert_eq!(value["payload"]["payload"]["done"], 41);
        // 支线那几页也走 camelCase，和别的字段同一条规则。
        assert_eq!(value["payload"]["payload"]["tracks"][0]["track"], 2);
        assert_eq!(value["payload"]["payload"]["tracks"][0]["total"], 50);

        let started = serde_json::to_value(LauncherEvent::Job(JobEvent::Started {
            id: "job-2".to_owned(),
            title: "安装 Sodium".to_owned(),
            subjects: vec!["sodium".to_owned()],
        }))
        .expect("serialize");
        assert_eq!(started["payload"]["type"], "started");
        assert_eq!(started["payload"]["payload"]["subjects"][0], "sodium");

        // 成功时 error 是 null，界面据此判断该不该把它留下来。
        let done = serde_json::to_value(LauncherEvent::Job(JobEvent::Done {
            id: "job-2".to_owned(),
            error: None,
        }))
        .expect("serialize");
        assert!(done["payload"]["payload"]["error"].is_null());
    }

    /// 文案有两种形状：id 加参数序列化成对象，现成的句子序列化成字符串。
    /// 界面按 `typeof` 区分，两边的形状都在这里钉死。
    #[test]
    fn job_text_serializes_ids_as_objects_and_plain_text_as_strings() {
        let message =
            serde_json::to_value(JobText::id("job.stage.install-loader").arg("loader", "Fabric"))
                .expect("serialize");
        assert_eq!(message["id"], "job.stage.install-loader");
        assert_eq!(message["params"]["loader"], "Fabric");

        let plain = serde_json::to_value(JobText::from("解析依赖")).expect("serialize");
        assert_eq!(plain, "解析依赖");

        // 没有参数时不带空的 params 字段。
        let bare = serde_json::to_value(JobText::id("job.note.asset-index")).expect("serialize");
        assert!(bare.get("params").is_none());
    }

    #[test]
    fn finishing_twice_only_says_so_once() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "装 Sodium", Vec::new());
        job.done(None);
        job.done(Some("这条不该发出去".to_owned()));

        let events = drain(&mut receiver);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, JobEvent::Done { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn a_dropped_job_reports_itself_as_failed() {
        // 界面是纯投影：没人替它清理开了工却没下文的作业，所以这一侧必须兜住。
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(Job::begin(&sender, "装整合包", Vec::new()));

        let events = drain(&mut receiver);
        assert!(matches!(
            events.last(),
            Some(JobEvent::Done { error: Some(_), .. })
        ));
    }

    #[tokio::test]
    async fn download_details_become_notes_and_never_touch_the_stage() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        job.expect(2);
        job.step(JobText::id("job.stage.download-files"));

        let downloads = job.downloads();
        let _ = downloads.send(DownloadEvent::TaskStarted {
            total_files: 3,
            total_bytes: 900,
        });
        let _ = downloads.send(DownloadEvent::Progress {
            done_bytes: 300,
            total_bytes: 900,
            speed_bps: 100,
        });
        // 批次收尾是必须看得见的时刻，不受限流影响。
        let _ = downloads.send(DownloadEvent::TaskFinished { failed: Vec::new() });
        let _ = downloads.send(DownloadEvent::Status {
            message: "读取资源索引".to_owned(),
        });
        // 桥是一个独立任务，给它一次调度机会。
        tokio::task::yield_now().await;

        let events = drain(&mut receiver);
        let bytes = events
            .iter()
            .rfind(|event| matches!(event, JobEvent::Bytes { .. }))
            .expect("progress becomes bytes");
        assert!(matches!(
            bytes,
            JobEvent::Bytes {
                done: 300,
                total: 900,
                ..
            }
        ));
        // 下载器说的细节是注脚，不是阶段名——阶段名只有 step() 能改。
        // 上一版这里断言的是相反的行为，正是「下载半天还写着读取资源索引」
        // 的病根。
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, JobEvent::Stage { .. }))
                .count(),
            1
        );
        let note = events
            .iter()
            .rfind(|event| matches!(event, JobEvent::Note { .. }))
            .expect("status becomes a note");
        assert!(matches!(
            note,
            JobEvent::Note { message: JobText::Plain(text), .. } if text == "读取资源索引"
        ));
    }

    #[tokio::test]
    async fn the_ledger_adds_up_across_streams_and_batches() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        job.expect(2);
        job.step(JobText::id("job.stage.download-files"));

        // 同一条通道连着两批：第二批开始后，第一批的读数折进底账。
        let first = job.downloads();
        let _ = first.send(DownloadEvent::TaskStarted {
            total_files: 2,
            total_bytes: 600,
        });
        let _ = first.send(DownloadEvent::Progress {
            done_bytes: 600,
            total_bytes: 600,
            speed_bps: 0,
        });
        let _ = first.send(DownloadEvent::TaskFinished { failed: Vec::new() });
        let _ = first.send(DownloadEvent::TaskStarted {
            total_files: 1,
            total_bytes: 100,
        });
        let _ = first.send(DownloadEvent::Progress {
            done_bytes: 40,
            total_bytes: 100,
            speed_bps: 0,
        });
        tokio::task::yield_now().await;

        // 另一条流（比如刷新账户）在自己的页上记账，合计包含两边。
        let second = job.downloads();
        let _ = second.send(DownloadEvent::TaskStarted {
            total_files: 1,
            total_bytes: 50,
        });
        let _ = second.send(DownloadEvent::Progress {
            done_bytes: 50,
            total_bytes: 50,
            speed_bps: 0,
        });
        let _ = second.send(DownloadEvent::TaskFinished { failed: Vec::new() });
        tokio::task::yield_now().await;

        let events = drain(&mut receiver);
        let last = events
            .iter()
            .filter_map(|event| match event {
                JobEvent::Bytes { done, total, .. } => Some((*done, *total)),
                _ => None,
            })
            .next_back()
            .expect("bytes were reported");
        assert_eq!(last, (600 + 40 + 50, 600 + 100 + 50));
    }

    /// 并排的几条支线在界面上各占一行，各自要有自己的进度——只有合计的话，
    /// 三行字里没有一行说得出自己走到哪了。匿名支线不报：它们不占那一行。
    #[tokio::test]
    async fn named_tracks_report_their_own_page_and_anonymous_ones_do_not() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        let files = job.track(JobText::id("job.track.download"));
        let anonymous = job.downloads();
        let file_events = files.downloads();
        let _ = file_events.send(DownloadEvent::TaskStarted {
            total_files: 1,
            total_bytes: 800,
        });
        let _ = file_events.send(DownloadEvent::Progress {
            done_bytes: 200,
            total_bytes: 800,
            speed_bps: 0,
        });
        let _ = anonymous.send(DownloadEvent::TaskStarted {
            total_files: 1,
            total_bytes: 50,
        });
        tokio::task::yield_now().await;

        let events = drain(&mut receiver);
        let tracks = events
            .iter()
            .filter_map(|event| match event {
                JobEvent::Bytes { tracks, .. } if !tracks.is_empty() => Some(tracks.clone()),
                _ => None,
            })
            .next_back()
            .expect("named pages are reported");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].total, 800);
    }

    /// 一批下完之后速度要归零。
    ///
    /// 速度只在有事件的时候重算，而下完之后就没有事件了——留着最后那个读数，
    /// 界面上会挂着一行永远不变的「12 MB/s」，那正是「卡住没反馈」的样子。
    #[tokio::test]
    async fn the_speed_drops_to_zero_when_a_batch_is_done() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        let downloads = job.downloads();
        let _ = downloads.send(DownloadEvent::TaskStarted {
            total_files: 1,
            total_bytes: 900,
        });
        let _ = downloads.send(DownloadEvent::Progress {
            done_bytes: 900,
            total_bytes: 900,
            speed_bps: 1_000_000,
        });
        let _ = downloads.send(DownloadEvent::TaskFinished { failed: Vec::new() });
        tokio::task::yield_now().await;

        let events = drain(&mut receiver);
        let speed = events
            .iter()
            .filter_map(|event| match event {
                JobEvent::Bytes { speed, .. } => Some(*speed),
                _ => None,
            })
            .next_back()
            .expect("bytes were reported");
        assert_eq!(speed, 0);
    }

    #[tokio::test]
    async fn named_tracks_announce_themselves_and_their_exit() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        {
            let track = job.track(JobText::id("job.track.snapshot"));
            track.note(JobText::from("正在读取存档"));
            // 离开作用域即收工——支线的一生比它承载的那件事短不了。
        }
        let events = drain(&mut receiver);
        let track_started = events
            .iter()
            .find_map(|event| match event {
                JobEvent::Track { track, .. } => Some(*track),
                _ => None,
            })
            .expect("track announced");
        assert!(
            events.iter().any(
                |event| matches!(event, JobEvent::Note { track, .. } if *track == track_started)
            )
        );
        assert!(events.iter().any(
            |event| matches!(event, JobEvent::TrackDone { track, .. } if *track == track_started)
        ));
    }
}
