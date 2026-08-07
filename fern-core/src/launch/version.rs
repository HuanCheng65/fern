//! 一个实例实际要启动的那份版本描述（文档 §1.2）。
//!
//! 装了加载器之后，磁盘上就有两份 JSON：原版的，和 Fabric/Forge 生成的那份
//! 「修改版」。后者用 `inheritsFrom` 指向前者，只写自己改动的部分。真正能拿
//! 去启动的是两份合并之后的结果。
//!
//! 合并规则在 `fern-meta` 里（那是协议知识），这里只负责把链从磁盘上跟完：
//! 读子版本 → 看 `inheritsFrom` → 读父版本 → 合并，一直到没有父为止。
//!
//! 补全和启动都要这份合并结果，而且**必须是同一份**：补全按 A 下文件、启动
//! 按 B 拼 classpath，就会出现「文件明明下好了却说缺」这种最难查的问题。所以
//! 只有这一个入口。

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use fern_meta::VersionMetadata;

use crate::{DataPaths, InstanceProfile, LoaderKind};

/// 继承链最多跟这么深。实际最多两级（原版 ← 加载器），留点余量，主要是防
/// 一份写坏的（或者恶意的）JSON 指回自己让我们一直读下去。
const MAX_DEPTH: usize = 8;

/// 这个实例要启动的版本 id。
///
/// 原版就是版本号本身；装了加载器则是加载器生成的那个 id
/// （`fabric-loader-0.16.5-1.21.1`），因为那才是带 mainClass 的那一份。
pub fn effective_id(profile: &InstanceProfile) -> String {
    match (&profile.loader, &profile.loader_profile) {
        (LoaderKind::Vanilla, _) | (_, None) => profile.game_version.clone(),
        // 还没装完（version_id 为空）就先按原版走：能启动一个原版，比因为
        // 加载器没装好而完全打不开要好。
        (_, Some(loader)) if loader.version_id.is_empty() => profile.game_version.clone(),
        (_, Some(loader)) => loader.version_id.clone(),
    }
}

