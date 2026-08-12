//! 一个模组 jar 自报的一切。
//!
//! 三个地方要用同一份东西，所以它单独成一层：
//!
//! - 模组列表要**名字和版本**（`mods.rs`）
//! - 启动前预检查要**依赖、modid、适配的游戏版本**（`launch/preflight.rs`）
//! - 崩溃归因要**包名前缀**，才能把栈帧落到某个模组上（`launch/crash/suspect.rs`）
//!
//! 三家的元数据格式各不相同，但要的东西是同一批：
//!
//! ```text
//! Fabric    fabric.mod.json          id / name / version / depends
//! Quilt     quilt.mod.json           quilt_loader.{id,version,depends}
//! Forge     META-INF/mods.toml       [[mods]] + [[dependencies.<id>]]
//! NeoForge  META-INF/neoforge.mods.toml  同上
//! ```
//!
//! **一个 jar 可以同时是好几家的。** 多加载器打包的模组（数据包型的尤其多）
//! 一个文件里就装着上面这三四份清单，装进哪个实例就由哪个加载器读走它认得的
//! 那一份，其余的没有人看。所以这里读到几份就留几份（[`ModJar::manifests`]）：
//! 只留第一份，会把一个通吃的模组判成「Fabric 的」，还会把它那份 Fabric 清单
//! 里的 `depends: fabric-api` 当成 NeoForge 实例的缺前置——两条都是凭空造出来
//! 的话。
//!
//! **读不懂不是错误。** 任何一步失败都退回「只知道文件名」，因为一个读不出
//! 元数据的 jar 照样能被加载器加载——我们不该比加载器更挑剔。

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::LoaderKind;

/// 一条依赖声明说的是哪一种关系。
///
/// **不是一个布尔量。** 三家都能声明「装了它反而起不来」，而那和「没装它起不来」
/// 正好相反——把它当成一条必需依赖，说出口的就是「缺少 X，去装一个」，而用户照做
/// 之后游戏才真的起不来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    /// 没有它就起不来。
    Required,
    /// 有更好，没有也行。缺了不是问题，不该拿去烦用户。
    Optional,
    /// 有它才起不来。NeoForge 的 `incompatible`、Fabric 的 `breaks`。
    Incompatible,
    /// 装在一起会出问题，但加载器只给个警告。`discouraged` / `conflicts`。
    Discouraged,
}

/// 一条依赖声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub mod_id: String,
    /// 版本区间，原样保留。看不懂的写法由 `launch::ranges` 兜底。
    pub range: String,
    pub relation: Relation,
}

/// 一个 jar 里为某一家加载器写的那份清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub loader: LoaderKind,
    /// **这一份**清单声明的依赖。同一个 jar 的另一份可以完全不同。
    pub depends: Vec<Dependency>,
}

impl Manifest {
    /// 这一份清单里必需的那些依赖。
    pub fn required(&self) -> impl Iterator<Item = &Dependency> {
        self.depends
            .iter()
            .filter(|dependency| dependency.relation == Relation::Required)
    }

    /// 它声明支持的 Minecraft 版本区间。没声明就是没有。
    pub fn minecraft_range(&self) -> Option<&str> {
        self.required()
            .find(|dependency| dependency.mod_id == "minecraft")
            .map(|dependency| dependency.range.as_str())
    }
}

/// 从一个 jar 里读到的东西。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModJar {
    pub file_name: String,
    /// 关掉的模组文件名带 `.disabled`，加载器不会读它。
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    /// 展示名。读不到元数据时是文件名。
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 这个 jar 里读到的所有清单，按 Fabric、Quilt、NeoForge、Forge 的顺序。
    /// 一份都读不到就是空——那是「不知道」，不是「原版」。
    #[serde(default)]
    pub manifests: Vec<Manifest>,
    /// 除自己之外，这个 jar 还让哪些 id 算「装了」：元数据里的 `provides`，
    /// 加上打包在它里面的那些 jar（jar-in-jar，见 `nested`）。
    #[serde(default)]
    pub provides: Vec<String>,
    /// 顶层包，例如 `net.caffeinemc.mods.sodium`。崩溃归因按它匹配栈帧。
    pub packages: Vec<String>,
}

impl ModJar {
    /// 这个 jar 是写给哪几家的。读不出清单就是空——那是「不知道」。
    pub fn loaders(&self) -> Vec<LoaderKind> {
        self.manifests
            .iter()
            .map(|manifest| manifest.loader)
            .collect()
    }

