use std::{collections::HashMap, path::PathBuf};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

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
}
