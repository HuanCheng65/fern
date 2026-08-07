//! 实例里装了哪些模组。
//!
//! 「装了什么、开着没有」是实例内的状态，和「去哪里找一个模组」是两件事——
//! 后者属于补给站。这里只管已经落在 `mods/` 里的那些。
//!
//! 启用与禁用靠改扩展名：`foo.jar` ↔ `foo.jar.disabled`。这是 Fabric、Forge、
//! NeoForge 都认的社区约定——三家的加载器都只扫 `.jar`，所以加个后缀就等于
//! 关掉它，而文件还在，随时能开回来。比移到另一个目录好：用户在文件管理器里
//! 看到的仍然是同一份东西。
//!
//! 展示名从 jar 里读。文件名（`jei-1.21.1-neoforge-19.21.0.247.jar`）认得出来
//! 但读起来费劲，而每个加载器都在 jar 里放了一份正经的元数据。读不到就退回
//! 文件名——列表里宁可显示一个丑名字，也不能少一行。

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::DataPaths;

/// 关掉一个模组时加的后缀。
const DISABLED: &str = ".disabled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    /// 磁盘上的文件名，禁用时带 `.disabled`。所有操作都按它定位。
    pub file_name: String,
    /// 展示名。读不到元数据时就是文件名。
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub enabled: bool,
    pub bytes: u64,
}

/// 这个实例的 mods 目录里有什么，按展示名排序。
pub fn list(paths: &DataPaths, instance_id: &str) -> Result<Vec<ModFile>> {
    let directory = mods_directory(paths, instance_id)?;
    let Ok(entries) = std::fs::read_dir(&directory) else {
        // 没有 mods 目录是正常状态：原版实例，或者还没装过东西。
        return Ok(Vec::new());
    };

    let mut mods = Vec::new();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let enabled = file_name.ends_with(".jar");
        if !enabled && !file_name.ends_with(".jar.disabled") {
            // mods 目录里常有 README、配置残留之类的东西，不是模组。
            continue;
        }
        let described = describe(&entry.path());
        mods.push(ModFile {
            name: described
                .as_ref()
                .and_then(|d| d.name.clone())
                .unwrap_or_else(|| display_from_file_name(&file_name)),
            version: described.and_then(|d| d.version),
            file_name,
            enabled,
            bytes: metadata.len(),
        });
    }

    mods.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.file_name.cmp(&right.file_name))
    });
    Ok(mods)
}

/// 开或关一个模组。返回改名之后的文件名。
pub fn set_enabled(
    paths: &DataPaths,
    instance_id: &str,
    file_name: &str,
    enabled: bool,
) -> Result<String> {
    let directory = mods_directory(paths, instance_id)?;
    let current = safe_entry(&directory, file_name)?;
    let target_name = if enabled {
        file_name.trim_end_matches(DISABLED).to_owned()
    } else if file_name.ends_with(DISABLED) {
        file_name.to_owned()
    } else {
        format!("{file_name}{DISABLED}")
    };

    if target_name == file_name {
        return Ok(target_name);
    }
    let target = directory.join(&target_name);
    std::fs::rename(&current, &target).with_context(|| format!("重命名 {}", current.display()))?;
    Ok(target_name)
}

/// 删掉一个模组。
pub fn remove(paths: &DataPaths, instance_id: &str, file_name: &str) -> Result<()> {
    let directory = mods_directory(paths, instance_id)?;
    let path = safe_entry(&directory, file_name)?;
    std::fs::remove_file(&path).with_context(|| format!("删除 {}", path.display()))
}

/// 把一个本地 jar 装进实例。
///
/// 用户从文件管理器里拖进来的那条路径。同名文件已存在就覆盖——他多半就是想
/// 换一个版本。
pub fn install(paths: &DataPaths, instance_id: &str, source: &Path) -> Result<ModFile> {
    let file_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("{} 不是一个文件", source.display()))?;
    if !file_name.ends_with(".jar") {
        return Err(anyhow!("{file_name} 不是 jar 文件"));
    }
    let directory = mods_directory(paths, instance_id)?;
    std::fs::create_dir_all(&directory)?;
    let destination = safe_entry(&directory, &file_name)?;
    std::fs::copy(source, &destination)
        .with_context(|| format!("复制到 {}", destination.display()))?;

    let described = describe(&destination);
    Ok(ModFile {
        name: described
            .as_ref()
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| display_from_file_name(&file_name)),
        version: described.and_then(|d| d.version),
        bytes: std::fs::metadata(&destination)
            .map(|m| m.len())
            .unwrap_or(0),
        file_name,
        enabled: true,
    })
}

