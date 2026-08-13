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
//!
//! **清单怎么读不写在这里**，走 [`jar::label`](super::jar::label)。这一层曾经
//! 自己抄了一份 fabric/quilt/mods.toml 的解析，于是「按加载器的宽容度读」这类
//! 修补补了预检查那份、忘了这份——同一个 jar 在预检查里认得出、在列表里只剩个
//! 文件名。只留一份，就没有第二处可以漂移。

use std::path::{Path, PathBuf};

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
        let label = super::jar::label(&entry.path());
        mods.push(ModFile {
            name: label
                .as_ref()
                .and_then(|it| it.name.clone())
                .unwrap_or_else(|| super::jar::display_name(&file_name)),
            version: label.and_then(|it| it.version),
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
    crate::backup::before_mod_change(
        paths,
        instance_id,
        Some(
            crate::backup::manifest::About::new(if enabled { "enable" } else { "disable" })
                .with("name", file_name.trim_end_matches(DISABLED)),
        ),
    );
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
    crate::backup::before_mod_change(
        paths,
        instance_id,
        Some(
            crate::backup::manifest::About::new("remove")
                .with("name", file_name.trim_end_matches(DISABLED)),
        ),
    );
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
    crate::backup::before_mod_change(
        paths,
        instance_id,
        Some(crate::backup::manifest::About::new("install").with("name", file_name.as_str())),
    );
    let directory = mods_directory(paths, instance_id)?;
    std::fs::create_dir_all(&directory)?;
    let destination = safe_entry(&directory, &file_name)?;
    std::fs::copy(source, &destination)
        .with_context(|| format!("复制到 {}", destination.display()))?;

    let label = super::jar::label(&destination);

    if let Ok(sha1) = crate::backup::sha1_of(&destination) {
        crate::instance::origin::record(
            paths,
            instance_id,
            vec![crate::instance::origin::Entry {
                file: format!("mods/{file_name}"),
                sha1,
                version: label.as_ref().and_then(|it| it.version.clone()),
                origin: crate::instance::origin::Origin::Import,
            }],
        );
    }

    Ok(ModFile {
        name: label
            .as_ref()
            .and_then(|it| it.name.clone())
            .unwrap_or_else(|| super::jar::display_name(&file_name)),
        version: label.and_then(|it| it.version),
        bytes: std::fs::metadata(&destination)
            .map(|m| m.len())
            .unwrap_or(0),
        file_name,
        enabled: true,
    })
}

fn mods_directory(paths: &DataPaths, instance_id: &str) -> Result<PathBuf> {
    let id = crate::InstanceId::parse(instance_id).map_err(|error| anyhow!("{error}"))?;
    Ok(crate::instance::paths_by_id(paths, id.as_str())
        .game_directory(id.as_str())
        .join("mods"))
}

/// 文件名来自界面，必须挡住路径穿越。
fn safe_entry(directory: &Path, file_name: &str) -> Result<PathBuf> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(anyhow!("非法的文件名：{file_name}"));
    }
    Ok(directory.join(file_name))
}

/// 模组在 jar 里自己声明的版本号。
///
/// 对账要它：内容变了而这个版本号没变，和用户换了个版本，是两件完全不同的事
/// （见 `integrity.rs`）。读不到就是 `None`——资源包和光影本来就没有。
pub(crate) fn declared_version(path: &Path) -> Option<String> {
    super::jar::label(path).and_then(|label| label.version)
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
