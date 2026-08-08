//! 启动层：从「点了启动」到「进程跑起来」之间的一切。
//!
//! 文件的顺序就是事情发生的顺序：`prepare` 补全文件（加载器那一段交给
//! `loader` 与 `forge`），`version` 把带 `inheritsFrom` 的几份 JSON 合成一份，
//! `rules` 决定这台机器该认哪些条目，`memory` 决定给多少内存、用什么 GC，
//! 本文件把这些拼成命令行并管住进程，游戏跑起来之后的输出归 `gamelog` 与
//! `crash`。
//!
//! **补全与启动必须读同一份合并后的元数据**（`version::resolve`）。两边各算
//! 各的，就会出现「文件明明下好了却说缺」这种最难查的问题。

pub(crate) mod crash;
pub(crate) mod forge;
pub(crate) mod gamelog;
pub(crate) mod loader;
pub(crate) mod memory;
pub(crate) mod preflight;
pub(crate) mod prepare;
pub(crate) mod ranges;
pub(crate) mod rules;
pub(crate) mod running;
pub(crate) mod version;

use std::{
    collections::HashMap,
    fs::File,
    io,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use fern_meta::{Library, RuleContext, VersionMetadata, release_ordinal};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use tokio::sync::mpsc::UnboundedSender;

use crate::{Account, DataPaths, LaunchStage, LauncherEvent, java};

use gamelog::LogParser;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
}

pub fn offline_credentials(player_name: impl Into<String>) -> Credentials {
    let player_name = player_name.into();
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{player_name}")).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Credentials {
        player_name,
        uuid: format_uuid(bytes),
        access_token: "0".to_owned(),
        user_type: "legacy".to_owned(),
    }
}

