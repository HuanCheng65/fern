use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use fern_download::DownloadClient;
use fern_meta::{VersionManifest, VersionManifestEntry};
use serde::{Deserialize, Serialize};

use crate::{
    DataPaths, InstanceId, InstanceProfile,
    data::{
        metacache::{self, Freshness},
        settings::source_order,
    },
    launch::prepare::MANIFEST_SLUG,
};

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
        // 旧实例的形状是「一个加载器」，读进来先摊成层表（见
        // `InstanceProfile::migrate`）。只在读这一侧做，写出去的永远是新形状。
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse {}", config.display()))?;
        InstanceProfile::migrate(&mut raw);
        profiles.push(
            serde_json::from_value(raw).with_context(|| format!("parse {}", config.display()))?,
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
    create_instance_with_loader(paths, name, game_version, crate::LoaderKind::Vanilla, None)
}

/// 建实例，可以带一个加载器。
///
/// 这里只把选择记下来，不下载任何东西：加载器的 profile 在补全阶段装，和
/// 游戏文件走同一条进度。建实例这一步应该是瞬间完成的。
pub fn create_instance_with_loader(
    paths: &DataPaths,
    name: &str,
    game_version: &str,
    loader: crate::LoaderKind,
    loader_version: Option<&str>,
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
    let id = allocate_id(paths)?;
    let mut profile = InstanceProfile::vanilla(InstanceId::parse(&id)?, name, game_version);
    if loader != crate::LoaderKind::Vanilla {
        // 拦在这里而不是补全时：建一个永远补全不了的实例，比直接说不行更糟——
        // 用户会以为成了，直到点启动才发现。
        if !crate::installable_loaders()
            .iter()
            .any(|option| option.kind == loader)
        {
            return Err(anyhow!(
                "{} 的安装尚未实现",
                crate::loader_display_name(loader)
            ));
        }
        let version = loader_version
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("选择 {loader:?} 时必须指定加载器版本"))?;
        // version_id 留空：装完之后才知道上游给的是哪个 id，那时候再补。
        profile.components.push(crate::Component {
            kind: loader,
            version,
            version_id: String::new(),
            jar_mods: Vec::new(),
        });
        profile = profile.normalized();
    }
    let instance_root = paths.instance_root(&id);
    fs::create_dir_all(instance_root.join(".minecraft")).context("create game directory")?;
    let bytes = serde_json::to_vec_pretty(&profile).context("serialize instance profile")?;
    fs::write(paths.instance_config(&id), bytes).context("write instance profile")?;
    Ok(profile)
}

/// 一个实例在这台机器上会得到什么。
///
/// 实例设置那一屏要能回答「不改的话会怎样」——「自动」这两个字本身不解释
/// 任何事情，只有把自动算出来的结果摆出来，用户才知道要不要动它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRuntime {
    /// 不手动指定时会分配的堆，单位 MB。
    pub automatic_memory_mb: u32,
    /// 现在按下启动会得到的那份分配，连同它的理由。
    ///
    /// 和真正启动时算的是同一个函数——界面显示一份、启动用另一份，就会出现
    /// 「设置里写着 8 G，实际跑的是 4 G」这种没人查得动的问题。
    pub allocation: crate::AllocationDecision,
    /// 真跑出来的那几个数。历史不够就是 `None`，界面那时不画实测刻度。
    pub measured: Option<crate::MemoryHistory>,
    pub physical_memory_mb: u32,
    /// 这个版本能接受的 Java 区间。
    pub requirement: crate::JavaRequirement,
    /// 现在会选中的那一个。为空说明得先下一个。
    pub java: Option<crate::JavaRuntime>,
    pub mods_count: u32,
    /// 这个实例最终生效的那份设置里，界面要显示「跟随全局」时该说的那些值。
    ///
    /// 实例设置面板必须说得出「不改的话会怎样」——「跟随全局」四个字本身不
    /// 解释任何事，把全局解析出来的结果摆在旁边才行。
    pub defaults: crate::GameDefaults,
    /// 交给游戏的内存上限，MB。滑杆的右端就是它。
    pub memory_ceiling_mb: u32,
}

