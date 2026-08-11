//! Fern's launcher orchestration boundary.
//!
//! Tauri commands call this crate. It owns launcher state and translates
//! download, Java, account, and process events into UI-facing values.
//!
//! 内部按**这件事属于谁**分层，每一层一个目录：
//!
//! ```text
//! data/      数据目录、设置、元数据缓存 —— 谁都要读的地基
//! instance/  实例是什么、曲库、mods/存档/服务器列表
//! account/   名册、三种登录方式、令牌保管
//! java/      声明什么、发现什么、缺了去下什么
//! launch/    补全 → 版本合并 → 加载器 → 参数拼装 → 进程 → 日志与崩溃
//! supply/    从站点获取：Modrinth 与整合包
//! ```
//!
//! `event` 和 `job` 留在顶层：它们是各层向界面说话的共用词汇，属于任何一层
//! 都会变成那一层的私产。
//!
//! 这个文件是整个 crate 的**唯一门面**——外面（Tauri 命令）只看得见下面这些
//! 名字，看不见目录结构。所以内部怎么分层是可以改的，改了不牵动前端。

mod account;
mod backup;
mod data;
mod event;
mod instance;
mod java;
mod job;
mod launch;
mod process;
mod supply;
mod update;

pub use account::Account;
pub use account::credentials::{client_token, store_secret};
pub use account::microsoft::{
    DeviceCodeChallenge, MicrosoftSession, Nudge, begin_login as begin_microsoft_login,
    ensure_fresh as refresh_microsoft_session, finish_login as finish_microsoft_login,
};
pub use account::roster::{
    AccountKind, AccountRecord, Roster, Secret, active as active_account,
    add_offline as add_offline_account, adopt_session as adopt_account,
    for_instance as account_for_instance, list as list_accounts, remove as remove_account,
    rename_offline as rename_offline_account, set_active as set_active_account,
};
pub use account::skin::{AccountSkin, of_record as account_skin};
pub use account::yggdrasil::{
    YggdrasilSession, authenticate, ensure_fresh as refresh_session, ensure_injector,
    prefetched as prefetched_metadata,
};
pub use backup::export::{Contents as ExportContents, Exported};
pub use backup::export::{
    fernpack as export_fernpack, mrpack as export_mrpack, world as export_world,
};
pub use backup::manifest::Reason as SnapshotReason;
pub use backup::select::{SkipReason, Skipped};
pub use backup::{
    InstanceUsage, Missing, Mode as RestoreMode, Restored, Scope as RestoreScope, Snapshot, Usage,
    collect_garbage, label as label_snapshot, list as list_snapshots, prune as prune_snapshots,
    reasons as snapshot_reasons, remove as remove_snapshot, restore as restore_snapshot,
    take as take_snapshot, usage as backup_usage,
};
pub use data::settings::{
    AccountSettings, DownloadSettings, EffectiveSettings, GameDefaults, Settings, SourcePreference,
    UpdateSettings, current as current_settings, effective as effective_settings,
    load as load_settings, save as save_settings,
};
pub use data::{DataPaths, ExternalGame, Isolation, nearby_game_directory};
pub use event::{LaunchStage, LauncherEvent, LogLevel};
pub use fern_download::DownloadEvent;
pub use fern_meta::{VersionManifest, VersionManifestEntry};
pub use instance::catalog::{
    InstanceRuntime, VersionOption, create_instance, create_instance_with_loader, delete_instance,
    duplicate_instance, instance_runtime, list_instances, list_versions, read_instance,
    read_prepared_java_major, rename_instance, set_instance_account, touch_played,
    update_instance_settings, write_instance_profile,
};
pub use instance::external::{
    ExternalScan, ExternalVersion, SkippedVersion, attach as attach_external_version,
    scan as scan_external_directory,
};
pub use instance::integrity::{
    Change, Compared, Depth as IntegrityDepth, Notice as IntegrityNotice,
    accept as accept_integrity, accept_new as accept_new_files, adopt as adopt_integrity,
    ask_upstream as ask_upstream_about_changes, compare as compare_integrity,
    look as check_integrity, notices as integrity_notices,
};
pub use instance::mods::{
    ModFile, install as install_mod, list as list_mods, remove as remove_mod,
    set_enabled as set_mod_enabled,
};
pub use instance::origin::{
    Origin, Record as OriginRecord, broken_at as origin_log_broken_at, latest as latest_origins,
    records as origin_records,
};
pub use instance::paths_for as instance_paths;
pub use instance::saves::{SaveEntry, list as list_saves, names as save_names};
pub use instance::servers::{ServerEntry, list as list_servers};
pub use instance::{
    CoverSeed, GarbageCollector, InstanceId, InstanceProfile, InstanceSettings, LoaderKind,
    LoaderProfile, ProcessPriority, Resolution,
};
pub use java::runtime::{ensure_java, install as install_java, remove as remove_runtime};
pub use java::{
    JavaGroup, JavaImage, JavaRequirement, JavaRuntime, add_path as add_java_path, detect_java,
    discover as discover_java, forget_path as forget_java_path, overview as java_overview,
    requirement as java_requirement, select as select_java,
};
pub use job::{Job, JobEvent, JobText, Track};
pub use launch::crash::{
    Action as FixAction, CrashReport, Diagnosis, Level as DiagnosisLevel, Reason as SuspectReason,
    Suspect, attribute_crash, diagnose as diagnose_crash, rules::Context as CrashContext,
};
pub use launch::preflight::{Finding, Severity, check as preflight_instance};
pub use update::{
    Channel as UpdateChannel, DEFAULT_ENDPOINT as UPDATE_ENDPOINT, Decision as UpdateDecision,
    Install as UpdateInstall, Manifest as UpdateManifest, check_now as check_for_update,
    effective_channel as update_channel, install as update_install, target as update_target,
    writable_beside_executable,
};