fn format_uuid(bytes: [u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchVariables {
    values: HashMap<String, String>,
}

impl LaunchVariables {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    pub fn with_credentials(self, credentials: &Credentials) -> Self {
        self.insert("auth_player_name", &credentials.player_name)
            .insert("auth_uuid", &credentials.uuid)
            .insert("auth_access_token", &credentials.access_token)
            .insert("user_type", &credentials.user_type)
    }

    pub fn substitute(&self, template: &str) -> String {
        let mut output = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("${") {
            output.push_str(&rest[..start]);
            let candidate = &rest[start + 2..];
            let Some(end) = candidate.find('}') else {
                output.push_str(&rest[start..]);
                return output;
            };
            let key = &candidate[..end];
            if let Some(value) = self.values.get(key) {
                output.push_str(value);
            } else {
                output.push_str("${");
                output.push_str(key);
                output.push('}');
            }
            rest = &candidate[end + 1..];
        }
        output.push_str(rest);
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub java_binary: PathBuf,
    pub working_directory: PathBuf,
    pub jvm_arguments: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub game_arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub instance_id: String,
    pub version_id: String,
    pub process_id: u32,
    pub java_binary: PathBuf,
    pub java_major: u16,
    pub required_java_major: Option<u16>,
    pub launch_log: PathBuf,
}

/// Build the vanilla launch command from the metadata already prepared on disk.
/// Authentication stays fully local: the offline UUID matches Minecraft's
/// canonical OfflinePlayer algorithm, so the same name remains stable across runs.
pub async fn launch_instance(
    paths: &DataPaths,
    instance_id: &str,
    quick_play: Option<crate::QuickPlay>,
    events: &UnboundedSender<LauncherEvent>,
    job: &crate::Job,
) -> Result<LaunchResult> {
    let stage = |stage: LaunchStage| {
        let _ = events.send(LauncherEvent::LaunchStage {
            instance_id: instance_id.to_owned(),
            stage,
        });
    };
    // 补全之后还剩这一步：刷新登录、组装命令、把进程拉起来。它自己没有几个
    // 字节可下（外置登录第一次要取一份 injector），但它是这次点击的最后一段，
    // 该算进「共几步」里。
    job.step("准备启动");
    stage(LaunchStage::ResolvingVersion);
    paths.ensure_exists()?;
    let profile = crate::read_instance(paths, instance_id)?;
    // 装了加载器时，要启动的是加载器生成的那份 JSON，它用 inheritsFrom 指回
    // 原版。合并在 version 模块里做一次，补全和启动用的必须是同一份——两边
    // 各算各的，就会出现「文件明明下好了却说缺」这种最难查的问题。
    let version_id = version::effective_id(&profile);
    // 快照写在 Fern 自己的数据根下，所以要留一份没被实例作用域改写过的路径。
    let launcher_paths = paths;
    // 外部实例的版本、库、游戏目录都在它自己的目录树里。这一句之后的每一个
    // `paths` 都是这个实例的那一套。
    let scoped = crate::instance::paths_for(paths, &profile);
    let paths = &scoped;
    // 同一份游戏目录跑两个进程，两边写同一批存档，而且没有任何报错。挡在最
    // 前面：后面那几步可能要几分钟，让人等完再说「不能启动」是最差的顺序。
    if let Some(occupant) = running::occupant(&paths.game_directory(instance_id)) {
        return Err(if occupant == instance_id {
            anyhow!("这个实例已经在运行")
        } else {
            anyhow!(
                "{} 正在使用同一个游戏目录，先结束它",
                crate::read_instance(paths, &occupant)
                    .map(|profile| profile.name)
                    .unwrap_or(occupant)
            )
        });
    }
    // 距上一张太久了就补一张，兜住「上次退出时崩了没拍成」。放在占用检查之后：
    // 别人正开着这个游戏目录的时候，拍到的是他正在写的文件。
    if crate::backup::due_before_launch(launcher_paths, instance_id) {
        // 第一次可能要读完整个存档，界面上不该是一段没有说明的停顿。
        job.step("拍摄快照");
        crate::backup::quietly(
            launcher_paths,
            instance_id,
            crate::SnapshotReason::BeforeLaunch,
        );
    }

    let metadata = version::resolve(paths, &version_id)
        .with_context(|| format!("读取 {version_id} 的版本描述"))?;
    // 客户端 jar 始终属于原版：加载器改的是启动方式，不是游戏本体。哪一份是
    // 原版由继承链说了算，见 version::client_jar。
    let client_jar = version::client_jar(paths, &profile);
    let main_class = metadata
        .main_class
        .clone()
        .ok_or_else(|| anyhow!("version {version_id} has no main class"))?;

    // 实例没说的跟全局默认，全局也没说的才是内置默认。求值只做这一次——启动
    // 按一份、界面显示另一份，就会出现「设置里写着 G1，实际跑的是 ZGC」。
    let effective = crate::effective_settings(
        &profile.settings,
        &crate::current_settings().game,
        memory::physical_memory_bytes(),
    );

    stage(LaunchStage::CheckingFiles);
    let context = rules::context(rules::Features {
        custom_resolution: effective.resolution.is_some(),
        quick_play: quick_play.clone(),
        ..rules::Features::default()
    });
    let natives_directory = paths.game_directory(instance_id).join("natives");
    tokio::fs::create_dir_all(&natives_directory).await?;
    let classpath =
        collect_classpath_and_extract_natives(paths, &metadata, &context, &natives_directory)
            .await?;
    if !tokio::fs::try_exists(&client_jar).await? {
        return Err(anyhow!("client jar is missing: {}", client_jar.display()));
    }

    // 这个实例记着的那一个，没记过就跟当前的走（见 accounts.rs）。
    let record = crate::account_for_instance(paths, &profile)
        .ok_or_else(|| anyhow!("尚未添加账户，请在设置中添加"))?;
    let mut account = Account::load(&record)?;
    account.ensure_fresh(paths, &job.downloads()).await?;
    // 刷新过了才记：这一刻「这个实例用这个账户」才算真的成立。之后游戏因为
    // 别的原因起不来也无所谓，身份这件事已经定了。
    if profile.account_id.as_deref() != Some(record.id.as_str()) {
        let mut updated = profile.clone();
        updated.account_id = Some(record.id.clone());
        crate::write_instance_profile(paths, &updated)?;
    }
    let credentials = account.launch_credentials()?;
    let mut variables = LaunchVariables::new().with_credentials(&credentials);
    let legacy_assets = metadata
        .asset_index
        .as_ref()
        .map(|index| paths.assets.join("virtual").join(&index.id))
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| paths.assets.clone());
    let game_directory = paths.game_directory(instance_id);
    tokio::fs::create_dir_all(&game_directory).await?;
    variables = variables
        .insert("game_directory", game_directory.to_string_lossy())
        .insert("assets_root", paths.assets.to_string_lossy())
        .insert(
            "assets_index_name",
            metadata
                .asset_index
                .as_ref()
                .map(|index| index.id.as_str())
                .unwrap_or_default(),
        )
        .insert("version_name", &version_id)
        .insert(
            "version_type",
            metadata.kind.as_deref().unwrap_or("release"),
        )
        // 1.7.3 之前用 ${game_assets} 指向一份按原名摆好的资源目录（见
        // prepare 里的 materialize_legacy_assets）。摆过就用那份，没摆过就
        // 退回资源根目录——新版本根本不读这个变量。
        .insert("game_assets", legacy_assets.to_string_lossy())
        // 同样是老版本才有的：${auth_session} 是 "token:<令牌>:<uuid>"。
        // 离线模式没有真的会话，给一个占位串，格式对得上就行。
        .insert(
            "auth_session",
            format!("token:{}:{}", credentials.access_token, credentials.uuid),
        )
        .insert("natives_directory", natives_directory.to_string_lossy())
        // NeoForge / Forge 用这两个拼模块路径（`-p ${library_directory}/…`）。
        // 少了它们，`-p` 收到的是一串没被替换的字面量，securejarhandler 根本
        // 不在模块路径上，于是后面每一条 `--add-opens …=cpw.mods.securejarhandler`
        // 都指向一个不存在的模块——报出来的是 InaccessibleObjectException，
        // 和「变量没替换」看不出任何关系。
        .insert("library_directory", paths.libraries.to_string_lossy())
        .insert(
            "classpath_separator",
            if cfg!(target_os = "windows") {
                ";"
            } else {
                ":"
            },
        )
        .insert("launcher_name", "Fern")
        .insert("launcher_version", env!("CARGO_PKG_VERSION"))
        .insert("clientid", "")
        .insert("auth_xuid", "");
    // 直接进某个世界或某个服务器。元数据里那三条参数由 feature 决定要不要
    // 出现，这里只负责把它们要的值备好——变量缺了的话参数会原样带着
    // `${...}` 进命令行，游戏收到的是一个字面上的占位符。
    if let Some(quick) = &quick_play {
        variables = variables.insert("quickPlayPath", "quickPlay/log.json");
        variables = match quick {
            crate::QuickPlay::World(name) => variables.insert("quickPlaySingleplayer", name),
            crate::QuickPlay::Server(address) => variables.insert("quickPlayMultiplayer", address),
        };
    }
    if let Some(resolution) = &effective.resolution {
        variables = variables
            .insert("resolution_width", resolution.width.to_string())
            .insert("resolution_height", resolution.height.to_string());
    }
    if let Some(logging) = metadata
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
    {
        let name = logging
            .file
            .id
            .clone()
            .unwrap_or_else(|| format!("{}.xml", logging.file.sha1));
        variables = variables.insert(
            "path",
            paths
                .assets
                .join("log_configs")
                .join(name)
                .to_string_lossy(),
        );
    }
    let (mut jvm_arguments, game_arguments) = metadata.resolved_arguments(&context);
    // Mojang 为受 Log4Shell 影响的版本准备了替换版 log4j 配置，我们一直在
    // 下载它，却从来没把启用它的那个参数加上去——文件躺在磁盘上，游戏并不
    // 知道。`argument` 是 `-Dlog4j.configurationFile=${path}`，`path` 上面
    // 刚刚指好。
    if let Some(logging) = metadata
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
    {
        jvm_arguments.push(logging.argument.clone());
    }
    jvm_arguments.extend(platform_arguments(&profile.game_version, &jvm_arguments));
    // javaagent 要排在最前：它得在游戏的任何一个类被加载之前挂上去。
    for (index, argument) in account.extra_jvm_args().into_iter().enumerate() {
        jvm_arguments.insert(index, argument);
    }
    let required_java_major = metadata
        .java_version
        .as_ref()
        .map(|version| version.major_version);
    stage(LaunchStage::PreparingJava);
    let requirement = java::requirement(&profile.game_version, profile.loader, required_java_major);
    let runtime = resolve_java_runtime(paths, &profile, &requirement)?;
    if runtime.major < requirement.minimum {
        return Err(anyhow!(
            "Java {} 无法运行 Minecraft {version_id}，此版本至少需要 Java {}（当前 {}）",
            runtime.major,
            requirement.minimum,
            runtime.version
        ));
    }
    let java_binary = runtime.path.clone();
    let java_major = runtime.major;

    // 内存与 GC 放在挑完 Java 之后：走哪条路要看 Java 大版本，给多少堆要看
    // 这个实例的 mods 目录有多大。
    let log_directory = paths.instance_log_directory(instance_id);
    std::fs::create_dir_all(&log_directory)?;
    let gc_log = log_directory.join("gc.log");
    // 判断「用户是不是已经自己表过态」时，看的必须是**元数据加用户参数**合起来
    // 的那一份。上一版先按元数据算完再把用户参数追加上去，于是用户写的
    // `-XX:+UseZGC` 和我们给的 `-XX:+UseG1GC` 一起进了命令行——JVM 直接拒绝
    // 启动，报出来只有一句「Could not create the Java Virtual Machine」。
    let declared: Vec<String> = jvm_arguments
        .iter()
        .chain(effective.jvm_arguments.iter())
        .cloned()
        .collect();
    let allocation = memory::plan(
        paths,
        &profile,
        &game_directory,
        java_major,
        effective.max_memory_mb,
        effective.memory_ceiling_mb,
        effective.garbage_collector,
        &declared,
        Some(&gc_log),
    );
    append_launch_log(
        &log_directory.join("launch.log"),
        &format!(
            "memory xmx={}M source={:?} gc={:?}",
            allocation.xmx_mb, allocation.source, allocation.gc
        ),
    )?;
    jvm_arguments.extend(allocation.arguments.iter().cloned());
    // 用户自己那几个排在最后：同一个开关出现两次时 JVM 认后面的，所以他写的
    // 永远能盖掉我们给的。
    jvm_arguments.extend(effective.jvm_arguments.iter().cloned());
    stage(LaunchStage::BuildingCommand);
    let plan = LaunchPlan {
        java_binary: java_binary.clone(),
        working_directory: game_directory,
        jvm_arguments: filter_jvm_arguments(jvm_arguments, java_major),
        classpath: classpath
            .into_iter()
            .chain(std::iter::once(client_jar))
            .collect(),
        main_class,
        game_arguments,
    };
    let java_binary = plan.java_binary.clone();
    let launch_log = log_directory.join("launch.log");
    append_launch_log(
        &launch_log,
        &format!(
            "starting version={version_id} java={} java_major={java_major} required_java_major={:?}",
            java_binary.display(),
            required_java_major
        ),
    )?;
    let arguments = plan.command_arguments(&variables);
    append_launch_log(&launch_log, &format!("arguments={arguments:?}"))?;
    let arguments = argfile_if_needed(
        arguments,
        &plan.main_class,
        java_major,
        &plan.working_directory,
    )?;
    stage(LaunchStage::StartingProcess);
    let started_at = std::time::SystemTime::now();
    let mut command = Command::new(&java_binary);
    command
        .args(arguments)
        .current_dir(&plan.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_priority(&mut command, effective.process_priority);
    let mut child = command
        .spawn()
        .with_context(|| format!("start Java from {}", java_binary.display()))?;
    append_launch_log(&launch_log, &format!("started pid={}", child.id()))?;
    // 进程起来了才算玩过。写不进去不该让已经跑起来的游戏被判失败——排序
    // 差一次，比启动被一个写盘错误打断好。
    if let Err(error) = crate::instance::catalog::touch_played(paths, instance_id) {
        append_launch_log(&launch_log, &format!("last-played not recorded: {error}"))?;
    }

    // 崩溃分析要用最后这一段，两个流写进同一个缓冲区——异常往往是 stderr
    // 的栈配上 stdout 的上下文，分开看反而少一半信息。
    let tail = Arc::new(Mutex::new(String::new()));
    // 「窗口已经开出来了」只报一次，两个读线程加一个超时兜底都可能先到。
    let running = Arc::new(AtomicBool::new(false));

    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(
            stdout,
            LogSink {
                path: launch_log.clone(),
                stream: "stdout",
                instance_id: instance_id.to_owned(),
                events: events.clone(),
                tail: tail.clone(),
                running: running.clone(),
            },
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(
            stderr,
            LogSink {
                path: launch_log.clone(),
                stream: "stderr",
                instance_id: instance_id.to_owned(),
                events: events.clone(),
                tail: tail.clone(),
                running: running.clone(),
            },
        );
    }

    // 日志里没等到窗口标志也不能一直不吭声：进程活过十五秒，就当它起来了。
    {
        let running = running.clone();
        let events = events.clone();
        let instance_id = instance_id.to_owned();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            announce_running(&running, &events, &instance_id);
        });
    }

    // 游戏跑着的时候，岛上那条细线读的就是这份日志（设计文档 §8）。它和退出
    // 之后的统计共用同一条日志流，所以这一路观察没有额外成本。
    let alive = Arc::new(AtomicBool::new(true));
    spawn_memory_watch(
        gc_log.clone(),
        allocation.xmx_mb,
        instance_id.to_owned(),
        events.clone(),
        alive.clone(),
    );

    let process_id = child.id();
    // 交给注册表之后，这个进程就是可寻址的了：能查、能停。`Child` 包在锁里
    // 是因为等待和 kill 都要碰它，而等待不能一直握着（见 running 模块）。
    let child = Arc::new(Mutex::new(child));
    running::register(
        instance_id,
        &plan.working_directory,
        process_id,
        started_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default(),
        child.clone(),
    );

    let wait_log = launch_log.clone();
    let wait_events = events.clone();
    let wait_instance = instance_id.to_owned();
    let wait_directory = plan.working_directory.clone();
    let wait_running = running.clone();
    // 崩溃分析要的上下文。在这里取，因为等待线程里没有 profile。
    let crash_loader = profile.loader;
    let crash_minecraft = profile.game_version.clone();
    let crash_mods_directory = plan.working_directory.join("mods");
    let snapshot_paths = launcher_paths.clone();
    let session = SessionRecord {
        paths: paths.clone(),
        instance_id: instance_id.to_owned(),
        modlist_hash: memory::history::modlist_hash(&plan.working_directory),
        gc_log: gc_log.clone(),
        xmx_mb: allocation.xmx_mb,
        zgc: allocation.gc.behaves_like_zgc(),
        started_at,
    };
    std::thread::spawn(move || {
        let running = wait_running;
        let exit_code = match running::wait(&child) {
            Ok(code) => {
                let text = code.map_or_else(|| "signal".to_owned(), |code| code.to_string());
                let _ = append_launch_log(&wait_log, &format!("exited code={text}"));
                code
            }
            Err(error) => {
                let _ = append_launch_log(&wait_log, &format!("wait error={error}"));
                None
            }
        };
        running::unregister(&wait_instance);

        alive.store(false, Ordering::SeqCst);
        // 抢在十五秒兜底之前把标记占掉：进程已经没了，再报一次「跑起来了」
        // 是在说一件不成立的事。实测就是这么出现的——退出之后又冒出一条
        // Running。
        running.store(true, Ordering::SeqCst);
        let _ = wait_events.send(LauncherEvent::LaunchStage {
            instance_id: wait_instance.clone(),
            stage: LaunchStage::Exited,
        });
        let _ = wait_events.send(LauncherEvent::GameExited {
            instance_id: wait_instance.clone(),
            exit_code,
        });

        let log_tail = tail.lock().map(|tail| tail.clone()).unwrap_or_default();

        // 这次跑成什么样，记一笔。下一次启动的分配就是照这些数算的。
        // 记不下来只是少学一次，绝不能影响别的任何事，所以错误只进日志。
        if let Err(error) = session.store(&log_tail) {
            let _ = append_launch_log(&wait_log, &format!("memory history not recorded: {error}"));
        }

        // 正常关掉游戏不该在界面上留下任何痕迹，崩了才需要说话。信号退出
        // （exit_code 为 None）也算——那多半是被 OOM killer 收走了。
        if exit_code != Some(0) {
            let report = crash::build_report(
                &crash::Situation {
                    instance_id: &wait_instance,
                    game_directory: &wait_directory,
                    started_at,
                    exit_code,
                    loader: crash_loader,
                    minecraft: &crash_minecraft,
                    // 崩了之后才读 mods：这一步几百毫秒，不该占着启动的路。
                    mods: crash::known_in(&crash_mods_directory),
                },
                &log_tail,
            );
            let _ = wait_events.send(LauncherEvent::GameCrashed(report));
        }

        // 这一次的成果就在那里，而且已经没有进程占着那些文件了。
        //
        // 崩了的那一次不拍：拍到的可能是一个写到一半的世界，而这恰恰是最需要
        // 「上一张还好着的快照」的时刻——不能让一张坏的把它挤下去。
        if exit_code == Some(0) {
            crate::backup::after_session(&snapshot_paths, &wait_instance);
        }

        // 对一次账，**不管这一次是怎么结束的**。
        //
        // 和快照那一条的判断正好相反：崩溃恰恰是最该查的时候。而这里是唯一一
        // 个同时满足两件事的时刻——刚才那段风险窗口关上了，并且没有人在等。
        // 所以这一遍读全部文件，不看缓存里的时间戳。
        crate::instance::integrity::after_session(&snapshot_paths, &wait_instance);
    });
    Ok(LaunchResult {
        instance_id: instance_id.to_owned(),
        version_id,
        process_id,
        java_binary,
        java_major,
        required_java_major,
        launch_log,
    })
}

/// 游戏跑着的时候，每隔几秒读一次 GC 日志的尾巴，报一次堆压力。
///
/// 读尾巴而不是整份：一场长会话的日志能到几 MB，而我们只关心最后那几次回收。
/// 读不到、解析不出来就什么都不报——**宁可岛上没有那条线，也不要一条编出来的
/// 线**。
fn spawn_memory_watch(
    gc_log: PathBuf,
    xmx_mb: u32,
    instance_id: String,
    events: UnboundedSender<LauncherEvent>,
    alive: Arc<AtomicBool>,
) {
    if xmx_mb == 0 {
        // 堆由用户的参数定，我们不知道分母是多少，那就不报占比。
        return;
    }
    std::thread::spawn(move || {
        while alive.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if !alive.load(Ordering::SeqCst) {
                break;
            }
            let Some(metrics) = read_log_tail(&gc_log, 64 * 1024)
                .as_deref()
                .and_then(memory::gclog::parse)
            else {
                continue;
            };
            let _ = events.send(LauncherEvent::GameMemory {
                instance_id: instance_id.clone(),
                used_mb: metrics.live_set_mb,
                peak_mb: metrics.peak_mb,
                xmx_mb,
            });
        }
    });
}

