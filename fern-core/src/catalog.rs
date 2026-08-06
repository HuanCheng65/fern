use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{BmclapiSource, DownloadClient, OfficialSource};
use fern_meta::{VersionManifest, VersionManifestEntry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{DataPaths, InstanceId, InstanceProfile};

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOption {
    pub id: String,
    pub kind: String,
    pub release_time: String,
    pub url: String,
}

impl From<&VersionManifestEntry> for VersionOption {
    fn from(entry: &VersionManifestEntry) -> Self {
        Self {
            id: entry.id.clone(),
            kind: entry.kind.clone(),
            release_time: entry.release_time.clone().unwrap_or_default(),
            url: entry.url.clone(),
        }
    }
}

pub fn list_instances(paths: &DataPaths) -> Result<Vec<InstanceProfile>> {
    paths
        .ensure_exists()
        .context("create launcher data directories")?;
    let mut profiles = Vec::new();
    for entry in fs::read_dir(&paths.instances).context("read instances directory")? {
        let entry = entry.context("read instance entry")?;
        if !entry
            .file_type()
            .context("read instance entry type")?
            .is_dir()
        {
            continue;
        }
        let config = entry.path().join("instance.json");
        if !config.is_file() {
            continue;
        }
        let bytes = fs::read(&config).with_context(|| format!("read {}", config.display()))?;
        profiles.push(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("parse {}", config.display()))?,
        );
    }
    profiles.sort_by(|left: &InstanceProfile, right: &InstanceProfile| left.name.cmp(&right.name));
    Ok(profiles)
}

pub fn create_instance(
    paths: &DataPaths,
    name: &str,
    game_version: &str,
) -> Result<InstanceProfile> {
    let name = name.trim();
    let game_version = game_version.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(anyhow!("instance name must contain 1-64 characters"));
    }
    if game_version.is_empty() || game_version.len() > 32 {
        return Err(anyhow!("game version is required"));
    }
    paths
        .ensure_exists()
        .context("create launcher data directories")?;
    let base = slug_for(name);
    let id = unique_id(paths, &base)?;
    let profile = InstanceProfile::vanilla(InstanceId::parse(&id)?, name, game_version);
    let instance_root = paths.instance_root(&id);
    fs::create_dir_all(instance_root.join(".minecraft")).context("create game directory")?;
    let bytes = serde_json::to_vec_pretty(&profile).context("serialize instance profile")?;
    fs::write(paths.instance_config(&id), bytes).context("write instance profile")?;
    Ok(profile)
}

pub async fn list_versions() -> Result<Vec<VersionOption>> {
    let client = DownloadClient::new(vec![Arc::new(OfficialSource), Arc::new(BmclapiSource)], 4);
    let bytes = client
        .fetch(VERSION_MANIFEST_URL)
        .await
        .context("fetch version manifest")?;
    let manifest: VersionManifest =
        serde_json::from_slice(&bytes).context("parse version manifest")?;
    Ok(manifest.versions.iter().map(VersionOption::from).collect())
}

fn slug_for(name: &str) -> String {
    let mut slug = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() {
            slug.push(byte.to_ascii_lowercase() as char);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "instance".to_owned()
    } else {
        slug[..slug.len().min(48)].to_owned()
    }
}

fn unique_id(paths: &DataPaths, base: &str) -> Result<String> {
    let direct = paths.instance_root(base);
    if !direct.exists() {
        return Ok(base.to_owned());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| anyhow!(error))?
        .as_secs();
    for index in 1..1000 {
        let candidate = format!("{base}-{stamp}-{index}");
        if !paths.instance_root(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "unable to allocate an instance id under {}",
        paths.root.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_lists_instance_profiles_from_disk() {
        let root = std::env::temp_dir().join(format!("fern-catalog-test-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        let profile = create_instance(&paths, "我的世界", "1.21.1").expect("create instance");
        let profiles = list_instances(&paths).expect("list instances");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, profile.id);
        assert_eq!(profiles[0].name, "我的世界");
        fs::remove_dir_all(root).expect("remove test data");
    }
}