    /// 所有清单里必需的那些依赖，不分加载器。
    ///
    /// 给挑 Java 那条路用：`depends: { "java": ">=25" }` 三份清单写的是同一件
    /// 事，而那条路在实例的加载器之外还有别的调用点，宁可多看一眼。**要问的是
    /// 「装在哪个实例里都成立的话」时才用它**，别的地方一律只看生效的那一份。
    pub fn every_requirement(&self) -> impl Iterator<Item = &Dependency> {
        self.manifests.iter().flat_map(Manifest::required)
    }

    /// 这个 jar 让某个 id 算「装了」没有。自己的 id 也算。
    pub fn supplies(&self, mod_id: &str) -> bool {
        self.mod_id.as_deref() == Some(mod_id) || self.provides.iter().any(|id| id == mod_id)
    }
}

/// 读一个 jar。读不动就只留文件名。
pub fn read(path: &Path) -> ModJar {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let enabled = file_name.ends_with(".jar");
    let mut jar = ModJar {
        name: display_name(&file_name),
        file_name,
        enabled,
        mod_id: None,
        version: None,
        manifests: Vec::new(),
        provides: Vec::new(),
        packages: Vec::new(),
    };

    let Ok(file) = std::fs::File::open(path) else {
        return jar;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return jar;
    };

    merge(&mut jar, describe(&mut archive));
    let nested = nested(&mut archive, 0);
    jar.provides.extend(nested.ids);
    jar.provides.sort();
    jar.provides.dedup();
    jar.provides
        .retain(|id| Some(id.as_str()) != jar.mod_id.as_deref());

    jar.packages = packages(&archive);
    // 自己一行代码都没有的 jar，里面那些模块的包就算它的：Fabric API 是个空
    // 壳，崩在 `net.fabricmc.fabric.impl.…` 时，没有这一条就没有任何模组认领
    // 那一帧。只在空壳上这么算——顺手打包了一个库、自己也有代码的模组，把库
    // 的包算成它的会把归因指错人。
    if jar.packages.is_empty() {
        jar.packages = nested.packages;
    }
    jar
}

/// 一个 jar 自报的那几段元数据，各自属于哪个加载器。
///
/// **读到几份就返回几份。** 一个 jar 里同时躺着 `fabric.mod.json` 和
/// `META-INF/neoforge.mods.toml` 是常事，两份都是真的，只是每次只有一份会被
/// 读走。碰到第一份就收工，等于替加载器做了一个它不会做的选择。
///
/// 单独一层，是因为**打包在里面的那些 jar 要走同一条路**：一个嵌套模块的
/// `fabric.mod.json` 和外层那份长得一模一样。
fn describe<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Vec<(Described, LoaderKind)> {
    let mut found = Vec::new();
    if let Some(described) = entry(archive, "fabric.mod.json").and_then(|t| fabric(&t)) {
        found.push((described, LoaderKind::Fabric));
    }
    if let Some(described) = entry(archive, "quilt.mod.json").and_then(|t| quilt(&t)) {
        found.push((described, LoaderKind::Quilt));
    }
    for (name, loader) in [
        ("META-INF/neoforge.mods.toml", LoaderKind::NeoForge),
        ("META-INF/mods.toml", LoaderKind::Forge),
    ] {
        let Some(text) = entry(archive, name) else {
            continue;
        };
        let Some(mut described) = forge(&text) else {
            continue;
        };
        // Forge 常把版本写成 `${file.jarVersion}`，真值在 MANIFEST 里。
        if described
            .version
            .as_deref()
            .is_some_and(|version| version.contains("${"))
        {
            described.version = entry(archive, "META-INF/MANIFEST.MF")
                .and_then(|manifest| manifest_value(&manifest, "Implementation-Version"));
        }
        found.push((described, loader));
    }
    found
}

/// 打包在一个 jar 里面的那些 jar（jar-in-jar）自报了什么。
///
/// Fabric API 几乎是个空壳：`fabric-block-getter-api-v2`、`fabric-rendering-v1`
/// 这四十来个模块各是一个独立的 jar，躺在 `META-INF/jars/` 下面，由加载器在
/// 运行时一并装载。模组写进 depends 的正是这些模块 id，而不是 `fabric-api`。
/// 只看外层那一个 id，预检查就会对着一个已经装好 Fabric API 的实例报出一串
/// 「缺前置」——用户照着去装，还根本找不到那些名字；崩溃归因那边同样认不出
/// `net.fabricmc.fabric.impl.…` 是谁的代码。
///
/// Forge/NeoForge 的 JarJar 是同一回事，只是目录叫 `META-INF/jarjar/`。
#[derive(Default)]
struct Nested {
    ids: Vec<String>,
    packages: Vec<String>,
}