/// 一份日志的最后若干字节，按 UTF-8 尽力解出来。
fn read_log_tail(path: &Path, bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(bytes)))
        .ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// 退出之后要记的那一笔。
///
/// 打包成一个值是因为它整个要搬进等待线程：一个个 clone 出去的写法，将来加
/// 一个字段就得在三个地方各改一次。
struct SessionRecord {
    paths: DataPaths,
    instance_id: String,
    modlist_hash: String,
    gc_log: PathBuf,
    xmx_mb: u32,
    zgc: bool,
    started_at: std::time::SystemTime,
}

impl SessionRecord {
    /// 把这一次会话记进历史。下一次启动的分配就是照这些数算的。
    fn store(&self, log_tail: &str) -> Result<()> {
        // 用户自己钉死了堆的那种情况没有分母，学不出任何东西，直接不记。
        if self.xmx_mb == 0 {
            return Ok(());
        }
        let text = std::fs::read_to_string(&self.gc_log).unwrap_or_default();
        let Some(metrics) = memory::gclog::parse(&text) else {
            return Ok(());
        };
        let minutes = self
            .started_at
            .elapsed()
            .map(|elapsed| elapsed.as_secs_f64() / 60.0)
            .unwrap_or_default();
        memory::history::record(
            &self.paths,
            &self.instance_id,
            &self.modlist_hash,
            memory::history::Session {
                at: memory::history::now_seconds(),
                minutes,
                xmx_mb: self.xmx_mb,
                metrics,
                // 堆真的爆了的那一行只会出现在游戏自己的输出里，GC 日志看不到。
                oom: log_tail.contains("OutOfMemoryError"),
                zgc: self.zgc,
            },
        )
    }
}