/// 不联网就能算出来的那部分。版本要求取自已经落盘的元数据，没补全过的实例
/// 拿不到——那时候按版本号推，够用来填一个默认值。
pub fn instance_runtime(paths: &DataPaths, instance_id: &str) -> Result<InstanceRuntime> {
    let profile = read_instance(paths, instance_id)?;
    let declared = read_prepared_metadata(paths, &profile.game_version)
        .and_then(|metadata| metadata.java_version.map(|version| version.major_version));
    let game_directory = crate::instance::paths_for(paths, &profile).game_directory(instance_id);
    // 「自动会挑哪个 Java」要和真正启动时挑的是同一个，模组要求的那条下界也
    // 在其中——否则这一屏写着 21、启动用的是 25。
    let requirement = crate::java_requirement(&profile.game_version, profile.loader, declared)
        .preferring(crate::launch::preflight::java_floor(
            &crate::instance::jar::read_all(&game_directory.join("mods")),
        ));
    let mods = crate::mods_profile(&game_directory);
    let physical = crate::physical_memory_bytes();
    let defaults = crate::current_settings().game;
    let ceiling = crate::heap_ceiling(physical, defaults.memory_ceiling_mb);
    let effective = crate::effective_settings(&profile.settings, &defaults, physical);
    let java = crate::select_java(&crate::discover_java(Some(paths)), &requirement);
    // 还没挑出 Java 时按这个版本的下限算：GC 决策树只关心大版本，而下限就是
    // 补全时会去下的那一个。
    let java_major = java.as_ref().map_or(requirement.minimum, |java| java.major);
    // 预览不采集日志——`gc_log` 给 None，算出来的参数里就没有 -Xlog。
    let allocation = crate::plan_allocation(
        paths,
        &profile,
        &game_directory,
        java_major,
        effective.max_memory_mb,
        ceiling,
        effective.garbage_collector,
        &effective.jvm_arguments,
        None,
    );
    // 「自动会给多少」是另一个问题：手填了值的实例，滑杆旁边仍然要说得出
    // 「不改的话会怎样」。
    let automatic = crate::plan_allocation(
        paths,
        &profile,
        &game_directory,
        java_major,
        None,
        ceiling,
        effective.garbage_collector,
        &effective.jvm_arguments,
        None,
    );

    // 界面要在尺上画刻度，那要的是数不是句子。用和这次分配同一条 GC 路径去
    // 读——显示的必须是算法真正用的那份，不能两边各算各的。
    let measured = crate::memory_history(
        paths,
        &profile,
        &game_directory,
        allocation.gc.behaves_like_zgc(),
    );

    Ok(InstanceRuntime {
        automatic_memory_mb: automatic.xmx_mb,
        allocation,
        measured,
        physical_memory_mb: physical.map_or(0, |bytes| (bytes / (1024 * 1024)) as u32),
        requirement,
        java,
        mods_count: mods.count,
        defaults,
        memory_ceiling_mb: ceiling,
    })
}

/// 钉住这个实例用哪个账户。`None` 是「跟着当前账户走」。
///
/// 不走 `update_instance_settings`：那个接口整份替换 `settings`，而账户不在
/// 那份结构里——正因为它会被整份替换。
pub fn set_instance_account(
    paths: &DataPaths,
    instance_id: &str,
    account_id: Option<&str>,
) -> Result<InstanceProfile> {
    let mut profile = read_instance(paths, instance_id)?;
    profile.account_id = account_id.map(str::to_owned);
    write_instance_profile(paths, &profile)?;
    Ok(profile)
}

/// 改实例设置。整份换掉而不是逐字段打补丁：设置面板本来就是一次性提交
/// 一整屏，逐字段的接口只会在两端各埋一半的默认值。
pub fn update_instance_settings(
    paths: &DataPaths,
    instance_id: &str,
    settings: crate::InstanceSettings,
) -> Result<InstanceProfile> {
    let mut profile = read_instance(paths, instance_id)?;
    profile.settings = settings;
    write_instance_profile(paths, &profile)?;
    Ok(profile)
}

