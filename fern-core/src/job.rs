//! 作业：一件耗时的、界面要能一直看见的事。
//!
//! 上一版只有一条不带身份的下载事件流，于是「谁在下东西」这个问题没有答案：
//! 补全游戏文件和装一个模组同时发生时，两份进度互相覆盖；装模组的进度压根
//! 没人显示，因为界面里唯一那个进度条长在启动按钮上。
//!
//! 作业解决的就是身份问题。每件事有一个 id，从头到尾贴在它自己的每一条事件
//! 上；界面据此把它们分开，也据此知道某个实例、某个项目上现在有什么在跑。
//!
//! **进度分两轴，不压成一个百分比。** 一次补全里，装 Forge 那一步要在本地跑
//! 一个第三方安装器，它根本没有百分比可言；硬给它编一个就是骗人。所以纵轴是
//! 「第几步 / 共几步」，横轴才是这一步内部的字节数——没有字节数的步骤就老实
//! 说不知道。
//!
//! 什么该是作业，判据三条，缺一不可：生命周期比发起它的界面长；有中间状态
//! 可说；失败了需要被接住。搜索、读详情、改个设置都不满足——那些是 async 加
//! 一个局部的加载状态就够了的事。

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use fern_download::DownloadEvent;

use crate::LauncherEvent;

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
        label: String,
        index: u8,
        of: u8,
    },
    /// 这一步内部的字节进度。`total` 为 0 表示不定量。
    Bytes {
        id: String,
        done: u64,
        total: u64,
        speed: u64,
    },
    /// 收工。`error` 有值就是失败了——失败的作业不会自己消失。
    Done { id: String, error: Option<String> },
}

/// 作业 id 只要在这一次运行里唯一：界面重载就全忘了，作业也活不过进程。
fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("job-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// 一个正在进行的作业。
///
/// 拿着它的那一方负责推进度；它被丢掉的时候如果还没收工，会自己发一条失败的
/// `Done`。界面是纯投影，没有超时也没有心跳——所以「开了工却没有下文」的作业
/// 必须由这一侧兜住，否则岛上会永远挂着一个不动的东西。
pub struct Job {
    id: String,
    events: UnboundedSender<LauncherEvent>,
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
            id: id.clone(),
            events: events.clone(),
            of: Arc::new(AtomicU8::new(0)),
            index: Arc::new(AtomicU8::new(0)),
            finished: AtomicBool::new(false),
        };
        job.send(JobEvent::Started {
            id,
            title: title.into(),
            subjects,
        });
        job
    }

    pub fn id(&self) -> &str {
        &self.id
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
    pub fn step(&self, label: impl Into<String>) {
        let index = self.index.fetch_add(1, Ordering::Relaxed) + 1;
        self.send(JobEvent::Stage {
            id: self.id.clone(),
            label: label.into(),
            index,
            of: self.of.load(Ordering::Relaxed),
        });
    }

    /// 收工。成功传 `None`，失败传原因。
    pub fn done(&self, error: Option<String>) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }
        self.send(JobEvent::Done {
            id: self.id.clone(),
            error,
        });
    }

    /// 照结果收工。命令层拿到的就是一个 `Result`，不必自己拆一遍。
    pub fn finish<T, E: std::fmt::Display>(&self, outcome: &Result<T, E>) {
        self.done(outcome.as_ref().err().map(|error| error.to_string()));
    }

    /// 一条把下载事件贴上这个作业 id 的通道。
    ///
    /// 下载器只认得 [`DownloadEvent`]，也不该去认识作业——桥搭在这里一次，
    /// 比让下载器知道自己在为谁干活好。翻译规则：
    ///
    /// - `Status` 是这一步内部的细节，所以沿用当前步号，只换说法；
    /// - `TaskStarted` 只是记下总字节数，本身没什么可说的；
    /// - `FileDone` 太碎，`TaskFinished` 由作业自己的 `Done` 交代。
    pub fn downloads(&self) -> UnboundedSender<DownloadEvent> {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = self.events.clone();
        let id = self.id.clone();
        let of = self.of.clone();
        let index = self.index.clone();
        let total = Arc::new(AtomicU64::new(0));
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                let translated = match event {
                    DownloadEvent::Status { message } => JobEvent::Stage {
                        id: id.clone(),
                        label: message,
                        index: index.load(Ordering::Relaxed).max(1),
                        of: of.load(Ordering::Relaxed),
                    },
                    DownloadEvent::TaskStarted { total_bytes, .. } => {
                        total.store(total_bytes, Ordering::Relaxed);
                        continue;
                    }
                    DownloadEvent::Progress {
                        done_bytes,
                        speed_bps,
                    } => JobEvent::Bytes {
                        id: id.clone(),
                        done: done_bytes,
                        total: total.load(Ordering::Relaxed),
                        speed: speed_bps,
                    },
                    DownloadEvent::FileDone { .. } | DownloadEvent::TaskFinished { .. } => continue,
                };
                if events.send(LauncherEvent::Job(translated)).is_err() {
                    break;
                }
            }
        });
        sender
    }

    fn send(&self, event: JobEvent) {
        let _ = self.events.send(LauncherEvent::Job(event));
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.done(Some("任务没有正常结束".to_owned()));
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
        job.step("读取版本信息");
        // 每一段各报各的，加起来才是总数。
        job.expect(1);
        job.expect(2);
        job.step("补全游戏文件");
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
        }))
        .expect("serialize");
        assert_eq!(value["type"], "job");
        assert_eq!(value["payload"]["type"], "bytes");
        assert_eq!(value["payload"]["payload"]["done"], 41);

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
    async fn download_progress_arrives_stamped_with_the_job_and_its_step() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = Job::begin(&sender, "准备 Sundial", Vec::new());
        job.expect(2);
        job.step("补全游戏文件");

        let downloads = job.downloads();
        let _ = downloads.send(DownloadEvent::TaskStarted {
            total_files: 3,
            total_bytes: 900,
        });
        let _ = downloads.send(DownloadEvent::Progress {
            done_bytes: 300,
            speed_bps: 100,
        });
        let _ = downloads.send(DownloadEvent::Status {
            message: "读取资源索引".to_owned(),
        });
        // 桥是一个独立任务，给它一次调度机会。
        tokio::task::yield_now().await;

        let events = drain(&mut receiver);
        let bytes = events
            .iter()
            .find(|event| matches!(event, JobEvent::Bytes { .. }))
            .expect("progress becomes bytes");
        assert_eq!(
            bytes,
            &JobEvent::Bytes {
                id: job.id().to_owned(),
                done: 300,
                total: 900,
                speed: 100,
            }
        );
        // 下载器报的细节是这一步内部的，不该让步号往前跳。
        let note = events
            .iter()
            .rfind(|event| matches!(event, JobEvent::Stage { .. }))
            .expect("status becomes a stage note");
        assert!(matches!(
            note,
            JobEvent::Stage {
                index: 1,
                of: 2,
                label,
                ..
            } if label == "读取资源索引"
        ));
    }
}
