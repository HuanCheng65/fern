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
use fern_meta::{Library, RuleContext, VersionMetadata, release_ordinal, rules_allow};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths, LaunchStage, LauncherEvent, crash, gamelog, gamelog::LogParser, java, tuning,
    version,
};

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
    player_name: &str,
    events: &UnboundedSender<LauncherEvent>,
) -> Result<LaunchResult> {
    let stage = |stage: LaunchStage| {
        let _ = events.send(LauncherEvent::LaunchStage {
            instance_id: instance_id.to_owned(),
            stage,
        });
    };
    stage(LaunchStage::ResolvingVersion);
    paths.ensure_exists()?;
    let profile = crate::list_instances(paths)?
        .into_iter()
        .find(|profile| profile.id.as_str() == instance_id)
        .ok_or_else(|| anyhow!("instance {instance_id} does not exist"))?;
    // 装了加载器时，要启动的是加载器生成的那份 JSON，它用 inheritsFrom 指回
    // 原版。合并在 version 模块里做一次，补全和启动用的必须是同一份——两边
    // 各算各的，就会出现「文件明明下好了却说缺」这种最难查的问题。
    let version_id = version::effective_id(&profile);
    let metadata = version::resolve(paths, &version_id)
        .with_context(|| format!("读取 {version_id} 的版本描述"))?;
    // 客户端 jar 始终属于原版：加载器改的是启动方式，不是游戏本体。
    let client_jar = paths
        .versions
        .join(&profile.game_version)
        .join(format!("{}.jar", profile.game_version));
    let main_class = metadata
        .main_class
        .clone()
        .ok_or_else(|| anyhow!("version {version_id} has no main class"))?;

    stage(LaunchStage::CheckingFiles);
    let context = current_rule_context(profile.settings.resolution.is_some());
    let natives_directory = paths.game_directory(instance_id).join("natives");
    tokio::fs::create_dir_all(&natives_directory).await?;
    let classpath =
        collect_classpath_and_extract_natives(paths, &metadata, &context, &natives_directory)
            .await?;
    if !tokio::fs::try_exists(&client_jar).await? {
        return Err(anyhow!("client jar is missing: {}", client_jar.display()));
    }

    // 用哪个账号是全局设置，不是每个实例各自一份——玩家只有一个身份。
    let (credentials, account_arguments) = resolve_account(paths, player_name, events).await?;
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
        .insert("launcher_name", "Fern")
        .insert("launcher_version", env!("CARGO_PKG_VERSION"))
        .insert("clientid", "")
        .insert("auth_xuid", "");
    if let Some(resolution) = &profile.settings.resolution {
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
    for (index, argument) in account_arguments.into_iter().enumerate() {
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
            "Java {} 起不了 Minecraft {version_id}，这个版本至少需要 Java {}（当前 {}）",
            runtime.major,
            requirement.minimum,
            runtime.version
        ));
    }
    let java_binary = runtime.path.clone();
    let java_major = runtime.major;

    // 堆大小和 GC 放在挑完 Java 之后：G1 那组参数只对 17 以上给，而堆大小
    // 要看这个实例的 mods 目录有多大。
    let heap = tuning::heap_megabytes(
        tuning::physical_memory_bytes(),
        tuning::mods_profile(&game_directory),
        profile.settings.max_memory_mb,
    );
    if let Some(argument) = tuning::heap_argument(&jvm_arguments, heap) {
        jvm_arguments.push(argument);
    }
    jvm_arguments.extend(tuning::gc_arguments(java_major, &jvm_arguments));
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
    let log_directory = paths.instance_log_directory(instance_id);
    std::fs::create_dir_all(&log_directory)?;
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
    stage(LaunchStage::StartingProcess);
    let started_at = std::time::SystemTime::now();
    let mut child = Command::new(&java_binary)
        .args(arguments)
        .current_dir(&plan.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start Java from {}", java_binary.display()))?;
    append_launch_log(&launch_log, &format!("started pid={}", child.id()))?;

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

    let process_id = child.id();
    let wait_log = launch_log.clone();
    let wait_events = events.clone();
    let wait_instance = instance_id.to_owned();
    let wait_directory = plan.working_directory.clone();
    let wait_running = running.clone();
    std::thread::spawn(move || {
        let running = wait_running;
        let exit_code = match child.wait() {
            Ok(status) => {
                let code = status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string());
                let _ = append_launch_log(&wait_log, &format!("exited code={code}"));
                status.code()
            }
            Err(error) => {
                let _ = append_launch_log(&wait_log, &format!("wait error={error}"));
                None
            }
        };

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

        // 正常关掉游戏不该在界面上留下任何痕迹，崩了才需要说话。信号退出
        // （exit_code 为 None）也算——那多半是被 OOM killer 收走了。
        if exit_code != Some(0) {
            let log_tail = tail.lock().map(|tail| tail.clone()).unwrap_or_default();
            let report = crash::build_report(
                &wait_instance,
                &wait_directory,
                started_at,
                exit_code,
                &log_tail,
            );
            let _ = wait_events.send(LauncherEvent::GameCrashed(report));
        }
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

/// 解析出这次要用谁的身份启动，以及为此要额外挂的 JVM 参数。
///
/// 离线模式什么都不用挂。外置登录要先把令牌刷新一遍（过期的令牌进服会被踢，
/// 而那时候的报错和登录没有任何关系），再把 authlib-injector 挂上去。
async fn resolve_account(
    paths: &DataPaths,
    player_name: &str,
    events: &UnboundedSender<LauncherEvent>,
) -> Result<(Credentials, Vec<String>)> {
    if crate::current_settings().account.kind != crate::AccountKind::Authlib {
        return Ok((offline_credentials_checked(player_name)?, Vec::new()));
    }

    let stored =
        crate::load_session()?.ok_or_else(|| anyhow!("外置登录还没有登录过，去设置里登录一次"))?;
    let session = crate::refresh_session(&stored)
        .await
        .context("刷新外置登录令牌失败，可能需要重新登录")?;
    if session != stored {
        crate::store_session(&session)?;
    }

    let downloads = crate::event::download_bridge(events);
    let injector = crate::ensure_injector(paths, &downloads).await?;
    // 预取失败不该拦住启动：injector 自己会去请求一次，只是慢一点。
    let prefetched = crate::prefetched_metadata(&session.api_root)
        .await
        .unwrap_or_default();

    Ok((
        Credentials {
            player_name: session.player_name.clone(),
            uuid: session.uuid.clone(),
            access_token: session.access_token.clone(),
            user_type: "msa".to_owned(),
        },
        crate::auth::jvm_arguments(&injector, &session.api_root, &prefetched),
    ))
}

/// 离线名字的规则是 Minecraft 自己的：3-16 位 ASCII。外置登录的名字由皮肤站
/// 决定，轮不到我们校验。
fn offline_credentials_checked(player_name: &str) -> Result<Credentials> {
    if !(3..=16).contains(&player_name.len())
        || !player_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(anyhow!("离线模式的名字要 3-16 位字母、数字或下划线"));
    }
    Ok(offline_credentials(player_name))
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
            "这台机器上没有找到任何 Java".to_owned()
        } else {
            format!(
                "找到的是 Java {}",
                runtimes
                    .iter()
                    .map(|runtime| runtime.major.to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            )
        };
        anyhow!("需要 Java {} 或更新的版本，{found}", requirement.minimum)
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
    for library in &metadata.libraries {
        if !rules_allow(library.rules.as_deref(), context) {
            continue;
        }
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

fn current_rule_context(has_custom_resolution: bool) -> RuleContext {
    RuleContext {
        os_name: if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        }
        .to_owned(),
        os_arch: std::env::consts::ARCH.to_owned(),
        os_version: String::new(),
        features: HashMap::from([("has_custom_resolution".to_owned(), has_custom_resolution)]),
    }
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
