//! Fern's launcher orchestration boundary.
//!
//! Tauri commands call this crate. It owns launcher state and translates
//! download, Java, account, and process events into UI-facing values.

mod catalog;
mod data;
mod event;
mod instance;
mod java;
mod launch;
mod prepare;
mod runtime;
mod settings;
mod tuning;

pub use catalog::{VersionOption, create_instance, list_instances, list_versions};
pub use data::DataPaths;
pub use event::{LaunchStage, LauncherEvent, LogLevel};
pub use fern_download::DownloadEvent;
pub use fern_meta::{VersionManifest, VersionManifestEntry};
pub use instance::{
    CoverSeed, InstanceId, InstanceProfile, InstanceSettings, LoaderKind, LoaderProfile, Resolution,
};
pub use java::{
    JavaRequirement, JavaRuntime, detect_java, discover as discover_java,
    requirement as java_requirement, select as select_java,
};
pub use launch::{
    Credentials, LaunchPlan, LaunchResult, LaunchVariables, launch_instance, offline_credentials,
};
pub use prepare::{PrepareResult, prepare_instance};
pub use runtime::{
    disk_usage as runtime_disk_usage, ensure_java, installed as installed_runtimes,
    remove as remove_runtime,
};
pub use settings::{
    AccountKind, AccountSettings, DownloadSettings, Settings, SourcePreference,
    current as current_settings, load as load_settings, save as save_settings,
};
pub use tuning::{ModsProfile, heap_megabytes, physical_memory_bytes};

/// Marker for the Pearl integration boundary.
///
/// Keeping the dependency in this crate makes Pearl a replaceable launcher
/// capability while Fern's metadata and launch pipeline remain independent.
pub fn pearl_dependency_present() -> bool {
    let _ = std::any::TypeId::of::<pearl_core::identity::NodeId>();
    true
}