fn mods_directory(paths: &DataPaths, instance_id: &str) -> Result<PathBuf> {
    let id = crate::InstanceId::parse(instance_id).map_err(|error| anyhow!("{error}"))?;
    Ok(paths.game_directory(id.as_str()).join("mods"))
}

/// 文件名来自界面，必须挡住路径穿越。
fn safe_entry(directory: &Path, file_name: &str) -> Result<PathBuf> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(anyhow!("非法的文件名：{file_name}"));
    }
    Ok(directory.join(file_name))
}

/// `jei-1.21.1-neoforge-19.21.0.247.jar` → `jei-1.21.1-neoforge-19.21.0.247`
fn display_from_file_name(file_name: &str) -> String {
    file_name
        .trim_end_matches(DISABLED)
        .trim_end_matches(".jar")
        .to_owned()
}

#[derive(Debug, Default)]
struct Described {
    name: Option<String>,
    version: Option<String>,
}

/// 从 jar 里读元数据。三家的格式各不相同，都试一遍。
fn describe(path: &Path) -> Option<Described> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    if let Some(described) = read_entry(&mut archive, "fabric.mod.json").and_then(|t| fabric(&t)) {
        return Some(described);
    }
    if let Some(described) = read_entry(&mut archive, "quilt.mod.json").and_then(|t| quilt(&t)) {
        return Some(described);
    }
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(text) = read_entry(&mut archive, name) {
            let mut described = forge(&text)?;
            // Forge 常把版本写成 `${file.jarVersion}`，真值在 MANIFEST 里。
            if described
                .version
                .as_deref()
                .is_some_and(|version| version.contains("${"))
            {
                described.version = read_entry(&mut archive, "META-INF/MANIFEST.MF")
                    .and_then(|manifest| manifest_value(&manifest, "Implementation-Version"));
            }
            return Some(described);
        }
    }
    None
}

fn read_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

fn fabric(text: &str) -> Option<Described> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(Described {
        name: string_at(&value, "name").or_else(|| string_at(&value, "id")),
        version: string_at(&value, "version"),
    })
}

fn quilt(text: &str) -> Option<Described> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let loader = value.get("quilt_loader")?;
    Some(Described {
        name: loader
            .get("metadata")
            .and_then(|metadata| string_at(metadata, "name"))
            .or_else(|| string_at(loader, "id")),
        version: string_at(loader, "version"),
    })
}

/// `mods.toml` 只需要 `[[mods]]` 第一段里的两个字段。
///
/// 不引 TOML 解析器：这份文件里我们要的两个键都是简单的 `key = "value"`，而
/// 引一个解析器要连它的错误处理、版本策略一起背上。真出现引不到的写法，退回
/// 文件名即可，不是灾难。
fn forge(text: &str) -> Option<Described> {
    let mods = text.find("[[mods]]")?;
    let section = &text[mods..];
    // 到下一个表头为止，免得把别的 `[[mods]]` 段的值混进来。
    let section = section[8..]
        .find("[[")
        .map(|end| &section[..end + 8])
        .unwrap_or(section);
    Some(Described {
        name: toml_string(section, "displayName").or_else(|| toml_string(section, "modId")),
        version: toml_string(section, "version"),
    })
}

fn toml_string(section: &str, key: &str) -> Option<String> {
    section.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix(key)?;
        let rest = rest.trim_start().strip_prefix('=')?.trim();
        let value = rest.strip_prefix('"')?;
        let end = value.find('"')?;
        Some(value[..end].to_owned())
    })
}

/// MANIFEST 的续行以空格开头，长值一定会折行。
fn manifest_value(text: &str, key: &str) -> Option<String> {
    let mut value: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
            value = Some(rest.trim().to_owned());
        } else if let Some(continuation) = line.strip_prefix(' ') {
            if let Some(current) = value.as_mut() {
                current.push_str(continuation.trim_end_matches(['\r', '\n']));
            }
        } else if value.is_some() {
            break;
        }
    }
    value.filter(|value| !value.is_empty())
}