fn nested<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    depth: usize,
) -> Nested {
    // 模块里再套模块是有的（Fabric API 的子模块又带着自己的依赖），但两层已经
    // 够；再深就只是在给一个构造出来的 zip 让我们空转的机会。
    const MAX_DEPTH: usize = 2;
    if depth > MAX_DEPTH {
        return Nested::default();
    }

    let names: Vec<String> = archive
        .file_names()
        .filter(|name| {
            name.ends_with(".jar")
                && ["META-INF/jars/", "META-INF/jarjar/"]
                    .iter()
                    .any(|directory| name.starts_with(directory))
        })
        .map(str::to_owned)
        .collect();

    let mut found = Nested::default();
    for name in names {
        let Some(bytes) = raw_entry(archive, &name) else {
            continue;
        };
        let Ok(mut module) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
            continue;
        };
        for (described, _) in describe(&mut module) {
            found.ids.extend(described.mod_id);
            found.ids.extend(described.provides);
        }
        found.packages.extend(packages(&module));
        let deeper = nested(&mut module, depth + 1);
        found.ids.extend(deeper.ids);
        found.packages.extend(deeper.packages);
    }
    // 四十个模块给出四十条 `net.fabricmc.fabric.impl.…`，留一条最短的就够——
    // 归因是按前缀匹配的。
    found.packages.sort();
    found.packages.dedup();
    found.packages = shortest_prefixes(&found.packages);
    found
}

/// 一组包名里，去掉那些已经被更短的一条覆盖住的。
fn shortest_prefixes(packages: &[String]) -> Vec<String> {
    packages
        .iter()
        .filter(|package| {
            !packages.iter().any(|other| {
                other.len() < package.len() && package.starts_with(&format!("{other}."))
            })
        })
        .cloned()
        .collect()
}

/// 把一个条目原样读进内存。
///
/// 嵌套 jar 只能这样读——`ZipArchive` 要 `Seek`，而条目本身是流。一个模块 jar
/// 几十 KB，读进来不心疼；上限挡的是声称自己解出来有几个 G 的那种压缩包。
fn raw_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let entry = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.take(LIMIT).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// 一个目录里的所有 jar，禁用的也算——「它被关掉了」本身就是预检查要说的话。
pub fn read_all(directory: &Path) -> Vec<ModJar> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut jars: Vec<ModJar> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.ends_with(".jar") || name.ends_with(".jar.disabled")
        })
        .map(|entry| read(&entry.path()))
        .collect();
    jars.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    jars
}

/// 这个实例的 mods 目录在哪。
pub fn directory(paths: &crate::DataPaths, instance_id: &str) -> PathBuf {
    paths.game_directory(instance_id).join("mods")
}

#[derive(Default)]
struct Described {
    mod_id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    /// 元数据里的 `provides`：这个 jar 声明自己顶替哪些 id。
    provides: Vec<String>,
    depends: Vec<Dependency>,
}

/// 把读到的几份清单并进一个 `ModJar`。
///
/// **名字、版本、modid 取第一份，依赖分开留着。** 前三样三家写的是同一件事——
/// 多加载器打包的模组就是同一个模组的几副面孔，谁先谁后都一样；而依赖不是，
/// 混在一起就等于在每个实例里都报出别家清单里的那些前置。
fn merge(jar: &mut ModJar, manifests: Vec<(Described, LoaderKind)>) {
    for (index, (described, loader)) in manifests.into_iter().enumerate() {
        if index == 0 {
            if let Some(name) = described.name {
                jar.name = name;
            }
            jar.mod_id = described.mod_id;
            jar.version = described.version;
        }
        // `provides` 反过来，几份并起来：多算一个 id 只会少报一条缺前置，
        // 而漏算一个会凭空多报一条。
        jar.provides.extend(described.provides);
        jar.manifests.push(Manifest {
            loader,
            depends: described.depends,
        });
    }
}

/// `provides` 两种写法都有：一串 id，或者一串带 `id` 的对象。
fn provided_ids(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    serde_json::Value::String(id) => Some(id.clone()),
                    _ => string_at(item, "id"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 一份 `.mod.json`，按加载器的宽容度读。
///
/// **不能只读严格 JSON。** 加载器读这些文件用的是 gson，而 gson 从不检查字符串
/// 字面量里的控制字符——`serde_json` 会。真的有模组因此在我们眼里凭空消失：
/// ObsidianUI 的 `fabric.mod.json` 里 `description` 横跨两行，中间是两个没转义
/// 的换行。它被 MidnightControls 打包在 `META-INF/jars/` 里，游戏照常启动，而
/// 预检查读不出那份清单，于是对着一个装好了的前置报「缺少 obsidianui」。
///
/// 只在严格那一遍失败之后才走这条路：绝大多数清单是规规矩矩的 JSON，不必为了
/// 一个别人家的手误让每一个 jar 都多走一趟。
///
/// `mods.rs` 为了列表另读一遍同样这几份文件，走的也是这里——不然同一个 jar
/// 在预检查里认得出、在模组列表里只剩个文件名。
pub(super) fn json(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&escape_control_characters(text)).ok())
}