/// 版本 id 能不能拿去当目录名。
///
/// 这些 id 全都来自网络——Mojang 的清单、加载器的 profile、以及别人写的
/// `inheritsFrom`——而它们会被直接拼进 `versions/<id>/<id>.json`。实例 id
/// 那套规则在这里不能用：版本号本来就带点（`1.21.1`、`1.21-pre1`），一律
/// 禁掉点就没有一个真实版本能过。所以放行点，但堵死 `..`。
pub fn is_safe_id(version_id: &str) -> bool {
    !version_id.is_empty()
        && version_id.len() <= 128
        && !version_id.contains("..")
        && !version_id.starts_with('.')
        && version_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

pub fn metadata_path(paths: &DataPaths, version_id: &str) -> PathBuf {
    paths
        .versions
        .join(version_id)
        .join(format!("{version_id}.json"))
}

/// 读一份版本 JSON，不跟继承链。
pub fn read_one(paths: &DataPaths, version_id: &str) -> Result<VersionMetadata> {
    // 挡在读之前而不是写之前：`inheritsFrom` 是别人 JSON 里的一个字符串，
    // 它指向哪里由不得我们，但读哪个文件由得。
    if !is_safe_id(version_id) {
        return Err(anyhow!("版本 id 无法作为目录名：{version_id}"));
    }
    let path = metadata_path(paths, version_id);
    let bytes = fs::read(&path).with_context(|| format!("读取 {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("解析 {}", path.display()))
}

/// 读出来并把继承链合并完。
pub fn resolve(paths: &DataPaths, version_id: &str) -> Result<VersionMetadata> {
    resolve_at(paths, version_id, 0)
}

fn resolve_at(paths: &DataPaths, version_id: &str, depth: usize) -> Result<VersionMetadata> {
    if depth >= MAX_DEPTH {
        return Err(anyhow!("{version_id} 的继承链过深，可能存在循环引用"));
    }
    let child = read_one(paths, version_id)?;
    let Some(parent_id) = child.inherits_from.clone() else {
        return Ok(child);
    };
    if parent_id == version_id {
        return Err(anyhow!("{version_id} 继承了自身"));
    }
    let parent = resolve_at(paths, &parent_id, depth + 1)
        .with_context(|| format!("{version_id} 继承自 {parent_id}"))?;
    Ok(VersionMetadata::merge(&parent, &child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverSeed, InstanceId, InstanceSettings, LoaderProfile};

    fn write(paths: &DataPaths, id: &str, json: serde_json::Value) {
        let path = metadata_path(paths, id);
        fs::create_dir_all(path.parent().expect("version directory")).expect("create directory");
        fs::write(path, serde_json::to_vec(&json).expect("serialize")).expect("write metadata");
    }

    fn profile(loader: LoaderKind, loader_version: Option<&str>) -> InstanceProfile {
        InstanceProfile {
            schema_version: 1,
            id: InstanceId::parse("moss").expect("valid id"),
            name: "Moss".to_owned(),
            game_version: "1.21.1".to_owned(),
            loader,
            loader_profile: loader_version.map(|version_id| LoaderProfile {
                kind: loader,
                version: "0.16.5".to_owned(),
                version_id: version_id.to_owned(),
            }),
            cover: CoverSeed {
                identity: "moss".to_owned(),
                growth: 0,
            },
            settings: InstanceSettings::default(),
            account_id: None,
            external: None,
            last_played: None,
        }
    }

    #[test]
    fn the_launchable_version_is_the_loaders_when_there_is_one() {
        assert_eq!(effective_id(&profile(LoaderKind::Vanilla, None)), "1.21.1");
        assert_eq!(
            effective_id(&profile(
                LoaderKind::Fabric,
                Some("fabric-loader-0.16.5-1.21.1")
            )),
            "fabric-loader-0.16.5-1.21.1"
        );
        // 标了加载器却没有 profile，只能按原版走——总比读一个不存在的文件好。
        assert_eq!(effective_id(&profile(LoaderKind::Fabric, None)), "1.21.1");
        assert_eq!(
            effective_id(&profile(LoaderKind::Fabric, Some(""))),
            "1.21.1"
        );
    }

    #[test]
    fn a_loader_profile_is_merged_over_the_vanilla_one() {
        let root = std::env::temp_dir().join(format!("fern-version-{}", std::process::id()));
        let paths = DataPaths::new(&root);

        write(
            &paths,
            "1.21.1",
            serde_json::json!({
                "id": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [{ "name": "com.mojang:brigadier:1.0.18" }],
                "assetIndex": { "id": "17", "sha1": "aa", "size": 1, "url": "https://x.invalid/a" }
            }),
        );
        write(
            &paths,
            "fabric-loader-0.16.5-1.21.1",
            serde_json::json!({
                "id": "fabric-loader-0.16.5-1.21.1",
                "inheritsFrom": "1.21.1",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries": [{ "name": "net.fabricmc:fabric-loader:0.16.5" }]
            }),
        );

        let merged = resolve(&paths, "fabric-loader-0.16.5-1.21.1").expect("resolve chain");
        // 子的 mainClass 覆盖父的，否则启动的还是原版。
        assert_eq!(
            merged.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        // 父只写在自己那份里的东西要带过来，否则 assets 全都下不了。
        assert_eq!(
            merged.asset_index.map(|index| index.id),
            Some("17".to_owned())
        );
        // 两边的库都要在，而且子的排前面。
        let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "net.fabricmc:fabric-loader:0.16.5",
                "com.mojang:brigadier:1.0.18"
            ]
        );

        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn version_ids_may_contain_dots_but_never_climb_out() {
        // 真实的版本号长这样，全都得放行。
        for id in [
            "1.21.1",
            "1.21-pre1",
            "24w14a",
            "fabric-loader-0.16.5-1.21.1",
            "quilt-loader-0.26.0-1.21.1",
            "1.20.1-forge-47.2.0",
        ] {
            assert!(is_safe_id(id), "{id} 应该是合法的版本 id");
        }
        for id in ["", "..", "../etc", "a/b", "a\\b", ".hidden", "a..b"] {
            assert!(!is_safe_id(id), "{id} 不该被当成版本 id");
        }
    }

    #[test]
    fn an_inherits_from_pointing_outside_is_refused() {
        let root = std::env::temp_dir().join(format!("fern-version-esc-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        write(
            &paths,
            "evil",
            serde_json::json!({ "id": "evil", "inheritsFrom": "../../../../etc/passwd" }),
        );
        // inheritsFrom 是别人 JSON 里的字符串，不能拿去拼路径。
        assert!(resolve(&paths, "evil").is_err());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn a_self_referencing_chain_fails_instead_of_hanging() {
        let root = std::env::temp_dir().join(format!("fern-version-loop-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        write(
            &paths,
            "loop",
            serde_json::json!({ "id": "loop", "inheritsFrom": "loop" }),
        );
        assert!(resolve(&paths, "loop").is_err());

        // 互相指也不能转下去。
        write(
            &paths,
            "ping",
            serde_json::json!({ "id": "ping", "inheritsFrom": "pong" }),
        );
        write(
            &paths,
            "pong",
            serde_json::json!({ "id": "pong", "inheritsFrom": "ping" }),
        );
        assert!(resolve(&paths, "ping").is_err());

        fs::remove_dir_all(root).expect("remove test root");
    }
}