fn string_at(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(directory: &Path, file_name: &str, entries: &[(&str, &str)]) {
        std::fs::create_dir_all(directory).expect("create directory");
        let mut writer =
            zip::ZipWriter::new(std::fs::File::create(directory.join(file_name)).expect("create"));
        for (name, body) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            writer.write_all(body.as_bytes()).expect("write");
        }
        writer.finish().expect("finish");
    }

    fn instance(root: &Path) -> DataPaths {
        let paths = DataPaths::new(root);
        std::fs::create_dir_all(paths.game_directory("moss").join("mods")).expect("create mods");
        paths
    }

    #[test]
    fn reads_the_display_name_out_of_each_loaders_metadata() {
        let root = std::env::temp_dir().join(format!("fern-mods-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = instance(&root);
        let mods = paths.game_directory("moss").join("mods");

        jar(
            &mods,
            "sodium.jar",
            &[(
                "fabric.mod.json",
                r#"{"id":"sodium","name":"Sodium","version":"0.6.0"}"#,
            )],
        );
        jar(
            &mods,
            "jei.jar",
            &[(
                "META-INF/neoforge.mods.toml",
                "modLoader=\"javafml\"\n[[mods]]\nmodId=\"jei\"\nversion=\"19.21.0\"\ndisplayName=\"Just Enough Items\"\n",
            )],
        );
        jar(
            &mods,
            "qsl.jar",
            &[(
                "quilt.mod.json",
                r#"{"quilt_loader":{"id":"qsl","version":"7.0.0","metadata":{"name":"Quilt Standard Libraries"}}}"#,
            )],
        );
        // 没有元数据的也要出现在列表里，不能被吞掉。
        jar(&mods, "mystery-1.2.3.jar", &[("README.txt", "hello")]);

        let listed = list(&paths, "moss").expect("list mods");
        let names: Vec<_> = listed.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Just Enough Items",
                "mystery-1.2.3",
                "Quilt Standard Libraries",
                "Sodium"
            ]
        );
        let jei = listed.iter().find(|m| m.name.starts_with("Just")).unwrap();
        assert_eq!(jei.version.as_deref(), Some("19.21.0"));

        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn forge_version_placeholders_fall_back_to_the_manifest() {
        // `${file.jarVersion}` 直接显示出来毫无意义，真值在 MANIFEST 里。
        let root = std::env::temp_dir().join(format!("fern-mods-ver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = instance(&root);
        let mods = paths.game_directory("moss").join("mods");
        jar(
            &mods,
            "create.jar",
            &[
                (
                    "META-INF/mods.toml",
                    "[[mods]]\nmodId=\"create\"\ndisplayName=\"Create\"\nversion=\"${file.jarVersion}\"\n",
                ),
                (
                    "META-INF/MANIFEST.MF",
                    "Manifest-Version: 1.0\nImplementation-Version: 6.0.4\n",
                ),
            ],
        );
        let listed = list(&paths, "moss").expect("list mods");
        assert_eq!(listed[0].version.as_deref(), Some("6.0.4"));
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn disabling_keeps_the_file_and_can_be_undone() {
        let root = std::env::temp_dir().join(format!("fern-mods-toggle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = instance(&root);
        let mods = paths.game_directory("moss").join("mods");
        jar(
            &mods,
            "sodium.jar",
            &[("fabric.mod.json", r#"{"id":"sodium","name":"Sodium"}"#)],
        );

        let disabled = set_enabled(&paths, "moss", "sodium.jar", false).expect("disable");
        assert_eq!(disabled, "sodium.jar.disabled");
        assert!(mods.join("sodium.jar.disabled").is_file());
        assert!(!mods.join("sodium.jar").exists());

        // 关掉之后仍然要出现在列表里，只是标成停用——否则用户会以为它没了。
        let listed = list(&paths, "moss").expect("list mods");
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].enabled);
        assert_eq!(listed[0].name, "Sodium");

        let enabled = set_enabled(&paths, "moss", "sodium.jar.disabled", true).expect("enable");
        assert_eq!(enabled, "sodium.jar");
        assert!(list(&paths, "moss").expect("list")[0].enabled);

        // 重复操作要是幂等的，界面上双击不该出 `.disabled.disabled`。
        set_enabled(&paths, "moss", "sodium.jar", true).expect("enable again");
        assert!(mods.join("sodium.jar").is_file());

        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn file_names_from_the_interface_cannot_escape_the_mods_directory() {
        let root = std::env::temp_dir().join(format!("fern-mods-esc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = instance(&root);
        for evil in ["../../settings.json", "..\\..\\x", "a/b.jar", ".."] {
            assert!(remove(&paths, "moss", evil).is_err(), "{evil} 应当被拒绝");
            assert!(set_enabled(&paths, "moss", evil, false).is_err(), "{evil}");
        }
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn non_jar_files_in_the_mods_directory_are_ignored() {
        let root = std::env::temp_dir().join(format!("fern-mods-junk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = instance(&root);
        let mods = paths.game_directory("moss").join("mods");
        std::fs::write(mods.join("README.txt"), "not a mod").expect("write");
        std::fs::write(mods.join("options.txt"), "junk").expect("write");
        assert!(list(&paths, "moss").expect("list").is_empty());
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn a_missing_mods_directory_is_not_an_error() {
        // 原版实例、或者还没装过东西——都是正常状态。
        let root = std::env::temp_dir().join(format!("fern-mods-none-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        assert!(list(&paths, "moss").expect("list").is_empty());
    }
}