/// 把字符串字面量里那些没转义的控制字符转义掉，字面量之外一个字符都不动。
fn escape_control_characters(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    let mut escaped = false;
    for character in text.chars() {
        if escaped {
            // 转义序列的第二个字符，原样放回去——它不是引号，也不是反斜杠。
            out.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' if inside => {
                escaped = true;
                out.push(character);
            }
            '"' => {
                inside = !inside;
                out.push(character);
            }
            control if inside && (control as u32) < 0x20 => match control {
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other => out.push_str(&format!("\\u{:04x}", other as u32)),
            },
            other => out.push(other),
        }
    }
    out
}

/// Fabric 的四张表，各是一种关系。
///
/// `breaks` 和 `conflicts` 说的是「装在一起会坏」，方向和 `depends` 相反。
/// `suggests` 只是一句推荐，加载器完全不管，不读。
fn fabric(text: &str) -> Option<Described> {
    let value = json(text)?;
    let mut depends = Vec::new();
    for (key, relation) in [
        ("depends", Relation::Required),
        ("recommends", Relation::Optional),
        ("breaks", Relation::Incompatible),
        ("conflicts", Relation::Discouraged),
    ] {
        let Some(table) = value.get(key).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (mod_id, range) in table {
            depends.push(Dependency {
                mod_id: mod_id.clone(),
                range: range_text(range),
                relation,
            });
        }
    }
    Some(Described {
        mod_id: string_at(&value, "id"),
        name: string_at(&value, "name").or_else(|| string_at(&value, "id")),
        version: string_at(&value, "version"),
        provides: provided_ids(value.get("provides")),
        depends,
    })
}

/// Quilt 的 `depends` 和 `breaks` 是两个数组，写法一样，方向相反。
fn quilt(text: &str) -> Option<Described> {
    let value = json(text)?;
    let loader = value.get("quilt_loader")?;
    let mut depends = Vec::new();
    for (key, present, absent) in [
        ("depends", Relation::Optional, Relation::Required),
        // `breaks` 里的 `optional: true` 是「不建议装在一起」，不是「不能」。
        ("breaks", Relation::Discouraged, Relation::Incompatible),
    ] {
        let Some(list) = loader.get(key).and_then(serde_json::Value::as_array) else {
            continue;
        };
        depends.extend(list.iter().filter_map(|item| {
            match item {
                serde_json::Value::String(id) => Some(Dependency {
                    mod_id: id.clone(),
                    range: "*".to_owned(),
                    relation: absent,
                }),
                serde_json::Value::Object(_) => Some(Dependency {
                    mod_id: string_at(item, "id")?,
                    range: item
                        .get("versions")
                        .map(range_text)
                        .unwrap_or_else(|| "*".to_owned()),
                    relation: match item.get("optional").and_then(serde_json::Value::as_bool) {
                        Some(true) => present,
                        _ => absent,
                    },
                }),
                _ => None,
            }
        }));
    }
    Some(Described {
        mod_id: string_at(loader, "id"),
        name: loader
            .get("metadata")
            .and_then(|metadata| string_at(metadata, "name"))
            .or_else(|| string_at(loader, "id")),
        version: string_at(loader, "version"),
        provides: provided_ids(loader.get("provides")),
        depends,
    })
}

