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
mod data;
mod event;
mod instance;
mod java;
mod job;
mod launch;
mod supply;

pub use account::Account;
pub use account::credentials::{client_token, store_secret};
pub use account::microsoft::{
    DeviceCodeChallenge, MicrosoftSession, begin_login as begin_microsoft_login,
    ensure_fresh as refresh_microsoft_session, finish_login as finish_microsoft_login,
};
pub use account::roster::{
    AccountKind, AccountRecord, Roster, Secret, active as active_account,
    add_offline as add_offline_account, adopt_session as adopt_account,
    for_instance as account_for_instance, list as list_accounts, remove as remove_account,
    rename_offline as rename_offline_account, set_active as set_active_account,
};
pub use account::yggdrasil::{
    YggdrasilSession, authenticate, ensure_fresh as refresh_session, ensure_injector,
    prefetched as prefetched_metadata,
};
pub use data::settings::{
    AccountSettings, DownloadSettings, EffectiveSettings, GameDefaults, Settings, SourcePreference,
    current as current_settings, effective as effective_settings, load as load_settings,
    save as save_settings,
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
pub use instance::mods::{
    ModFile, install as install_mod, list as list_mods, remove as remove_mod,
    set_enabled as set_mod_enabled,
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
pub use job::{Job, JobEvent};
pub use launch::crash::{CrashDiagnosis, CrashReport, diagnose as diagnose_crash};
pub use launch::gamelog::{LogLine, LogParser};
pub use launch::loader::{
    LoaderOption, LoaderVersion, display_name as loader_display_name,
    installable as installable_loaders, latest_version as latest_loader_version,
    list_versions as list_loader_versions,
};
pub use launch::memory::{
    AllocationDecision, AllocationSource, ExplanationItem, GcPath, MemoryBudget, ModsProfile,
    Topic as ExplanationTopic, heap_ceiling, memory_budget, mods_profile, physical_memory_bytes,
    plan as plan_allocation,
};
pub use launch::prepare::{PrepareResult, prepare_instance};
pub use launch::rules::QuickPlay;
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
    SearchHit, SearchQuery, SearchResult, VersionDependency, install as install_from_modrinth,
    is_external_url, project as modrinth_project, search as search_modrinth,
    versions as modrinth_versions,
};

/// Marker for the Pearl integration boundary.
///
/// Keeping the dependency in this crate makes Pearl a replaceable launcher
/// capability while Fern's metadata and launch pipeline remain independent.
pub fn pearl_dependency_present() -> bool {
    let _ = std::any::TypeId::of::<pearl_core::identity::NodeId>();
    true
}