/// 让游戏进程跑在指定的优先级上（文档 §6.3）。
///
/// 默认不动——调度器本来就偏向前台进程，绝大多数情况下调它没有意义。存在的
/// 理由是「一边挂机一边干别的」这类场景，那时候用户明确知道自己要什么。
fn apply_priority(command: &mut Command, priority: crate::ProcessPriority) {
    if priority == crate::ProcessPriority::Normal {
        return;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 这两个常量在 std 里没有，值来自 Win32 的 processthreadsapi.h。
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
        command.creation_flags(match priority {
            crate::ProcessPriority::Low => BELOW_NORMAL_PRIORITY_CLASS,
            crate::ProcessPriority::High => ABOVE_NORMAL_PRIORITY_CLASS,
            crate::ProcessPriority::Normal => return,
        });
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // nice 值越小优先级越高。调高需要特权，拿不到就算了——为了一个可选的
        // 优化而让启动失败是不划算的，所以这里忽略返回值。
        let niceness = match priority {
            crate::ProcessPriority::Low => 10,
            crate::ProcessPriority::High => -5,
            crate::ProcessPriority::Normal => return,
        };
        // SAFETY: 在 fork 和 exec 之间执行，setpriority 是 async-signal-safe 的。
        unsafe {
            command.pre_exec(move || {
                libc::setpriority(libc::PRIO_PROCESS, 0, niceness);
                Ok(())
            });
        }
    }
}