/// `mods.toml`：`[[mods]]` 的第一段，加上 `[[dependencies.<modid>]]`。
fn forge(text: &str) -> Option<Described> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        mods: Vec<Entry>,
        #[serde(default)]
        dependencies: HashMap<String, Vec<Requirement>>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        #[serde(default)]
        mod_id: Option<String>,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        version: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Requirement {
        #[serde(default)]
        mod_id: Option<String>,
        #[serde(default)]
        version_range: Option<String>,
        /// 1.20 之前用布尔，之后用 `type = "required"`。两种都认。
        #[serde(default)]
        mandatory: Option<bool>,
        #[serde(default, rename = "type")]
        kind: Option<String>,
    }

    /// `type` 的四个取值，大小写不敏感。
    ///
    /// **`incompatible` 不是一种依赖。** 它说的是「装了它我就起不来」，AllTheLeaks
    /// 那样的修补型模组会一口气列出十来个。把它当成必需依赖，预检查报出来的是
    /// 「缺少 ambientsounds，去装一个」——一句和事实正好相反的话。
    ///
    /// 两个字段都没写就是必需：`type` 的默认值是 `required`。
    fn relation(requirement: &Requirement) -> Relation {
        match requirement
            .kind
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("optional") => Relation::Optional,
            Some("incompatible") => Relation::Incompatible,
            Some("discouraged") => Relation::Discouraged,
            // 老写法只有必需与否两种，落到这里的 `type` 我们不认识，同样按它论。
            _ => match requirement.mandatory {
                Some(false) => Relation::Optional,
                _ => Relation::Required,
            },
        }
    }

    let file: File = toml::from_str(text).ok()?;
    let first = file.mods.first();
    let mod_id = first.and_then(|entry| entry.mod_id.clone());
    // 依赖表按 modid 分组，只要这个 jar 自己那一组。
    let depends = mod_id
        .as_ref()
        .and_then(|id| file.dependencies.get(id))
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            Some(Dependency {
                mod_id: requirement.mod_id.clone()?,
                range: requirement.version_range.clone().unwrap_or_default(),
                relation: relation(requirement),
            })
        })
        .collect();
    Some(Described {
        name: first.and_then(|entry| entry.display_name.clone()),
        version: first.and_then(|entry| entry.version.clone()),
        mod_id,
        // mods.toml 没有 provides 这一说；一个 jar 里的其余 `[[mods]]` 段
        // 同样是它提供的 id。
        provides: file
            .mods
            .iter()
            .skip(1)
            .filter_map(|entry| entry.mod_id.clone())
            .collect(),
        depends,
    })
}

/// jar 里的顶层包。
///
/// 不取全体类的最长公共前缀：很多模组把依赖也打了进来（shade），那时候公共前缀
/// 会退化成空。所以按前两段分组，只留占比够大的那些组，再在组内取公共前缀——
/// 「被打进来的三方库」自然被丢掉，而模组自己的包留了下来。
fn packages<R: std::io::Read + std::io::Seek>(archive: &zip::ZipArchive<R>) -> Vec<String> {
    const IGNORED: [&str; 4] = ["META-INF/", "assets/", "data/", "mappings/"];

    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut total = 0usize;
    for name in archive.file_names() {
        if !name.ends_with(".class") || IGNORED.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let Some((directory, _)) = name.rsplit_once('/') else {
            continue;
        };
        let segments: Vec<&str> = directory.split('/').collect();
        if segments.len() < 2 {
            continue;
        }
        total += 1;
        groups
            .entry(segments[..2].join("."))
            .or_default()
            .push(directory.replace('/', "."));
    }
    if total == 0 {
        return Vec::new();
    }

    let mut found: Vec<String> = groups
        .into_values()
        // 占比太小的多半是被打进来的三方库，不是这个模组自己的代码。
        .filter(|members| members.len() * 5 >= total)
        .filter_map(|members| common_prefix(&members))
        .collect();
    found.sort();
    found
}

/// 一组包名的公共前缀，按段算。
fn common_prefix(packages: &[String]) -> Option<String> {
    let mut prefix: Vec<&str> = packages.first()?.split('.').collect();
    for package in packages.iter().skip(1) {
        let segments: Vec<&str> = package.split('.').collect();
        let shared = prefix
            .iter()
            .zip(segments.iter())
            .take_while(|(left, right)| left == right)
            .count();
        prefix.truncate(shared);
    }
    (prefix.len() >= 2).then(|| prefix.join("."))
}

/// 一条依赖写的版本区间。字符串一种写法，数组一种写法。
///
/// **数组是「或」，一个字符串里的空格是「与」。** 这是 fabric.mod.json 明写的
/// 规矩，而两者一旦拉平就正好拧反：LambDynamicLights 写的是
///
/// ```json
/// "minecraft": ["~1.21.5- <1.21.6-", "=1.21.6-alpha.25.14.craftmine"]
/// ```
///
/// 第一段说「1.21.5 系列」，第二段单独把那个愚人节快照加回来。用空格拼成一条，
/// 就成了「既要在 1.21.6 以下、又要正好是这个快照」——没有任何版本满足得了，
/// 于是一个明明写着支持它的模组被报成不适配。所以拼成 `||`，由
/// [`ranges::contains`](crate::launch::ranges::contains) 认这个分隔符。
fn range_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(" || "),
        _ => "*".to_owned(),
    }
}

fn entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
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

