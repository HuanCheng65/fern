//! Fern's launcher orchestration boundary.
//!
//! Tauri commands call this crate. It owns launcher state and translates
//! download, Java, account, and process events into UI-facing values.

mod account;
mod auth;
mod catalog;
mod crash;
mod credentials;
mod data;
mod event;
mod forge;
mod gamelog;
mod instance;
mod java;
mod launch;
mod loader;
mod microsoft;
mod modrinth;
mod mods;
mod prepare;
mod registry;
mod rules;
mod runtime;
mod saves;
mod settings;
mod tuning;
mod version;

pub use account::Account;
pub use auth::{
    AccountView, YggdrasilSession, authenticate, ensure_fresh as refresh_session, ensure_injector,
    prefetched as prefetched_metadata,
};
pub use catalog::{
    InstanceRuntime, VersionOption, create_instance, create_instance_with_loader, delete_instance,
    duplicate_instance, instance_runtime, list_instances, list_versions, rename_instance,
    touch_played, update_instance_settings, write_instance_profile,
};
pub use crash::{CrashDiagnosis, CrashReport, diagnose as diagnose_crash};
pub use credentials::{
    clear_microsoft_session, clear_session, client_token, load_microsoft_session, load_session,
    store_microsoft_session, store_session,
};
pub use data::DataPaths;
pub use event::{LaunchStage, LauncherEvent, LogLevel, download_bridge};
pub use fern_download::DownloadEvent;
pub use fern_meta::{VersionManifest, VersionManifestEntry};
pub use gamelog::{LogLine, LogParser};
pub use instance::{
    CoverSeed, GarbageCollector, InstanceId, InstanceProfile, InstanceSettings, LoaderKind,
    LoaderProfile, ProcessPriority, Resolution,
};
pub use java::{
    JavaRequirement, JavaRuntime, detect_java, discover as discover_java,
    requirement as java_requirement, select as select_java,
};
pub use launch::{
    Credentials, LaunchPlan, LaunchResult, LaunchVariables, launch_instance, offline_credentials,
};
pub use loader::{
    LoaderOption, LoaderVersion, display_name as loader_display_name,
    installable as installable_loaders, latest_version as latest_loader_version,
    list_versions as list_loader_versions,
};
pub use microsoft::{
    DeviceCodeChallenge, MicrosoftSession, begin_login as begin_microsoft_login,
    ensure_fresh as refresh_microsoft_session, finish_login as finish_microsoft_login,
};
pub use modrinth::{
    ProjectVersion, SearchHit, SearchResult, install as install_from_modrinth,
    search as search_modrinth, versions as modrinth_versions,
};
pub use mods::{
    ModFile, install as install_mod, list as list_mods, remove as remove_mod,
    set_enabled as set_mod_enabled,
};
pub use prepare::{PrepareResult, prepare_instance};
pub use runtime::{ensure_java, remove as remove_runtime};
pub use saves::{SaveEntry, list as list_saves};
pub use settings::{
    AccountKind, AccountSettings, DownloadSettings, Settings, SourcePreference,
    current as current_settings, load as load_settings, save as save_settings,
};
pub use tuning::{ModsProfile, heap_megabytes, mods_profile, physical_memory_bytes};
pub use version::{effective_id as effective_version_id, resolve as resolve_version};

/// Marker for the Pearl integration boundary.
///
/// Keeping the dependency in this crate makes Pearl a replaceable launcher
/// capability while Fern's metadata and launch pipeline remain independent.
pub fn pearl_dependency_present() -> bool {
    let _ = std::any::TypeId::of::<pearl_core::identity::NodeId>();
    true
}