fn append_launch_log(path: &Path, message: &str) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{} {message}", chrono_like_timestamp())
}

/// 一个读线程要往哪些地方送。
struct LogSink {
    path: PathBuf,
    stream: &'static str,
    instance_id: String,
    events: UnboundedSender<LauncherEvent>,
    tail: Arc<Mutex<String>>,
    running: Arc<AtomicBool>,
}

/// 崩溃分析看最后这么多字节就够，再多只是把内存和 IPC 撑大。
const TAIL_LIMIT: usize = 64 * 1024;

fn spawn_log_reader<R>(reader: R, sink: LogSink)
where
    R: io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        // 落盘的那一份必须无条件写：界面订阅失败、事件被丢弃，日志文件都
        // 还得在。
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink.path)
            .ok()
            .map(io::BufWriter::new);
        let mut file = file;
        let mut parser = LogParser::new();
        let stderr = sink.stream == "stderr";

        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if let Some(file) = file.as_mut() {
                let _ = writeln!(file, "[{}] {line}", sink.stream);
                // 不缓冲太久：游戏卡死时，最后写进去的几行往往就是原因。
                let _ = file.flush();
            }
            if let Ok(mut tail) = sink.tail.lock() {
                tail.push_str(&line);
                tail.push('\n');
                if tail.len() > TAIL_LIMIT * 2 {
                    let cut = tail.len() - TAIL_LIMIT;
                    let cut = (cut..tail.len())
                        .find(|index| tail.is_char_boundary(*index))
                        .unwrap_or(tail.len());
                    *tail = tail[cut..].to_owned();
                }
            }
            let Some(parsed) = parser.push(&line, stderr) else {
                continue;
            };
            if gamelog::signals_window_ready(&parsed.message) {
                announce_running(&sink.running, &sink.events, &sink.instance_id);
            }
            if sink
                .events
                .send(LauncherEvent::GameLog {
                    instance_id: sink.instance_id.clone(),
                    level: parsed.level,
                    message: parsed.message,
                })
                .is_err()
            {
                // 界面走了，日志还得继续读——不读，管道满了游戏就卡死。
                continue;
            }
        }

        if let Some(parsed) = parser.flush(stderr) {
            let _ = sink.events.send(LauncherEvent::GameLog {
                instance_id: sink.instance_id.clone(),
                level: parsed.level,
                message: parsed.message,
            });
        }
        if let Some(file) = file.as_mut() {
            let _ = file.flush();
        }
    });
}