/// 元数据读不出来时的退路：去掉后缀，够认。
fn display_name(file_name: &str) -> String {
    file_name
        .trim_end_matches(".disabled")
        .trim_end_matches(".jar")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(directory: &Path, file_name: &str, entries: &[(&str, &str)]) -> PathBuf {
        std::fs::create_dir_all(directory).expect("create directory");
        let path = directory.join(file_name);
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create jar"));
        for (name, content) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(content.as_bytes()).expect("write entry");
        }
        writer.finish().expect("finish jar");
        path
    }

    fn manifest(jar: &ModJar, loader: LoaderKind) -> &Manifest {
        jar.manifests
            .iter()
            .find(|manifest| manifest.loader == loader)
            .unwrap_or_else(|| panic!("{loader:?} 那份清单"))
    }

    fn relation_to(manifest: &Manifest, mod_id: &str) -> Option<Relation> {
        manifest
            .depends
            .iter()
            .find(|dependency| dependency.mod_id == mod_id)
            .map(|dependency| dependency.relation)
    }

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fern-jar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn reads_fabric_metadata_and_its_dependencies() {
        let root = temporary("fabric");
        let path = jar(
            &root,
            "sodium.jar",
            &[(
                "fabric.mod.json",
                r#"{"id":"sodium","name":"Sodium","version":"0.6.0",
                    "depends":{"fabric-api":">=0.100.0","minecraft":"~1.21"},
                    "recommends":{"iris":"*"}}"#,
            )],
        );
        let read = read(&path);
        assert_eq!(read.mod_id.as_deref(), Some("sodium"));
        assert_eq!(read.name, "Sodium");
        assert_eq!(read.loaders(), vec![LoaderKind::Fabric]);
        let fabric = manifest(&read, LoaderKind::Fabric);
        assert_eq!(fabric.minecraft_range(), Some("~1.21"));
        assert_eq!(relation_to(fabric, "fabric-api"), Some(Relation::Required));
        // 建议装的不是必需的，缺了不该拿去烦用户。
        assert_eq!(relation_to(fabric, "iris"), Some(Relation::Optional));
        std::fs::remove_dir_all(root).ok();
    }

    /// `type` 的四个取值各是一种关系，`incompatible` 尤其不能当成依赖。
    ///
    /// 照着 AllTheLeaks 的 `neoforge.mods.toml` 写：那个修补型模组一口气列了
    /// 十来条 `type = "incompatible"`，说的是「这些模组的老版本有内存泄漏，
    /// 和我装在一起会起不来」。把它读成必需依赖，预检查说出口的就是「缺少
    /// ambientsounds，去装一个」——一句和事实正好相反、而且用户照做之后更糟的话。
    #[test]
    fn the_four_kinds_of_forge_dependency_are_four_different_things() {
        let root = temporary("relations");
        let path = jar(
            &root,
            "alltheleaks.jar",
            &[(
                "META-INF/neoforge.mods.toml",
                r#"
modLoader = "javafml"
[[mods]]
modId = "alltheleaks"
displayName = "All The Leaks"
version = "1.1.7"
[[dependencies.alltheleaks]]
    modId = "neoforge"
    type = "required"
    versionRange = "[21.1.195,)"
[[dependencies.alltheleaks]]
    modId = "jei"
    type = "optional"
[[dependencies.alltheleaks]]
    modId = "ambientsounds"
    type = "incompatible"
    versionRange = "(,6.1.0]"
[[dependencies.alltheleaks]]
    modId = "occultism"
    type = "DISCOURAGED"
"#,
            )],
        );
        let read = read(&path);
        let neoforge = manifest(&read, LoaderKind::NeoForge);
        assert_eq!(relation_to(neoforge, "neoforge"), Some(Relation::Required));
        assert_eq!(relation_to(neoforge, "jei"), Some(Relation::Optional));
        assert_eq!(
            relation_to(neoforge, "ambientsounds"),
            Some(Relation::Incompatible)
        );
        // 取值大小写不敏感，NeoForge 自己就是这么读的。
        assert_eq!(
            relation_to(neoforge, "occultism"),
            Some(Relation::Discouraged)
        );
        // 必需的只有那一条，别的三条都不该出现在「缺什么」里。
        let required: Vec<&str> = neoforge
            .required()
            .map(|dependency| dependency.mod_id.as_str())
            .collect();
        assert_eq!(required, vec!["neoforge"]);

        std::fs::remove_dir_all(root).ok();
    }

    /// `[[dependencies.x]]` 是嵌套表，正是当初手写行解析扛不住的地方。
    #[test]
    fn reads_forge_dependencies_from_nested_tables() {
        let root = temporary("forge");
        let path = jar(
            &root,
            "ae2.jar",
            &[(
                "META-INF/mods.toml",
                r#"
modLoader = "javafml"
[[mods]]
modId = "appliedenergistics2"
displayName = "Applied Energistics 2"
version = "19.0.0"
[[dependencies.appliedenergistics2]]
    modId = "jei"
    mandatory = true
    versionRange = "[19.5.0,)"
[[dependencies.appliedenergistics2]]
    modId = "curios"
    type = "optional"
    versionRange = "[9.0,)"
"#,
            )],
        );
        let read = read(&path);
        assert_eq!(read.mod_id.as_deref(), Some("appliedenergistics2"));
        assert_eq!(read.loaders(), vec![LoaderKind::Forge]);
        let depends = &manifest(&read, LoaderKind::Forge).depends;
        assert_eq!(depends.len(), 2);
        assert_eq!(depends[0].relation, Relation::Required);
        assert_eq!(depends[0].range, "[19.5.0,)");
        assert_eq!(depends[1].relation, Relation::Optional);
        std::fs::remove_dir_all(root).ok();
    }

    /// 一个文件通吃三家的模组，三份清单都要读到，而且各是各的依赖。
    ///
    /// 照着 Explorify v1.6.5 的真实内容写：它一个 jar 里躺着
    /// `fabric.mod.json`、`META-INF/mods.toml`、`META-INF/neoforge.mods.toml`，
    /// 而那份 Fabric 清单要 `fabric-api`，NeoForge 那份只要 `minecraft`。碰到
    /// 第一份就收工的读法，会把它判成「Fabric 的模组」，再在 NeoForge 实例里
    /// 追加一条「缺 Fabric API」——两条都是关于一份没有人读的文件。
    #[test]
    fn a_jar_written_for_three_loaders_keeps_all_three_manifests() {
        let root = temporary("multiloader");
        let toml = r#"
modLoader = "lowcodefml"
[[mods]]
modId = "explorify"
displayName = "Explorify"
version = "1.6.5"
[[dependencies.explorify]]
    modId = "minecraft"
    mandatory = true
    versionRange = "[1.20,)"
"#;
        let path = jar(
            &root,
            "Explorify v1.6.5.mod.jar",
            &[
                (
                    "fabric.mod.json",
                    r#"{"id":"explorify","name":"Explorify","version":"1.6.5",
                        "depends":{"fabric-api":"*","minecraft":">=1.20"}}"#,
                ),
                ("META-INF/mods.toml", toml),
                ("META-INF/neoforge.mods.toml", toml),
            ],
        );

        let read = read(&path);
        assert_eq!(
            read.loaders(),
            vec![LoaderKind::Fabric, LoaderKind::NeoForge, LoaderKind::Forge]
        );
        // 装进哪个实例，就只有那一家的依赖算数。
        let ids = |loader| {
            manifest(&read, loader)
                .required()
                .map(|dependency| dependency.mod_id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(LoaderKind::NeoForge), vec!["minecraft"]);
        assert_eq!(ids(LoaderKind::Forge), vec!["minecraft"]);
        assert!(ids(LoaderKind::Fabric).contains(&"fabric-api"));
        // 版本区间同样跟着清单走：Fabric 那份写的是另一种写法。
        assert_eq!(
            manifest(&read, LoaderKind::NeoForge).minecraft_range(),
            Some("[1.20,)")
        );
        assert_eq!(
            manifest(&read, LoaderKind::Fabric).minecraft_range(),
            Some(">=1.20")
        );
        // 名字、版本、modid 三家写的是同一件事，取哪一份都一样。
        assert_eq!(read.name, "Explorify");
        assert_eq!(read.version.as_deref(), Some("1.6.5"));

        std::fs::remove_dir_all(root).ok();
    }

    /// Fabric API 的模块都在它自己的 `META-INF/jars/` 里，模组 depends 写的
    /// 是那些模块 id。
    #[test]
    fn the_jars_packed_inside_a_jar_count_as_provided() {
        let root = temporary("jarinjar");
        std::fs::create_dir_all(&root).expect("create root");

        // 先造一个模块 jar，再把它整个塞进外层 jar 的 META-INF/jars/。
        let mut entries: Vec<(String, &str)> = (0..8)
            .map(|index| {
                (
                    format!("net/fabricmc/fabric/impl/blockview/Class{index}.class"),
                    "",
                )
            })
            .collect();
        entries.push((
            "fabric.mod.json".to_owned(),
            r#"{"id":"fabric-block-getter-api-v2","version":"1.0.0"}"#,
        ));
        let borrowed: Vec<(&str, &str)> = entries
            .iter()
            .map(|(name, content)| (name.as_str(), *content))
            .collect();
        let module = jar(&root, "module.jar", &borrowed);
        let module_bytes = std::fs::read(&module).expect("read module");

        let path = root.join("fabric-api.jar");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create jar"));
        writer
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer
            .write_all(
                br#"{"id":"fabric-api","name":"Fabric API","version":"0.100.0",
                     "provides":["fabricapi"]}"#,
            )
            .expect("write entry");
        writer
            .start_file(
                "META-INF/jars/fabric-block-getter-api-v2.jar",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start nested");
        writer.write_all(&module_bytes).expect("write nested");
        writer.finish().expect("finish jar");

        let read = read(&path);
        assert_eq!(read.mod_id.as_deref(), Some("fabric-api"));
        // 声明的别名和打包进来的模块，都算这个 jar 提供的。
        assert_eq!(
            read.provides,
            vec!["fabric-block-getter-api-v2", "fabricapi"]
        );
        assert!(read.supplies("fabric-api"));
        assert!(read.supplies("fabric-block-getter-api-v2"));
        assert!(!read.supplies("sodium"));
        // 外层一行代码都没有，崩在模块里的那一帧只能靠这些包认领。
        assert_eq!(read.packages, vec!["net.fabricmc.fabric.impl.blockview"]);

        std::fs::remove_dir_all(root).ok();
    }

    /// 清单里有个没转义的换行，加载器照读，我们也得照读。
    ///
    /// 照着 ObsidianUI 0.2.11 的 `fabric.mod.json` 写：它的 `description` 横跨
    /// 两行，中间是两个真的换行符——gson 不管，`serde_json` 直接判错。那一份躺在
    /// MidnightControls 的 `META-INF/jars/` 里，于是一个装好了就能玩的实例，被
    /// 报出一条「MidnightControls 缺少 obsidianui」。
    #[test]
    fn a_manifest_with_a_raw_newline_in_it_is_still_read() {
        let root = temporary("lenient");
        let module = jar(
            &root,
            "obsidianui.jar",
            &[(
                "fabric.mod.json",
                "{\n  \"id\": \"obsidianui\",\n  \"version\": \"0.2.11+mc1.21.5\",\n  \
                 \"name\": \"ObsidianUI\",\n  \
                 \"description\": \"SpruceUI unofficial architectury port.\n\nJust a GUI library.\",\n  \
                 \"depends\": {\"minecraft\": \">=1.21.5\"}\n}",
            )],
        );
        let module_bytes = std::fs::read(&module).expect("read module");

        let path = root.join("midnightcontrols.jar");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create jar"));
        writer
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer
            .write_all(
                br#"{"id":"midnightcontrols","name":"MidnightControls","version":"1.11.0",
                     "depends":{"obsidianui":">=0.2.5"}}"#,
            )
            .expect("write entry");
        writer
            .start_file(
                "META-INF/jars/obsidianui-0.2.11+mc1.21.5-fabric.jar",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start nested");
        writer.write_all(&module_bytes).expect("write nested");
        writer.finish().expect("finish jar");

        let read = read(&path);
        assert!(read.supplies("obsidianui"), "{:?}", read.provides);

        // 字面量之外的换行本来就该在，转义只发生在引号里面。
        let value =
            json("{\n  \"a\": \"one\ntwo\",\n  \"b\": \"back\\\\slash\"\n}").expect("宽容那一遍");
        assert_eq!(value["a"], serde_json::json!("one\ntwo"));
        assert_eq!(value["b"], serde_json::json!("back\\slash"));

        std::fs::remove_dir_all(root).ok();
    }

    /// 被打进来的三方库不该被当成这个模组的包。
    #[test]
    fn packages_survive_a_shaded_dependency() {
        let root = temporary("packages");
        let mut entries: Vec<(String, &str)> = (0..20)
            .map(|index| {
                (
                    format!("net/caffeinemc/mods/sodium/client/Class{index}.class"),
                    "",
                )
            })
            .collect();
        entries.push(("org/joml/Vector3f.class".to_owned(), ""));
        entries.push(("fabric.mod.json".to_owned(), r#"{"id":"sodium"}"#));
        let borrowed: Vec<(&str, &str)> = entries
            .iter()
            .map(|(name, content)| (name.as_str(), *content))
            .collect();
        let path = jar(&root, "sodium.jar", &borrowed);

        let read = read(&path);
        assert_eq!(read.packages, vec!["net.caffeinemc.mods.sodium.client"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unreadable_jar_still_yields_its_file_name() {
        let root = temporary("broken");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("mystery.jar");
        std::fs::write(&path, b"not a zip").expect("write");
        let read = read(&path);
        assert_eq!(read.name, "mystery");
        assert!(read.enabled);
        assert!(read.mod_id.is_none());
        std::fs::remove_dir_all(root).ok();
    }
}