/// 删掉一个实例，连同它的存档、模组、日志。
///
/// 不可撤销，所以路径要算得准：`InstanceId` 挡住了 `..`，再 canonicalize 一次
/// 确认它确实落在 instances 目录里面——软链接能让一个合法的名字指到别处去。
pub fn delete_instance(paths: &DataPaths, instance_id: &str) -> Result<()> {
    let id = InstanceId::parse(instance_id)?;
    let root = paths.instance_root(id.as_str());
    if !root.is_dir() {
        return Err(anyhow!("实例 {instance_id} 不存在"));
    }

    let instances = fs::canonicalize(&paths.instances).context("读取实例目录")?;
    let target = fs::canonicalize(&root).context("读取实例目录")?;
    if target == instances || !target.starts_with(&instances) {
        return Err(anyhow!("{} 不在实例目录内", target.display()));
    }

    // 删的是实例目录。外部实例的实例目录里只有一份 instance.json，游戏文件
    // 在别人的目录树下——**一个都不碰**。那些文件不归我们所有。
    fs::remove_dir_all(&target).with_context(|| format!("删除 {}", target.display()))?;
    // 日志在另一棵树下，一起清掉，否则重名的新实例会捡到旧日志。
    let logs = paths.instance_log_directory(id.as_str());
    if logs.is_dir() {
        let _ = fs::remove_dir_all(logs);
    }
    // 内存历史同理：留着它，下一个拿到同一个 id 的实例会继承一份不属于它的
    // 统计，而且是看不见的那种——分配值莫名其妙地偏高或偏低。
    crate::launch::memory::history::forget(paths, id.as_str());
    // 来源记录也在另一棵树下（`security/`），同理。
    crate::instance::origin::forget(paths, id.as_str());
    // 快照没有实例就无从恢复，留着只是孤儿占盘，用量页还会列出一个已经
    // 不存在的实例。
    crate::backup::forget(paths, id.as_str());
    Ok(())
}

/// 改显示名。
///
/// 目录名（也就是实例 id）不动：它是封面的种子，也被日志目录引用着。
/// 「封面就是实例的脸」——改个名字不该换一张脸。
pub fn rename_instance(
    paths: &DataPaths,
    instance_id: &str,
    name: &str,
) -> Result<InstanceProfile> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(anyhow!("实例名称需为 1-64 个字符"));
    }
    let mut profile = read_instance(paths, instance_id)?;
    profile.name = name.to_owned();
    write_instance_profile(paths, &profile)?;
    Ok(profile)
}

/// 复制一个实例。
///
/// 带上模组和配置，不带存档、日志、崩溃报告和截图。复制实例通常是为了「同一
/// 套底子换一组模组」，把几个 G 的存档一起抄过去既慢又不是用户要的；真想要
/// 存档的人会自己去拷。
pub fn duplicate_instance(
    paths: &DataPaths,
    instance_id: &str,
    name: &str,
) -> Result<InstanceProfile> {
    let source = read_instance(paths, instance_id)?;
    // 外部实例复制不了：两个实例指着同一个游戏目录，等于两份存档互相覆盖。
    // 而把别人的目录整个复制一份到我们这里，是一个几十 G 的、用户没要求过的
    // 动作。说不行比默默做错好。
    if source.external.is_some() {
        return Err(anyhow!(
            "{} 的游戏文件在 Fern 之外，复制它会让两个实例共用同一份存档",
            source.name
        ));
    }
    let mut copy = create_instance_with_loader(
        paths,
        name,
        &source.game_version,
        source.loader,
        source.loader_component().map(|l| l.version.as_str()),
    )?;
    // 整摞层一起复制：只带主加载器那一层的话，叠着别的层的实例复制出来就少
    // 了几层，而界面上看不出区别。
    copy.components = source.components.clone();
    copy.settings = source.settings.clone();
    write_instance_profile(paths, &copy)?;

    let from = paths.game_directory(instance_id);
    let to = paths.game_directory(copy.id.as_str());
    for entry in fs::read_dir(&from).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "saves" | "logs" | "crash-reports" | "screenshots" | "natives"
        ) {
            continue;
        }
        copy_tree(&entry.path(), &to.join(name.as_ref()))?;
    }
    // 模组是原样拷过去的，那份「谁放进来的」对副本一样成立。
    crate::instance::origin::inherit(paths, instance_id, copy.id.as_str());
    Ok(copy)
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    let metadata = fs::symlink_metadata(from)?;
    if metadata.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)?;
    }
    // 软链接不跟：跟过去就成了两个实例共享同一份文件，改一个动两个。
    Ok(())
}

/// 把实例文件整份写回去。先写临时文件再改名——写到一半断电不该让实例
/// 彻底打不开。
/// 给这个实例再叠一层。
///
/// 只用于附加层（今天只有 LiteLoader）：主加载器是建实例时就定下的一次选择，
/// 换它等于换一个实例。版本取上游最新的稳定版——附加层的版本几乎没有人会去
/// 挑，而它只有一两个可选。
pub async fn add_component(
    paths: &DataPaths,
    instance_id: &str,
    kind: crate::LoaderKind,
) -> Result<InstanceProfile> {
    let mut profile = read_instance(paths, instance_id)?;
    if profile.components.iter().any(|one| one.kind == kind) {
        return Ok(profile);
    }
    let version = crate::launch::loader::latest_version(paths, kind, &profile.game_version).await?;
    // 叠在最后：层表的顺序就是合并的顺序，附加层要盖在主加载器之上。
    profile.components.push(crate::Component {
        kind,
        version,
        version_id: String::new(),
        jar_mods: Vec::new(),
    });
    let profile = profile.normalized();
    write_instance_profile(paths, &profile)?;
    Ok(profile)
}