/// 只报一次「跑起来了」。
fn announce_running(
    running: &AtomicBool,
    events: &UnboundedSender<LauncherEvent>,
    instance_id: &str,
) {
    if running.swap(true, Ordering::SeqCst) {
        return;
    }
    running::mark_ready(instance_id);
    let _ = events.send(LauncherEvent::LaunchStage {
        instance_id: instance_id.to_owned(),
        stage: LaunchStage::Running,
    });
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("[{seconds}]")
}

/// 挑一个 Java 出来启动这个实例。
///
/// 用户在实例设置里填了路径就照做——那时候他要的是控制权，不是建议；只有
/// 版本真的不够才拦。没填就自己挑，这一层对他完全隐形。
fn resolve_java_runtime(
    paths: &DataPaths,
    profile: &crate::InstanceProfile,
    requirement: &java::JavaRequirement,
) -> Result<java::JavaRuntime> {
    if let Some(configured) = profile.settings.java_path.as_deref() {
        return java::probe(configured);
    }
    let runtimes = java::discover(Some(paths));
    java::select(&runtimes, requirement).ok_or_else(|| {
        let found = if runtimes.is_empty() {
            "未找到任何 Java".to_owned()
        } else {
            format!(
                "已找到 Java {}",
                runtimes
                    .iter()
                    .map(|runtime| runtime.major.to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            )
        };
        anyhow!("需要 Java {} 或更高版本；{found}", requirement.minimum)
    })
}

/// 元数据没说、但这个平台和版本的组合非有不可的 JVM 参数（文档 §5.2）。
///
/// 只补两条。不预先堆参数：每一条都要能说清为什么，说不清的就该等到有人真的
/// 报了问题再加。
fn platform_arguments(version_id: &str, existing: &[String]) -> Vec<String> {
    let mut extra = Vec::new();
    let ordinal = release_ordinal(version_id);

    // macOS 上 LWJGL 3 硬性要求渲染跑在第一个线程上，不加就是启动即闪退。
    // 1.13 之后的元数据 rules 里带了这一条，所以通常轮不到我们；1.13 之前用
    // 的是 LWJGL 2，加了反而不对——所以两头都要判。
    if cfg!(target_os = "macos")
        && ordinal.is_some_and(|version| version >= (1, 13, 0))
        && !existing
            .iter()
            .any(|argument| argument == "-XstartOnFirstThread")
    {
        extra.push("-XstartOnFirstThread".to_owned());
    }

    // Log4Shell（CVE-2021-44228）。1.7 到 1.18.1 的 log4j 会解析日志文本里的
    // JNDI 查找——一条聊天消息就足以让客户端去远端拉一个类回来执行。Mojang
    // 的正解是换一份配置文件（logging.client，上面刚加过），这个开关是第二
    // 道；两道一起上才盖得住那些没有 logging 段的版本。
    if ordinal.is_some_and(|version| ((1, 7, 0)..=(1, 18, 1)).contains(&version)) {
        extra.push("-Dlog4j2.formatMsgNoLookups=true".to_owned());
    }

    extra
}

fn filter_jvm_arguments(arguments: Vec<String>, java_major: u16) -> Vec<String> {
    arguments
        .into_iter()
        .filter(|argument| argument != "--sun-misc-unsafe-memory-access=allow" || java_major >= 24)
        .collect()
}

async fn collect_classpath_and_extract_natives(
    paths: &DataPaths,
    metadata: &VersionMetadata,
    context: &RuleContext,
    natives_directory: &Path,
) -> Result<Vec<PathBuf>> {
    let mut classpath = Vec::new();
    // rules 与坐标冲突都在这一步解决：同一个 group:artifact 只留一份，否则
    // 加载器会因为 classpath 上有两份同名类而拒绝启动（见 effective_libraries）。
    for library in metadata.effective_libraries(context) {
        let Some(downloads) = &library.downloads else {
            // 只给了仓库前缀的库（加载器的那些）——路径由坐标推出来，补全时
            // 就是按同一个坐标下的。指向不了任何文件的占位条目跳过。
            if library.url.is_some() {
                if let Some(relative) = fern_meta::maven_path(&library.name) {
                    classpath.push(paths.libraries.join(relative));
                } else {
                    return Err(anyhow!("库坐标 {} 无法推出路径", library.name));
                }
            }
            continue;
        };
        if let Some(artifact) = &downloads.artifact {
            let relative = artifact
                .path
                .as_deref()
                .ok_or_else(|| anyhow!("library {} has no artifact path", library.name))?;
            let path = paths.libraries.join(relative);
            if library.extract.is_some() || library.natives.is_some() {
                extract_native_jar(&path, natives_directory, library).await?;
            } else {
                // Modern metadata publishes native jars as classifier-only artifacts.
                // LWJGL loads these jars from the classpath and extracts the dylib itself.
                classpath.push(path);
            }
        }
        if let (Some(natives), Some(classifiers)) = (&library.natives, &downloads.classifiers) {
            let Some(template) = natives.get(&context.os_name) else {
                continue;
            };
            let arch = if context.os_arch.contains("64") {
                "64"
            } else {
                "32"
            };
            let classifier = template.replace("${arch}", arch);
            if let Some(native) = classifiers.get(&classifier) {
                let path = paths.libraries.join(
                    native
                        .path
                        .as_deref()
                        .ok_or_else(|| anyhow!("library {} has no native path", library.name))?,
                );
                extract_native_jar(&path, natives_directory, library).await?;
            }
        }
    }
    Ok(classpath)
}

async fn extract_native_jar(path: &Path, destination: &Path, library: &Library) -> Result<()> {
    let path = path.to_owned();
    let destination = destination.to_owned();
    let excludes = library
        .extract
        .as_ref()
        .map(|rule| rule.exclude.clone())
        .unwrap_or_default();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file =
            File::open(&path).with_context(|| format!("open native jar {}", path.display()))?;
        let mut archive = zip::ZipArchive::new(file).context("read native jar archive")?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_owned();
            if excludes.iter().any(|prefix| name.starts_with(prefix)) || name.ends_with('/') {
                continue;
            }
            let relative = Path::new(&name);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                continue;
            }
            let output = destination.join(relative);
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut target = File::create(output)?;
            io::copy(&mut entry, &mut target)?;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

impl LaunchPlan {
    pub fn command_arguments(&self, variables: &LaunchVariables) -> Vec<String> {
        let separator = if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        };
        let classpath = self
            .classpath
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join(separator);
        let variables = variables.clone().insert("classpath", classpath);
        let mut arguments = self
            .jvm_arguments
            .iter()
            .map(|argument| variables.substitute(argument))
            .collect::<Vec<_>>();
        if !arguments
            .iter()
            .any(|argument| argument == "-cp" || argument == "-classpath")
        {
            arguments.push("-cp".to_owned());
            arguments.push(variables.substitute("${classpath}"));
        }
        arguments.push(self.main_class.clone());
        arguments.extend(
            self.game_arguments
                .iter()
                .map(|argument| variables.substitute(argument)),
        );
        arguments
    }
}

