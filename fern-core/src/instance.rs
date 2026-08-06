use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const INSTANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(String);

impl InstanceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InstanceIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if valid {
            Ok(Self(value))
        } else {
            Err(InstanceIdError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceIdError;

impl fmt::Display for InstanceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("instance id must contain 1-64 ASCII letters, numbers, '-' or '_'")
    }
}

impl std::error::Error for InstanceIdError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderProfile {
    pub kind: LoaderKind,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverSeed {
    pub identity: String,
    #[serde(default)]
    pub growth: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProfile {
    pub schema_version: u32,
    pub id: InstanceId,
    pub name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_profile: Option<LoaderProfile>,
    pub cover: CoverSeed,
    #[serde(default)]
    pub settings: InstanceSettings,
}

impl InstanceProfile {
    pub fn vanilla(
        id: InstanceId,
        name: impl Into<String>,
        game_version: impl Into<String>,
    ) -> Self {
        let cover_identity = id.as_str().to_owned();
        Self {
            schema_version: INSTANCE_SCHEMA_VERSION,
            id,
            name: name.into(),
            game_version: game_version.into(),
            loader: LoaderKind::Vanilla,
            loader_profile: None,
            cover: CoverSeed {
                identity: cover_identity,
                growth: 0,
            },
            settings: InstanceSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_profile_has_a_stable_json_shape() {
        let profile = InstanceProfile::vanilla(
            InstanceId::parse("cinder-valley").expect("valid id"),
            "Cinder Valley",
            "1.21.1",
        );
        let value = serde_json::to_value(&profile).expect("serialize profile");

        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["gameVersion"], "1.21.1");
        assert_eq!(value["loader"], "vanilla");
        assert_eq!(value["cover"]["identity"], "cinder-valley");
    }

    #[test]
    fn instance_ids_are_safe_as_directory_names() {
        assert!(InstanceId::parse("moss_archive-2").is_ok());
        assert!(InstanceId::parse("../other").is_err());
        assert!(InstanceId::parse("contains spaces").is_err());
    }
}