/// 撤掉一层。主加载器撤不掉——那是建实例时的选择。
pub fn remove_component(
    paths: &DataPaths,
    instance_id: &str,
    kind: crate::LoaderKind,
) -> Result<InstanceProfile> {
    let mut profile = read_instance(paths, instance_id)?;
    if !kind.stackable() {
        return Err(anyhow!(
            "{} 是这个实例的主加载器，撤不掉",
            crate::loader_display_name(kind)
        ));
    }
    profile.components.retain(|one| one.kind != kind);
    let profile = profile.normalized();
    write_instance_profile(paths, &profile)?;
    Ok(profile)
}

pub fn write_instance_profile(paths: &DataPaths, profile: &InstanceProfile) -> Result<()> {
    // 算得出来的字段在这里重算一遍。写盘只有这一个入口，所以「主加载器和层
    // 表对不上」这件事没有别的地方能发生。
    let profile = &profile.clone().normalized();
    let bytes = serde_json::to_vec_pretty(profile).context("serialize instance profile")?;
    let path = paths.instance_config(profile.id.as_str());
    let temporary = path.with_extension("json.part");
    fs::write(&temporary, bytes).context("write instance profile")?;
    fs::rename(&temporary, &path).context("replace instance profile")?;
    Ok(())
}

/// 记下「刚刚玩过」。
///
/// 在进程真的起来之后才盖章，不是在点下启动时——补全失败、Java 找不到、
/// JVM 起不来的那些次都不算玩过，否则曲库的排序会被一串失败的尝试顶上去。
pub fn touch_played(paths: &DataPaths, instance_id: &str) -> Result<()> {
    let mut profile = read_instance(paths, instance_id)?;
    profile.last_played = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default(),
    );
    write_instance_profile(paths, &profile)
}

/// 读一个实例的配置。
///
/// 好几处都要先拿到实例才知道下一步做什么（补全要看加载器、作业要拿名字当
/// 标题），各自抄一遍 `list_instances().find()` 是把同一个「不存在」的错误
/// 写了四份。
pub fn read_instance(paths: &DataPaths, instance_id: &str) -> Result<InstanceProfile> {
    list_instances(paths)?
        .into_iter()
        .find(|profile| profile.id.as_str() == instance_id)
        .ok_or_else(|| anyhow!("instance {instance_id} does not exist"))
}

/// 已经落盘的版本元数据里声明的 Java 大版本。
///
/// 这是权威的**下限**。补全过的实例读得到，没补全过的读不到——那时只能按
/// 版本号推，界面上要说明那是估计。
pub fn read_prepared_java_major(paths: &DataPaths, version_id: &str) -> Option<u16> {
    read_prepared_metadata(paths, version_id)
        .and_then(|metadata| metadata.java_version)
        .map(|version| version.major_version)
}