/// 这台机器是什么。反馈问题时的第一句话。
///
/// 版本号从 `launch::rules::os_version` 来——那一份本来是给版本元数据里的
/// `^10\.` 之类规则求值用的，顺手在这里也用上，不必再写一遍三个平台的读法。
pub fn platform() -> String {
    let name = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    let version = launch::rules::os_version();
    let architecture = std::env::consts::ARCH;
    if version.is_empty() {
        format!("{name} · {architecture}")
    } else {
        format!("{name} {version} · {architecture}")
    }
}

/// 后端会发出的全部文案 id。
///
/// 后端不产出句子，只产出 id 和参数（见 `launch::crash::rules` 与
/// `launch::preflight`）。这个清单是它与界面之间的契约：界面必须为每一条备好
/// 文案，而 `fern-ui/src/lib/i18n/keys.ts` 由测试照着它生成，少一条就是编译错误。
pub fn message_ids() -> Vec<String> {
    let mut ids: Vec<String> = launch::crash::rules::ids()
        .into_iter()
        .map(|id| format!("crash.{id}"))
        .chain(
            launch::preflight::kind::ALL
                .iter()
                .map(|kind| format!("preflight.{kind}")),
        )
        .chain(
            backup::manifest::Reason::ALL
                .iter()
                .map(|reason| format!("snapshot.{}", reason.tag())),
        )
        .chain(
            backup::select::SkipReason::ALL
                .iter()
                .map(|reason| format!("snapshot.skipped.{}", reason.tag())),
        )
        .chain(
            instance::integrity::kind::ALL
                .iter()
                .map(|kind| format!("integrity.{kind}")),
        )
        .chain(job::TEXT_IDS.iter().map(|id| (*id).to_owned()))
        .collect();
    ids.sort();
    ids
}

pub use launch::gamelog::{LogLine, LogParser};
pub use launch::loader::{
    LoaderOption, LoaderVersion, display_name as loader_display_name,
    installable as installable_loaders, latest_version as latest_loader_version,
    list_versions as list_loader_versions,
};
pub use launch::memory::{
    AllocationDecision, AllocationSource, ExplanationItem, GcPath, MemoryBudget, MemoryHistory,
    ModsProfile, Topic as ExplanationTopic, heap_ceiling, measured as memory_history,
    memory_budget, mods_profile, physical_memory_bytes, plan as plan_allocation,
};
pub use launch::prepare::{PrepareResult, prepare_instance};
pub use launch::rules::QuickPlay;
pub use launch::running::{RunningGame, list as running_games, stop as stop_game};
pub use launch::version::{effective_id as effective_version_id, resolve as resolve_version};
pub use launch::{
    Credentials, LaunchPlan, LaunchResult, LaunchVariables, launch_instance, offline_credentials,
};
pub use supply::modpack::{
    PackSummary, inspect as inspect_modpack, install as install_modpack,
    install_from_modrinth as install_modpack_from_modrinth,
};
pub use supply::plan::{
    DependencyKind, InstallPlan, PlannedFile, Requirement, RequirementState,
    resolve as resolve_install_plan,
};
pub use supply::{
    GalleryImage, InstallOutcome, ProjectDetail, ProjectLink, ProjectVersion, ResourceKind,
    SearchHit, SearchQuery, SearchResult, Source, VersionDependency,
    install as install_from_modrinth, is_external_url, project as modrinth_project,
    search as search_modrinth, versions as modrinth_versions,
};

/// Marker for the Pearl integration boundary.
///
/// Keeping the dependency in this crate makes Pearl a replaceable launcher
/// capability while Fern's metadata and launch pipeline remain independent.
pub fn pearl_dependency_present() -> bool {
    let _ = std::any::TypeId::of::<pearl_core::identity::NodeId>();
    true
}

#[cfg(test)]
mod message_tests {
    /// 生成界面那边的 id 清单，并盯着它别过期。
    ///
    /// 后端加了一条规则、界面还没写文案时，`pnpm check` 会报错——因为文案表
    /// 声明成了 `Record<BackendMessage, …>`。这条测试负责把 id 清单同步过去；
    /// 清单变了它会自己重写文件并失败一次，再跑一遍就过。
    #[test]
    fn the_interface_has_the_current_list_of_message_ids() {
        let mut text = String::new();
        text.push_str(
            "// 由 `cargo test` 生成，不要手改（见 fern-core/src/lib.rs 的 message_ids）。\n",
        );
        text.push_str("//\n");
        text.push_str(
            "// 后端只发 id 和参数，不发句子。这里的每一条都必须在文案表里有标题与说明——\n",
        );
        text.push_str("// 少一条是编译错误，不是运行时才发现的空白。\n");
        text.push_str("export const BACKEND_MESSAGES = [\n");
        for id in super::message_ids() {
            text.push_str(&format!("  '{id}',\n"));
        }
        text.push_str(
            "] as const\n\nexport type BackendMessage = (typeof BACKEND_MESSAGES)[number]\n",
        );

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fern-ui/src/lib/i18n/keys.ts");
        // 单独编译 fern-core 时没有界面那一半，跳过。
        let Some(parent) = path.parent().filter(|parent| parent.exists()) else {
            return;
        };
        let _ = parent;
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != text {
            std::fs::write(&path, &text).expect("写 keys.ts");
            panic!("文案 id 清单变了，已重写 {}，再跑一遍。", path.display());
        }
    }
}
