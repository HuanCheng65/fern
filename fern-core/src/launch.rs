use std::{
    collections::HashMap,
    fs::File,
    io,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow};
use fern_meta::{Library, RuleContext, VersionMetadata, rules_allow};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::DataPaths;

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
) -> Result<LaunchResult> {
    if !(3..=16).contains(&player_name.len())
        || !player_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(anyhow!(
            "offline player name must contain 3-16 ASCII letters, numbers or '_'"
        ));
    }
    paths.ensure_exists()?;
    let profile = crate::list_instances(paths)?
        .into_iter()
        .find(|profile| profile.id.as_str() == instance_id)
        .ok_or_else(|| anyhow!("instance {instance_id} does not exist"))?;
    let version_id = profile.game_version.clone();
    let version_root = paths.versions.join(&version_id);
    let metadata_path = version_root.join(format!("{version_id}.json"));
    let metadata_bytes = tokio::fs::read(&metadata_path)
        .await
        .with_context(|| format!("read prepared metadata {}", metadata_path.display()))?;
    let metadata: VersionMetadata =
        serde_json::from_slice(&metadata_bytes).context("parse prepared version metadata")?;
    let main_class = metadata
        .main_class
        .clone()
        .ok_or_else(|| anyhow!("version {version_id} has no main class"))?;

    let context = current_rule_context(profile.settings.resolution.is_some());
    let natives_directory = paths.game_directory(instance_id).join("natives");
    tokio::fs::create_dir_all(&natives_directory).await?;
    let classpath =
        collect_classpath_and_extract_natives(paths, &metadata, &context, &natives_directory)
            .await?;
    let client_jar = version_root.join(format!("{version_id}.jar"));
    if !tokio::fs::try_exists(&client_jar).await? {
        return Err(anyhow!("client jar is missing: {}", client_jar.display()));
    }

    let credentials = offline_credentials(player_name);
    let mut variables = LaunchVariables::new().with_credentials(&credentials);
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
    let (jvm_arguments, game_arguments) = metadata.resolved_arguments(&context);
    let (java_binary, java_major, java_version) =
        resolve_java_binary(profile.settings.java_path.as_deref())?;
    let required_java_major = metadata
        .java_version
        .as_ref()
        .map(|version| version.major_version);
    if let Some(required) = required_java_major {
        if java_major < required {
            return Err(anyhow!(
                "Java {java_major} is incompatible with Minecraft {version_id}; Java {required} or newer is required (detected {java_version})"
            ));
        }
    }
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
    let mut child = Command::new(&java_binary)
        .args(arguments)
        .current_dir(&plan.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start Java from {}", java_binary.display()))?;
    append_launch_log(&launch_log, &format!("started pid={}", child.id()))?;
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, launch_log.clone(), "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, launch_log.clone(), "stderr");
    }
    let process_id = child.id();
    let wait_log = launch_log.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) => {
            let code = status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string());
            let _ = append_launch_log(&wait_log, &format!("exited code={code}"));
        }
        Err(error) => {
            let _ = append_launch_log(&wait_log, &format!("wait error={error}"));
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

fn append_launch_log(path: &Path, message: &str) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{} {message}", chrono_like_timestamp())
}

fn spawn_log_reader<R>(reader: R, path: PathBuf, stream: &'static str)
where
    R: io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        let mut file = io::BufWriter::new(file);
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            let _ = writeln!(file, "[{}] {line}", stream);
        }
        let _ = file.flush();
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

fn resolve_java_binary(configured: Option<&Path>) -> Result<(PathBuf, u16, String)> {
    let candidate = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("java"));
    let output = Command::new(&candidate)
        .arg("-version")
        .output()
        .with_context(|| format!("find Java executable {}", candidate.display()))?;
    let version_output = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(anyhow!(
            "Java executable {} failed version check",
            candidate.display()
        ));
    }
    let java_major = parse_java_major(&version_output).ok_or_else(|| {
        anyhow!(
            "unable to determine Java version for {} ({})",
            candidate.display(),
            version_output.trim()
        )
    })?;
    Ok((candidate, java_major, version_output.trim().to_owned()))
}

fn parse_java_major(output: &str) -> Option<u16> {
    let version = output
        .split_once("version \"")
        .and_then(|(_, value)| value.split('"').next())
        .or_else(|| {
            output
                .lines()
                .find_map(|line| line.trim().strip_prefix("openjdk "))
        })?;
    let version = version.strip_prefix("1.").unwrap_or(version);
    version.split('.').next()?.parse().ok()
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
            } else if !library.name.contains(":natives-") {
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

    #[test]
    fn parses_modern_and_legacy_java_versions() {
        assert_eq!(
            parse_java_major("openjdk version \"21.0.8\" 2025-07-15"),
            Some(21)
        );
        assert_eq!(parse_java_major("java version \"1.8.0_402\""), Some(8));
        assert_eq!(parse_java_major("openjdk 24.0.1 2025-04-15"), Some(24));
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