fn read_prepared_metadata(
    paths: &DataPaths,
    version_id: &str,
) -> Option<fern_meta::VersionMetadata> {
    let path = paths
        .versions
        .join(version_id)
        .join(format!("{version_id}.json"));
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// 能建实例的所有版本。
///
/// `refresh` 是用户按下刷新的那一下。平时走缓存：这个列表有两千多条，六小时
/// 之内它的内容不会变，而每次打开「新建实例」都重拉一遍只是在让那一屏白等。
pub async fn list_versions(paths: &DataPaths, refresh: bool) -> Result<Vec<VersionOption>> {
    let client = DownloadClient::new(source_order(), 4);
    let cached = metacache::mutable(
        &client,
        paths,
        MANIFEST_SLUG,
        metacache::VERSION_MANIFEST_URL,
        if refresh {
            Freshness::Force
        } else {
            Freshness::Within(metacache::LISTING_TTL)
        },
    )
    .await?;
    let manifest: VersionManifest =
        serde_json::from_slice(&cached.bytes).context("解析版本清单")?;
    Ok(manifest.versions.iter().map(VersionOption::from).collect())
}

/// Crockford 的 base32：去掉了 I、L、O、U，剩下的字符念出来、抄下来都不会
/// 混。目录名是会被人读出来贴到聊天框里的东西。
const ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const ID_LENGTH: usize = 10;

/// 发一个实例 id。
///
/// **id 和名字没有关系。** 它是目录名、日志目录名，也是封面的恒定种子，一旦
/// 发出去就永不改变；名字则是随时能改的标签。曾经这里是把名字转写成 slug，
/// 三个问题：全中文的名字整串塌成同一个词（于是每台机器上第一个中文实例长着
/// 同一张脸），界面用来指代「不是某个实例」的词（地址里的 `new`）会被撞上，
/// 而且改完名字之后 id 成了旧名字的化石——一个会过期的可读名字比不可读的更
/// 糟，因为人会信它。
///
/// 那就干脆不可读。找哪个文件夹是哪个实例，走详情页的「游戏目录」，概览里也
/// 把 id 摆着可以复制。
pub(crate) fn allocate_id(paths: &DataPaths) -> Result<String> {
    for _ in 0..64 {
        let candidate = token()?;
        if !paths.instance_root(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "unable to allocate an instance id under {}",
        paths.root.display()
    ))
}

/// 只要求在这台机器上不重名，所以取模带来的那点偏差无所谓。用随机数而不是
/// 时间戳：时间戳会泄露创建时刻，系统时钟往回跳一下还会撞。
pub(crate) fn token() -> Result<String> {
    let mut bytes = [0u8; ID_LENGTH];
    getrandom::fill(&mut bytes).context("draw random bytes for the instance id")?;
    Ok(bytes
        .iter()
        .map(|byte| ID_ALPHABET[(byte % 32) as usize] as char)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_opaque_and_carry_nothing_from_the_name() {
        let root = std::env::temp_dir().join(format!("fern-ids-{}", std::process::id()));
        let paths = DataPaths::new(&root);

        // 中文名以前会整串塌成同一个 id，于是两个实例长着同一张脸。
        let first = create_instance(&paths, "余烬谷", "1.21.1").expect("create first");
        let second = create_instance(&paths, "余烬谷", "1.21.1").expect("create second");
        assert_ne!(first.id, second.id);
        assert_ne!(first.cover.identity, second.cover.identity);

        // 名字本身照常保留，转写的只是不再发生。
        assert_eq!(first.name, "余烬谷");
        for id in [first.id.as_str(), second.id.as_str()] {
            assert_eq!(id.len(), ID_LENGTH);
            assert!(
                id.bytes().all(|byte| ID_ALPHABET.contains(&byte)),
                "{id} 里有字母表之外的字符"
            );
        }

        // 地址里 instances/new 指的是新建页，实例 id 不该撞上它。
        let named_new = create_instance(&paths, "New", "1.21.1").expect("create third");
        assert_ne!(named_new.id.as_str(), "new");

        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn every_installable_loader_is_accepted_at_creation() {
        let root = std::env::temp_dir().join(format!("fern-loader-guard-{}", std::process::id()));
        let paths = DataPaths::new(&root);

        // 界面列出来的每一种都必须建得出来，否则用户走到一半才被拦住。
        // 反过来的守卫（拒掉装不上的）留着是给将来新增的 LoaderKind 兜底。
        for option in crate::installable_loaders() {
            if option.kind == crate::LoaderKind::Vanilla {
                continue;
            }
            let created = create_instance_with_loader(
                &paths,
                &option.label,
                "1.21.1",
                option.kind,
                Some("1.0.0"),
            );
            assert!(created.is_ok(), "{:?} 建不出来", option.kind);
        }

        // 装得上的照常。
        let ok = create_instance_with_loader(
            &paths,
            "Fabric",
            "1.21.1",
            crate::LoaderKind::Fabric,
            Some("0.16.5"),
        )
        .expect("fabric is installable");
        assert_eq!(ok.loader, crate::LoaderKind::Fabric);
        assert_eq!(
            ok.loader_component().map(|p| p.version.as_str()),
            Some("0.16.5")
        );
        // 装完才知道 id，建的时候留空。
        assert_eq!(
            ok.loader_component().map(|p| p.version_id.as_str()),
            Some("")
        );

        fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn settings_survive_a_round_trip_through_disk() {
        let root = std::env::temp_dir().join(format!("fern-settings-rt-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        let profile = create_instance(&paths, "Moss", "1.21.1").expect("create instance");

        let updated = update_instance_settings(
            &paths,
            profile.id.as_str(),
            crate::InstanceSettings {
                java_path: Some("/usr/lib/jvm/java-21/bin/java".into()),
                max_memory_mb: Some(6144),
                ..crate::InstanceSettings::default()
            },
        )
        .expect("update settings");
        assert_eq!(updated.settings.max_memory_mb, Some(6144));

        let reread = list_instances(&paths).expect("list instances");
        assert_eq!(reread[0].settings.max_memory_mb, Some(6144));
        // 其余字段不该被顺手改掉。
        assert_eq!(reread[0].name, "Moss");
        assert_eq!(reread[0].game_version, "1.21.1");

        fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn runtime_preview_answers_what_happens_if_nothing_is_changed() {
        let root = std::env::temp_dir().join(format!("fern-runtime-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        let profile = create_instance(&paths, "Moss", "1.21.1").expect("create instance");

        let runtime = instance_runtime(&paths, profile.id.as_str()).expect("runtime preview");
        assert_eq!(runtime.requirement.minimum, 21);
        assert!(runtime.automatic_memory_mb >= 2048);
        assert_eq!(runtime.mods_count, 0);

        fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn deleting_takes_the_game_directory_and_the_logs() {
        let root = std::env::temp_dir().join(format!("fern-delete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        let profile = create_instance(&paths, "Moss", "1.21.1").expect("create");
        let id = profile.id.as_str().to_owned();

        // 造一份存档和一份日志，确认两棵树都被清掉。
        let saves = paths.game_directory(&id).join("saves/world");
        fs::create_dir_all(&saves).expect("create save");
        fs::create_dir_all(paths.instance_log_directory(&id)).expect("create logs");
        fs::write(paths.instance_log_directory(&id).join("launch.log"), "x").expect("write log");

        delete_instance(&paths, &id).expect("delete");
        assert!(!paths.instance_root(&id).exists());
        // 日志留着的话，重名的新实例会捡到旧日志。
        assert!(!paths.instance_log_directory(&id).exists());
        assert!(list_instances(&paths).expect("list").is_empty());

        // 删两次不该 panic，只是说它不在了。
        assert!(delete_instance(&paths, &id).is_err());
        // 目录名里的花招要在 InstanceId 那一关就被挡住。
        assert!(delete_instance(&paths, "../..").is_err());

        fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn renaming_changes_the_label_but_not_the_identity() {
        let root = std::env::temp_dir().join(format!("fern-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        let before = create_instance(&paths, "Moss", "1.21.1").expect("create");

        let after = rename_instance(&paths, before.id.as_str(), "  余烬谷  ").expect("rename");
        assert_eq!(after.name, "余烬谷");
        // id 是封面的种子，也被日志目录引用着——改名不该换一张脸。
        assert_eq!(after.id, before.id);
        assert_eq!(after.cover.identity, before.cover.identity);
        assert!(paths.instance_root(before.id.as_str()).is_dir());

        assert!(rename_instance(&paths, before.id.as_str(), "   ").is_err());
        fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn duplicating_carries_the_mods_but_not_the_saves() {
        let root = std::env::temp_dir().join(format!("fern-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        let source = create_instance_with_loader(
            &paths,
            "Moss",
            "1.21.1",
            crate::LoaderKind::Fabric,
            Some("0.16.5"),
        )
        .expect("create");
        let id = source.id.as_str().to_owned();

        let game = paths.game_directory(&id);
        fs::create_dir_all(game.join("mods")).expect("mods");
        fs::write(game.join("mods/sodium.jar"), "jar").expect("mod");
        fs::create_dir_all(game.join("config/sodium")).expect("config");
        fs::write(game.join("config/sodium/options.json"), "{}").expect("config file");
        fs::create_dir_all(game.join("saves/world")).expect("saves");
        fs::write(game.join("saves/world/level.dat"), "big").expect("save");

        let copy = duplicate_instance(&paths, &id, "Moss 副本").expect("duplicate");
        assert_ne!(copy.id, source.id);
        assert_eq!(copy.name, "Moss 副本");
        assert_eq!(copy.loader, crate::LoaderKind::Fabric);

        let copied = paths.game_directory(copy.id.as_str());
        assert!(copied.join("mods/sodium.jar").is_file());
        // 嵌套的配置目录也要跟过去。
        assert!(copied.join("config/sodium/options.json").is_file());
        // 存档不跟：几个 G 抄一遍既慢又不是用户要的。
        assert!(!copied.join("saves").exists());

        fs::remove_dir_all(root).expect("remove root");
    }

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