/// Windows 的命令行总长上限，留一点余量。
///
/// CreateProcess 的硬上限是 32767 个字符。大型整合包的 classpath 能有几百个
/// jar，路径再深一点就顶上去了——超了之后进程根本起不来，报的错还和长度
/// 毫无关系。
const COMMAND_LINE_LIMIT: usize = 31000;

/// 参数太长时改用 `@argfile`（文档 §5.1）。
///
/// Java 9 起支持从文件读参数。只在 Windows 上、只在真的超长时才用——平时
/// 直接传参更好排查，日志里能原样看到命令行。
///
/// 返回替换后的参数表。用不上就原样返回。
fn argfile_if_needed(
    arguments: Vec<String>,
    main_class: &str,
    java_major: u16,
    directory: &Path,
) -> io::Result<Vec<String>> {
    let length: usize = arguments.iter().map(|argument| argument.len() + 3).sum();
    if !cfg!(target_os = "windows") || java_major < 9 || length < COMMAND_LINE_LIMIT {
        return Ok(arguments);
    }

    // 主类和它后面的游戏参数不能进 argfile：`@file` 只用来给 JVM 读它自己的
    // 参数，展开的位置在主类之前。按主类本身定位，不靠「像不像选项」去猜——
    // classpath 那一长串里什么字符都可能有。
    let split = arguments
        .iter()
        .position(|argument| argument == main_class)
        .unwrap_or(arguments.len());
    let (jvm, rest) = arguments.split_at(split);

    let path = directory.join("fern-launch.args");
    std::fs::write(&path, encode_argfile(jvm))?;

    let mut replaced = vec![format!("@{}", path.display())];
    replaced.extend(rest.iter().cloned());
    Ok(replaced)
}

