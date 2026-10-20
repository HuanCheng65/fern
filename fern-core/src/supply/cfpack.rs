//! CurseForge 的整合包。
//!
//! 一个 zip：`manifest.json` 说清游戏版本、加载器和要装哪些文件，加一个
//! overrides 目录（名字写在清单里，几乎总是 `overrides`）直接铺进游戏目录。
//!
//! 和 mrpack 最大的不同是**清单里没有下载地址**，只有项目 id 和文件 id——
//! 下载地址、文件名、sha1 都要拿 id 去问 CurseForge。所以读清单不用联网，
//! 而知道「要下什么、能不能下」要。
//!
//! 有些文件作者不允许第三方下载，API 不给地址。热门整合包里常有几个这样的
//! 文件，它们走手动下载那条路（见 `sideload.rs`）：在建实例**之前**就要把它们
//! 全部找齐，缺一个都不开工——装到一半才发现，留下的是一个起不来的实例。

use std::{collections::HashMap, io::Read, path::Path};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use super::{curseforge, sideload::Sideload};
use crate::LoaderKind;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    #[serde(default)]
    manifest_type: String,
    minecraft: Minecraft,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    files: Vec<ManifestFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

fn default_overrides() -> String {
    "overrides".to_owned()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Minecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<ModLoader>,
}

#[derive(Debug, Deserialize)]
struct ModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    /// 作者标成可选的文件，CurseForge 自己的客户端默认也不装。
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

impl Manifest {
    pub(super) fn game_version(&self) -> &str {
        &self.minecraft.version
    }

    /// 加载器和它的版本。`forge-47.2.0` 这种写法：名字和版本之间是第一个 `-`。
    pub(super) fn loader(&self) -> (LoaderKind, String) {
        let chosen = self
            .minecraft
            .mod_loaders
            .iter()
            .find(|loader| loader.primary)
            .or(self.minecraft.mod_loaders.first());
        let Some((name, version)) = chosen.and_then(|loader| loader.id.split_once('-')) else {
            return (LoaderKind::Vanilla, String::new());
        };
        let kind = match name.to_ascii_lowercase().as_str() {
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            _ => return (LoaderKind::Vanilla, String::new()),
        };
        (kind, version.to_owned())
    }

    /// 要装的那些（只算必需的）。
    pub(super) fn wanted(&self) -> Vec<(u64, u64)> {
        self.files
            .iter()
            .filter(|file| file.required)
            .map(|file| (file.project_id, file.file_id))
            .collect()
    }

    /// overrides 目录在 zip 里的前缀。清单里的名字来自网络，只认一层普通的
    /// 目录名；其余的当作默认值。
    pub(super) fn overrides_prefix(&self) -> String {
        let name = self.overrides.trim_matches('/');
        let plain = !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..";
        format!("{}/", if plain { name } else { "overrides" })
    }
}

/// 从 zip 里读清单。不是 CurseForge 的整合包就返回 `None`，让调用方去试别的格式。
pub(super) fn read(archive_path: &Path) -> Result<Option<Manifest>> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("打开 {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("这个文件不是有效的 zip")?;
    let Ok(mut entry) = archive.by_name("manifest.json") else {
        return Ok(None);
    };
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .context("读取 manifest.json")?;
    let manifest: Manifest = serde_json::from_str(&text).context("解析 manifest.json")?;
    if manifest.manifest_type != "minecraftModpack" {
        return Err(anyhow!("manifest.json 不是 Minecraft 整合包的清单"));
    }
    Ok(Some(manifest))
}

/// 整合包里一个要放进游戏目录的文件。
#[derive(Debug, Clone)]
pub(super) struct PackFile {
    pub title: String,
    /// 相对游戏目录。
    pub path: String,
    pub file_name: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: u64,
    /// 要手动下载时的网页。
    pub page: Option<String>,
    /// 手动下的那一份。
    pub local: Option<std::path::PathBuf>,
}

/// 界面上「这几个要手动下载」那一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualFile {
    pub title: String,
    pub file_name: String,
    pub page: String,
    pub found: bool,
}

impl PackFile {
    pub(super) fn manual(&self) -> Option<ManualFile> {
        if self.url.is_some() {
            return None;
        }
        Some(ManualFile {
            title: self.title.clone(),
            file_name: self.file_name.clone(),
            page: self.page.clone().unwrap_or_default(),
            found: self.local.is_some(),
        })
    }
}