/// argfile 的转义规则。
///
/// Java 按空白切分，双引号内的内容当作一个整体；引号内的反斜杠是转义字符，
/// 所以 Windows 路径里的每个反斜杠都要写两遍。不转义的话
/// `C:\Users\name` 会变成 `C:Usersname`——一个只在长命令行时才出现的、
/// 极难对上号的故障。
fn encode_argfile(arguments: &[String]) -> String {
    let mut text = String::new();
    for argument in arguments {
        text.push('"');
        for character in argument.chars() {
            if character == '\\' || character == '"' {
                text.push('\\');
            }
            text.push(character);
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_deterministic_version_three() {
        let first = offline_credentials("FernPlayer");
        let second = offline_credentials("FernPlayer");
        assert_eq!(first.uuid, second.uuid);
        assert_eq!(first.uuid.as_bytes()[14], b'3');
        assert_eq!(first.user_type, "legacy");
    }

    #[test]
    fn template_preserves_unknown_variables() {
        let variables = LaunchVariables::new().insert("known", "value");
        assert_eq!(
            variables.substitute("${known}/${private}"),
            "value/${private}"
        );
    }

    #[test]
    fn launch_plan_builds_classpath_and_substitutes_credentials() {
        let credentials = offline_credentials("FernPlayer");
        let variables = LaunchVariables::new().with_credentials(&credentials);
        let plan = LaunchPlan {
            java_binary: PathBuf::from("java"),
            working_directory: PathBuf::from("instance"),
            jvm_arguments: vec!["-Xmx2G".to_owned()],
            classpath: vec![
                PathBuf::from("libraries/a.jar"),
                PathBuf::from("client.jar"),
            ],
            main_class: "net.minecraft.client.main.Main".to_owned(),
            game_arguments: vec!["--username".to_owned(), "${auth_player_name}".to_owned()],
        };
        let arguments = plan.command_arguments(&variables);
        assert!(arguments.iter().any(|argument| argument == "-cp"));
        assert!(arguments.iter().any(|argument| argument == "FernPlayer"));
        assert_eq!(arguments.last().map(String::as_str), Some("FernPlayer"));
    }

    /// natives 解压此前没有任何覆盖，而它同时负责两件容易出事的事：按
    /// `extract.exclude` 排除 META-INF，以及挡住 jar 里指向外面的路径。
    #[tokio::test]
    async fn native_extraction_honours_excludes_and_refuses_to_escape() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let root = std::env::temp_dir().join(format!("fern-natives-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test root");
        let jar_path = root.join("natives.jar");

        let mut writer = zip::ZipWriter::new(std::fs::File::create(&jar_path).expect("create jar"));
        let options = SimpleFileOptions::default();
        for (name, body) in [
            ("META-INF/MANIFEST.MF", &b"manifest"[..]),
            ("liblwjgl.so", &b"native code"[..]),
            ("../escaped.so", &b"should not land outside"[..]),
        ] {
            writer.start_file(name, options).expect("start entry");
            writer.write_all(body).expect("write entry");
        }
        writer.finish().expect("finish jar");

        let destination = root.join("natives");
        let library = Library {
            name: "org.lwjgl:lwjgl-platform:2.9.4".to_owned(),
            extract: Some(fern_meta::ExtractRule {
                exclude: vec!["META-INF/".to_owned()],
            }),
            ..Library::default()
        };
        extract_native_jar(&jar_path, &destination, &library)
            .await
            .expect("extract natives");

        assert!(destination.join("liblwjgl.so").is_file());
        assert!(!destination.join("META-INF/MANIFEST.MF").exists());
        assert!(!root.join("escaped.so").exists());

        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn argfile_encoding_survives_a_round_trip_through_java() {
        // 引号里的反斜杠是转义字符，Windows 路径里每一个都要写两遍。不转义的
        // 话 `C:\\Users\\name` 会变成 `C:Usersname`——一个只在长命令行时才
        // 出现、极难对上号的故障。Java 9+ 在所有平台都认 @argfile，所以这条
        // 能在 Linux 上真跑一遍。
        let Some(java) = crate::discover_java(None)
            .into_iter()
            .find(|runtime| runtime.major >= 9)
        else {
            return;
        };

        let root = std::env::temp_dir().join(format!("fern-argfile-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("args");
        let tricky = r#"C:\Users\Fern Player\一个"引号""#;
        std::fs::write(
            &path,
            encode_argfile(&[
                format!("-Dfern.test={tricky}"),
                "-XshowSettings:properties".to_owned(),
                "-version".to_owned(),
            ]),
        )
        .expect("write argfile");

        let output = Command::new(&java.path)
            .arg(format!("@{}", path.display()))
            .output()
            .expect("run java");
        let text = String::from_utf8_lossy(&output.stderr).into_owned()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains(tricky),
            "参数没有原样传进 JVM。argfile 内容：\n{}\n实际属性：\n{}",
            std::fs::read_to_string(&path).unwrap_or_default(),
            text.lines()
                .filter(|line| line.contains("fern.test"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn short_command_lines_are_left_alone() {
        let arguments = vec![
            "-Xmx4G".to_owned(),
            "net.minecraft.client.main.Main".to_owned(),
            "--username".to_owned(),
        ];
        let same = argfile_if_needed(
            arguments.clone(),
            "net.minecraft.client.main.Main",
            21,
            Path::new("/tmp"),
        )
        .expect("no argfile needed");
        assert_eq!(same, arguments);
    }

    #[test]
    fn log4shell_mitigation_covers_exactly_the_affected_versions() {
        let affected = |version: &str| {
            platform_arguments(version, &[])
                .iter()
                .any(|argument| argument == "-Dlog4j2.formatMsgNoLookups=true")
        };
        assert!(affected("1.7.10"));
        assert!(affected("1.12.2"));
        assert!(affected("1.18.1"));
        assert!(!affected("1.18.2"));
        assert!(!affected("1.21.1"));
        assert!(!affected("1.6.4"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lwjgl3_versions_get_the_main_thread_flag_only_when_metadata_forgot() {
        let has_flag = |version: &str, existing: &[String]| {
            platform_arguments(version, existing)
                .iter()
                .any(|argument| argument == "-XstartOnFirstThread")
        };
        assert!(has_flag("1.13", &[]));
        // 元数据已经给了就别给第二遍。
        assert!(!has_flag("1.21.1", &["-XstartOnFirstThread".to_owned()]));
        // LWJGL 2 的版本加了反而不对。
        assert!(!has_flag("1.12.2", &[]));
    }

    #[test]
    fn filters_unsafe_memory_access_flag_for_old_java() {
        let args = vec![
            "--sun-misc-unsafe-memory-access=allow".to_owned(),
            "-Xmx2G".to_owned(),
        ];
        assert_eq!(filter_jvm_arguments(args.clone(), 21), vec!["-Xmx2G"]);
        assert_eq!(filter_jvm_arguments(args, 24).len(), 2);
    }
}