/// 把清单里的 id 换成一个个具体的文件：放哪、从哪下、sha1 是多少。
pub(super) async fn resolve(manifest: &Manifest, sideload: &Sideload) -> Result<Vec<PackFile>> {
    let wanted = manifest.wanted();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let projects: Vec<u64> = wanted.iter().map(|(project, _)| *project).collect();
    let files: Vec<u64> = wanted.iter().map(|(_, file)| *file).collect();
    let info: HashMap<u64, curseforge::PackProject> = curseforge::pack_projects(&projects).await?;
    let found = curseforge::pack_files(&files).await?;

    let blocked: Vec<String> = found
        .iter()
        .filter(|file| file.url.is_none())
        .filter_map(|file| file.sha1.clone())
        .collect();
    let mirrors = super::mirrored_on_modrinth(&blocked).await;

    let mut out = Vec::new();
    for (project, file_id) in wanted {
        let Some(file) = found.iter().find(|file| file.id == file_id) else {
            return Err(anyhow!("CurseForge 上找不到整合包里的文件 {file_id}"));
        };
        let about = info.get(&project);
        // 文件名来自网络，要拼进路径：只认一个普通的文件名。
        if Path::new(&file.name).file_name().and_then(|it| it.to_str()) != Some(file.name.as_str())
        {
            return Err(anyhow!("整合包里的文件名不安全：{}", file.name));
        }
        let directory = about.map_or("mods", |about| about.directory);
        let url = file.url.clone().or_else(|| {
            file.sha1
                .as_ref()
                .and_then(|sha1| mirrors.get(sha1))
                .cloned()
        });
        let local = match url {
            Some(_) => None,
            None => sideload.locate(&file.name, file.sha1.as_deref(), file.size),
        };
        out.push(PackFile {
            title: about.map_or_else(|| file.name.clone(), |about| about.title.clone()),
            path: format!("{directory}/{}", file.name),
            file_name: file.name.clone(),
            url,
            sha1: file.sha1.clone(),
            size: file.size,
            page: about
                .and_then(|about| about.page.as_ref())
                .map(|page| format!("{page}/files/{file_id}")),
            local,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        serde_json::from_str(json).expect("parse")
    }

    #[test]
    fn the_loader_is_read_from_its_id() {
        let pack = manifest(
            r#"{"manifestType":"minecraftModpack","name":"P",
                "minecraft":{"version":"1.20.1","modLoaders":[
                  {"id":"fabric-0.15.0","primary":false},
                  {"id":"forge-47.2.0","primary":true}]},
                "files":[]}"#,
        );
        assert_eq!(pack.game_version(), "1.20.1");
        // 标了 primary 的那个说了算，不是第一个。
        assert_eq!(pack.loader(), (LoaderKind::Forge, "47.2.0".to_owned()));

        let neo = manifest(
            r#"{"manifestType":"minecraftModpack",
                "minecraft":{"version":"1.21.1","modLoaders":[{"id":"neoforge-21.1.77"}]}}"#,
        );
        assert_eq!(neo.loader(), (LoaderKind::NeoForge, "21.1.77".to_owned()));
    }

    #[test]
    fn optional_files_are_left_out() {
        let pack = manifest(
            r#"{"manifestType":"minecraftModpack","minecraft":{"version":"1.20.1"},
                "files":[{"projectID":1,"fileID":10,"required":true},
                         {"projectID":2,"fileID":20,"required":false},
                         {"projectID":3,"fileID":30}]}"#,
        );
        assert_eq!(pack.wanted(), vec![(1, 10), (3, 30)]);
    }

    #[test]
    fn the_overrides_folder_cannot_point_elsewhere() {
        let with = |name: &str| {
            manifest(&format!(
                r#"{{"manifestType":"minecraftModpack","minecraft":{{"version":"1"}},
                    "overrides":{}}}"#,
                serde_json::to_string(name).expect("quote")
            ))
            .overrides_prefix()
        };
        assert_eq!(with("overrides"), "overrides/");
        assert_eq!(with("custom"), "custom/");
        // 名字来自网络。爬目录、带斜杠的一律当默认值。
        assert_eq!(with(".."), "overrides/");
        assert_eq!(with("a/b"), "overrides/");
        assert_eq!(with(""), "overrides/");
    }
}
