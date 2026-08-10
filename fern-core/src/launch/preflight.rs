//! 启动之前先看一眼。
//!
//! 最痛的失败是这样的：装完一个整合包，点启动，黑框一闪，没了。而那一类失败
//! **在点启动之前就能看出来**——缺前置、同一个模组装了两份、模组不适配这个游戏
//! 版本或这个加载器。这些答案全在那些 jar 自己的元数据里，读一遍几百毫秒。
//!
//! 与崩溃分析的分工很清楚：**崩溃分析回答「刚才为什么死了」，预检查回答「按现在
//! 这样点下去会不会死」。** 崩溃那边每条规则都要一条文本证据；这边一条文本都
//! 没有，全是事实比对。两者共用同一个 [`Action`]，于是界面上那颗按钮是同一颗。
//!
//! **只报确定的事。** 可选依赖缺了不报，禁用的模组不参与依赖判断但会被单独提
//! 一句。一个基于误解的警告会让用户去动一个本来没问题的模组，比不报更糟。
//!
//! 于是看不懂的版本区间往哪边倒，要看这一条在问什么：问「它适配吗」时当作
//! 满足，问「它会不会和谁打架」时当作不满足。两边都是同一句「宁可漏报」，
//! 而写成同一个函数就会有一边说反（见 `ranges::contains`）。
//!
//! 预检查**不阻止启动**。它给的是判断依据，不是许可——用户可能比我们更清楚。
//!
//! 和崩溃分析一样，这一层不产出句子：给出的是文案 id（`preflight.<kind>`）加
//! 一组参数，措辞与翻译都在 `fern-ui/src/lib/i18n/`。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    DataPaths, InstanceProfile, LoaderKind,
    instance::jar::{self, Manifest, ModJar, Relation},
    launch::{crash::Action, mcversion, ranges},
};

/// 一条要说给用户的话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// 这一条在这次检查里的唯一键，界面拿它做列表的 key。
    pub id: String,
    /// 文案 id：界面按 `preflight.<kind>` 查。取值见 [`kind`] 模块。
    pub kind: String,
    pub severity: Severity,
    /// 文案里的占位符。
    pub args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// 预检查会说的话，全部在这里。
///
/// 列成常量而不是散在各处的字符串字面量：界面那边要为每一条备好文案，而
/// 「有哪些条」必须有一个地方说得清楚。
pub mod kind {
    /// 实例没有加载器，mods 里的东西不会被加载。
    pub const NO_LOADER: &str = "no-loader";
    /// 同一个模组装了多份。
    pub const DUPLICATE: &str = "duplicate";
    /// 模组是给另一个加载器的。
    pub const WRONG_LOADER: &str = "wrong-loader";
    /// 模组声明的游戏版本不含当前版本。
    pub const WRONG_GAME_VERSION: &str = "wrong-game-version";
    /// 必需的前置没装。
    pub const MISSING_DEPENDENCY: &str = "missing-dependency";
    /// 前置装了，但被关掉了。
    pub const DISABLED_DEPENDENCY: &str = "disabled-dependency";
    /// 两个模组装在一起会起不来。
    pub const INCOMPATIBLE: &str = "incompatible";
    /// 模组要求的 Java 大版本，不是这个实例会用的那个。
    pub const WRONG_JAVA: &str = "wrong-java";

    /// 全部取值。界面那边的文案表按它对齐。
    pub const ALL: [&str; 8] = [
        NO_LOADER,
        DUPLICATE,
        WRONG_LOADER,
        WRONG_GAME_VERSION,
        MISSING_DEPENDENCY,
        DISABLED_DEPENDENCY,
        INCOMPATIBLE,
        WRONG_JAVA,
    ];
}

fn args<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// 大概率起不来。
    Blocking,
    /// 可能有问题，但不一定。
    Warning,
}

/// 这个实例的游戏版本，两副面孔。
///
/// `id` 是磁盘上、也是界面上那个名字；`semantic` 是 fabric-loader 归一化之后的
/// 那个（`25w15a` → `1.21.6-alpha.25.15.a`，见 [`mcversion::semantic`]）。
///
/// 要两份，是因为**两个加载器阵营比的不是同一个版本号**，见 [`Game::comparable`]。
#[derive(Debug, Clone)]
pub struct Game {
    pub id: String,
    pub semantic: Option<String>,
}

impl Game {
    /// `release_target` 是游戏自己写在 client jar 里的那个正式版号，只有快照
    /// 需要它（见 [`version::release_target`](super::version::release_target)）。
    pub fn of(id: &str, release_target: Option<&str>) -> Self {
        Self {
            id: id.to_owned(),
            semantic: mcversion::semantic(id, release_target),
        }
    }

    /// 拿去和这个加载器的模组声明的区间比的那个版本号。`None` 是「比不了」。
    ///
    /// Fabric 和 Quilt 的模组写的是 loader 归一化之后的语义化版本号，所以比的是
    /// `semantic`。Forge 和 NeoForge 不归一化：FML 把 minecraft 当成一个版本号就是
    /// `1.21.1` 的内置模组，`versionRange = "[1.21.1,1.22)"` 直接拿它原样比，所以
    /// 这一边比的是 `id`。
    ///
    /// 但只在正式版上比。Maven 区间容不下 `25w14craftmine` 这种 id——`[1.21.6,)`
    /// 对上它，得出的照样是「一个模组都不兼容」。这两个加载器本来也只为正式版
    /// 出构建，落到这里说明这是一份我们不认识的东西，不猜。
    fn comparable(&self, loader: LoaderKind) -> Option<&str> {
        match loader {
            LoaderKind::Fabric | LoaderKind::Quilt => self.semantic.as_deref(),
            LoaderKind::Forge | LoaderKind::NeoForge | LoaderKind::Vanilla => {
                mcversion::is_release(&self.id).then_some(self.id.as_str())
            }
        }
    }
}

/// 看一遍这个实例。没有问题就是空列表。
pub fn check(paths: &DataPaths, profile: &InstanceProfile) -> Vec<Finding> {
    let scoped = crate::instance::paths_for(paths, profile);
    let jars = jar::read_all(&jar::directory(&scoped, profile.id.as_str()));
    let game = Game::of(
        &profile.game_version,
        super::version::release_target(&scoped, profile).as_deref(),
    );
    inspect(
        &jars,
        profile.loader,
        &game,
        java_major(paths, profile, &jars),
    )
}

/// 这个实例点下去会用哪个大版本的 Java。
///
/// 和启动走同一条路（`java_requirement` + `resolve_java_runtime`），**包括那两条
/// 容易漏掉的输入**：元数据里声明的大版本，和模组自己要求的下界。少一条，预检查
/// 说的就是一回事、真跑起来用的是另一份 Java——而这里说出口的正是「你会用哪个
/// Java」，说错了就是凭空捏造一条警告。
///
/// 挑不出来时返回 `None`——那时该说的话是「没有可用的 Java」，那是启动自己会报
/// 的错，不该在这里变成一条关于某个模组的警告。
fn java_major(paths: &DataPaths, profile: &InstanceProfile, jars: &[ModJar]) -> Option<u16> {
    let declared = crate::read_prepared_java_major(paths, &profile.game_version);
    let requirement = crate::java::requirement(&profile.game_version, profile.loader, declared)
        .preferring(java_floor(jars));
    super::resolve_java_runtime(paths, profile, &requirement)
        .ok()
        .map(|runtime| runtime.major)
}

/// 这些模组里，要求得最高的那条 Java 下界。
///
/// 加载器把 Java 当成一个内置模组，`depends: { "java": ">=25" }` 是一条真的会让
/// 它拒绝启动的约束。装了这样的模组，「自动」就该去挑一个 25——否则自动挑出来的
/// 那份 Java 能跑游戏、跑不了这一屋子模组，而用户看到的只是「点了启动，什么也
/// 没发生」。
///
/// **只认读得懂的下界。** `>=25`、`[25,)`、`25` 认，`<=17` 这样的上界和看不懂的
/// 写法一律不算——把一个上界当成下界去挑 Java，比不挑更糟。
///
/// 不分加载器地看每一份清单：一个多加载器打包的模组要 Java 25，它三份清单里
/// 写的是同一件事，而挑错 Java 的代价是「点了启动什么也没发生」。
pub fn java_floor(jars: &[ModJar]) -> Option<u16> {
    jars.iter()
        .filter(|jar| jar.enabled)
        .flat_map(|jar| jar.every_requirement())
        .filter(|dependency| dependency.mod_id == "java")
        .filter_map(|dependency| lower_bound(&dependency.range))
        .max()
}

/// 区间的下界，读不出来就是 `None`。
fn lower_bound(range: &str) -> Option<u16> {
    let range = range.trim();
    let (rest, exclusive) = match range.strip_prefix(">=").or_else(|| range.strip_prefix('[')) {
        Some(rest) => (rest, false),
        None => match range.strip_prefix('>') {
            Some(rest) => (rest, true),
            // 光秃秃的一个版本号（`=25` 也算）是「正好这个」，下界就是它自己。
            None => (range.strip_prefix('=').unwrap_or(range), false),
        },
    };
    let token: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if token.is_empty() {
        return None;
    }
    // 后面还跟着别的东西（`>=1.8` 的 `.8`、`~25` 的波浪号已经在上面落空）就不认：
    // Java 的大版本是一个整数，多出来的那一截说明这不是我们以为的那种写法。
    let tail = rest.trim_start()[token.len()..].trim_start();
    if !tail.is_empty()
        && !tail.starts_with(',')
        && !tail.starts_with(']')
        && !tail.starts_with(')')
    {
        return None;
    }
    let major: u16 = token.parse().ok()?;
    Some(if exclusive { major + 1 } else { major })
}

/// 纯函数那一半：给定这些 jar 和这个上下文，有什么话要说。
///
/// 和磁盘分开，于是它能对着构造出来的元数据单独测——而这一层的价值全在判断上，
/// 不在读文件上。
pub fn inspect(
    jars: &[ModJar],
    loader: LoaderKind,
    minecraft: &Game,
    java_major: Option<u16>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let enabled: Vec<&ModJar> = jars.iter().filter(|jar| jar.enabled).collect();
    if enabled.is_empty() {
        return findings;
    }

    // 原版实例里的 mods 目录不会被任何人读。这是最省事、也最容易被忽略的一条。
    if loader == LoaderKind::Vanilla {
        findings.push(Finding {
            id: kind::NO_LOADER.to_owned(),
            kind: kind::NO_LOADER.to_owned(),
            severity: Severity::Blocking,
            args: args([("count", enabled.len().to_string())]),
            action: None,
        });
        return findings;
    }

    findings.extend(duplicates(&enabled));
    findings.extend(wrong_loader(&enabled, loader, minecraft));

    // 加载器读不了的那些，剩下的话就不必再说了：它根本不会被加载，谈不上缺前置，
    // 也谈不上适配哪个游戏版本。该说的那一句 `wrong_loader` 已经说了，再追一条
    // 「缺 Fabric API」只会让用户去装一个他根本不需要的东西。
    //
    // 剩下的检查一律对着**生效的那一份清单**问，不再碰 jar 里别的那些。
    let loadable: Vec<(&ModJar, &Manifest)> = enabled
        .iter()
        .filter_map(|jar| live(jar, loader, minecraft).map(|manifest| (*jar, manifest)))
        .collect();
    findings.extend(wrong_game_version(&loadable, minecraft, loader));
    findings.extend(missing_dependencies(jars, &loadable, loader));
    findings.extend(incompatible(&loadable));
    findings.extend(wrong_java(&loadable, java_major));
    findings.sort_by_key(|finding| finding.severity);
    findings
}

/// 同一个 modid 出现两次。加载器不知道该用哪个，多半直接拒绝启动。
fn duplicates(enabled: &[&ModJar]) -> Vec<Finding> {
    let mut seen: std::collections::HashMap<&str, Vec<&ModJar>> = std::collections::HashMap::new();
    for jar in enabled {
        if let Some(mod_id) = &jar.mod_id {
            seen.entry(mod_id).or_default().push(jar);
        }
    }
    let mut findings: Vec<Finding> = seen
        .into_iter()
        .filter(|(_, jars)| jars.len() > 1)
        .map(|(mod_id, jars)| {
            let names: Vec<&str> = jars.iter().map(|jar| jar.file_name.as_str()).collect();
            Finding {
                id: format!("{}:{mod_id}", kind::DUPLICATE),
                kind: kind::DUPLICATE.to_owned(),
                severity: Severity::Blocking,
                args: args([
                    ("mod", jars[0].name.clone()),
                    ("count", jars.len().to_string()),
                    ("files", names.join("、")),
                ]),
                // 保留第一份，其余的给一个能点的删除——多出来的那些才是问题。
                action: Some(Action::RemoveMod {
                    file: jars[1].file_name.clone(),
                }),
            }
        })
        .collect();
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// 这个实例会读这个 jar 里的哪一份清单。`None` 是「一份都不读」。
///
/// 读不出清单的 jar 返回它自己那份空的：一个我们看不懂的 jar 照样能被加载器
/// 加载，不该因为看不懂就判它出局。
fn live<'a>(jar: &'a ModJar, loader: LoaderKind, minecraft: &Game) -> Option<&'a Manifest> {
    const NOTHING: &Manifest = &Manifest {
        loader: LoaderKind::Vanilla,
        depends: Vec::new(),
    };
    if jar.manifests.is_empty() {
        return Some(NOTHING);
    }
    jar.manifests
        .iter()
        // 先找一模一样的：Quilt 实例上，一个同时带两份清单的 jar 该读它的
        // `quilt.mod.json`，而不是那份将就着也能用的 Fabric 清单。
        .find(|manifest| manifest.loader == loader)
        .or_else(|| {
            jar.manifests
                .iter()
                .find(|manifest| accepts(loader, manifest.loader, minecraft))
        })
}

/// 这个实例的加载器读不读得了那一份清单。
///
/// Quilt 读得了 Fabric 的模组。NeoForge 那边要看版本：**它换过一次元数据文件
/// 名**——20.5（也就是 1.20.5）之前读的是 Forge 那份 `META-INF/mods.toml`，从
/// 20.5 起只认 `neoforge.mods.toml`。所以在 1.20.4 及更早的 NeoForge 实例里，
/// 一个只带 `mods.toml` 的 jar 是正常的，多半就是为 NeoForge 编的：那个年代
/// 两家的清单长得一模一样，我们分不出来，加载器也分不出来。
///
/// 认不出的版本按「读得了」算。这条判断报出来的是一句「删掉这个模组」，而
/// 猜错的代价是让用户去删一个本来在正常工作的模组。
fn accepts(instance: LoaderKind, manifest: LoaderKind, minecraft: &Game) -> bool {
    if instance == manifest {
        return true;
    }
    match (instance, manifest) {
        (LoaderKind::Quilt, LoaderKind::Fabric) => true,
        (LoaderKind::NeoForge, LoaderKind::Forge) => {
            !mcversion::is_release(&minecraft.id)
                || ranges::compare(&minecraft.id, "1.20.5") == std::cmp::Ordering::Less
        }
        _ => false,
    }
}

/// Fabric 的模组放进 Forge 实例里不会被加载，反之亦然。
///
/// 但**一个 jar 可以同时是好几家的**，所以要一份清单都对不上才算数：只要有一
/// 份是这个实例读得了的，它就是装对了地方。
fn wrong_loader(enabled: &[&ModJar], loader: LoaderKind, minecraft: &Game) -> Vec<Finding> {
    enabled
        .iter()
        .filter(|jar| live(jar, loader, minecraft).is_none())
        .map(|jar| Finding {
            id: format!("{}:{}", kind::WRONG_LOADER, jar.file_name),
            kind: kind::WRONG_LOADER.to_owned(),
            severity: Severity::Blocking,
            // 加载器名是个术语，交给界面去译；这里只给取值。逗号分开的一串，
            // 因为一个 jar 可以写着好几家——只是没有一家是这个实例。
            args: args([
                ("mod", jar.name.clone()),
                ("instanceLoader", tag(loader).to_owned()),
                (
                    "modLoader",
                    jar.loaders()
                        .into_iter()
                        .map(tag)
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ]),
            action: Some(Action::RemoveMod {
                file: jar.file_name.clone(),
            }),
        })
        .collect()
}

/// 模组自己声明的 MC 版本区间不含这个实例的版本。
///
/// 比之前先要有一个**比得了**的版本号，而是哪一个要看加载器（[`Game::comparable`]）。
/// 拿不到就整项不做：`25w14craftmine` 去比 `>=1.21.6`，比出来的是「一个模组都不
/// 兼容」，而那句话本身才是错的。
fn wrong_game_version(
    loadable: &[(&ModJar, &Manifest)],
    minecraft: &Game,
    loader: LoaderKind,
) -> Vec<Finding> {
    let Some(version) = minecraft.comparable(loader) else {
        return Vec::new();
    };
    loadable
        .iter()
        .filter_map(|(jar, manifest)| {
            let range = manifest.minecraft_range()?;
            (!ranges::satisfies(range, version)).then(|| Finding {
                id: format!("{}:{}", kind::WRONG_GAME_VERSION, jar.file_name),
                kind: kind::WRONG_GAME_VERSION.to_owned(),
                severity: Severity::Warning,
                args: args([
                    ("mod", jar.name.clone()),
                    // 说给人听的仍然是他认得的那个 id。
                    ("minecraft", minecraft.id.clone()),
                    ("range", range.to_owned()),
                ]),
                action: None,
            })
        })
        .collect()
}

/// 必需的前置没装。这是最常见、也最容易修的一条。
///
/// **只有 `Required` 那一种算数。** 可选的缺了不是问题；而 `Incompatible` 说的是
/// 「装了它才起不来」，拿它去报「缺少 X」，等于劝用户亲手把游戏弄坏。
fn missing_dependencies(
    all: &[ModJar],
    loadable: &[(&ModJar, &Manifest)],
    loader: LoaderKind,
) -> Vec<Finding> {
    // 加载器和游戏自己也是依赖项，但它们不在 mods 目录里，不该被当成缺失。
    // 一个 jar 里打包的那些 jar 同样算装了（`ModJar::provides`）——Fabric API
    // 的四十来个模块就是这么进来的。
    let provided: std::collections::HashSet<String> = loadable
        .iter()
        .flat_map(|(jar, _)| jar.mod_id.iter().chain(jar.provides.iter()).cloned())
        .chain(builtin(loader))
        .collect();

    let mut findings = Vec::new();
    let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (jar, manifest) in loadable {
        for dependency in manifest.required() {
            if provided.contains(&dependency.mod_id) {
                continue;
            }
            // 缺的是 Fabric API 的某个模块时，要说的是「装 Fabric API」——
            // 模块名单独拿去搜什么也搜不到。
            let wanted = supplier(&dependency.mod_id);
            if !reported.insert(wanted.to_owned()) {
                continue;
            }
            // 装了但被关掉了是另一回事：让用户开回来，而不是再下一份。
            let disabled = all
                .iter()
                .find(|other| !other.enabled && other.supplies(&dependency.mod_id));
            findings.push(match disabled {
                Some(off) => Finding {
                    id: format!("{}:{wanted}", kind::DISABLED_DEPENDENCY),
                    kind: kind::DISABLED_DEPENDENCY.to_owned(),
                    severity: Severity::Blocking,
                    args: args([("dependency", off.name.clone()), ("mod", jar.name.clone())]),
                    action: None,
                },
                None => Finding {
                    id: format!("{}:{wanted}", kind::MISSING_DEPENDENCY),
                    kind: kind::MISSING_DEPENDENCY.to_owned(),
                    severity: Severity::Blocking,
                    args: args([("dependency", wanted.to_owned()), ("mod", jar.name.clone())]),
                    action: Some(Action::InstallMod {
                        query: wanted.to_owned(),
                    }),
                },
            });
        }
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// 两个模组装在一起会起不来。
///
/// 和「缺前置」正好相反的那一条：`incompatible` 说的是「这个模组在场，我就拒绝
/// 加载」，加载器真的会照此罢工。补丁型模组最爱用它——AllTheLeaks 一口气点名十
/// 来个模组的老版本，理由是那些版本有内存泄漏。
///
/// **只在能确定的时候说话。** 要三件事同时成立：那个模组确实装着、它的版本号
/// 读得出来、而那个版本号**确定**落在点名的区间里。区间看不懂就闭嘴——别处的
/// 「看不懂就当满足」在这里会变成凭空捏造一条冲突，而这一条是拦路的红字。
///
/// `discouraged` 不在这里。加载器自己也只是记一行日志然后照常启动，那是「可能
/// 有问题」，不是「起不来」，我们不跟着喊。
fn incompatible(loadable: &[(&ModJar, &Manifest)]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (jar, manifest) in loadable {
        let refused = manifest
            .depends
            .iter()
            .filter(|dependency| dependency.relation == Relation::Incompatible);
        for dependency in refused {
            let Some(other) = loadable
                .iter()
                .map(|(other, _)| *other)
                .filter(|other| other.file_name != jar.file_name)
                .find(|other| other.supplies(&dependency.mod_id))
            else {
                continue;
            };
            // 版本号读不出来就没法判断它落不落在区间里，而「装了就是冲突」
            // 只在区间是「所有版本」时才成立——那种写法这里一样走得通。
            let Some(version) = other.version.as_deref() else {
                continue;
            };
            if ranges::contains(&dependency.range, version) != Some(true) {
                continue;
            }
            findings.push(Finding {
                id: format!(
                    "{}:{}:{}",
                    kind::INCOMPATIBLE,
                    jar.file_name,
                    dependency.mod_id
                ),
                kind: kind::INCOMPATIBLE.to_owned(),
                severity: Severity::Blocking,
                args: args([
                    ("mod", jar.name.clone()),
                    ("other", other.name.clone()),
                    ("version", version.to_owned()),
                    // Forge 那边不写 versionRange 就是「所有版本」。
                    ("range", displayed_range(&dependency.range)),
                ]),
                // 不给按钮。删哪一个、还是把另一个升上去，这里判断不了——而多数
                // 时候正确答案是后者，一颗「移除」会把用户带到反方向。
                action: None,
            });
        }
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// 说给人看的区间。空的就是「所有版本」，写成通配符总好过一片空白。
fn displayed_range(range: &str) -> String {
    let range = range.trim();
    if range.is_empty() {
        "*".to_owned()
    } else {
        range.to_owned()
    }
}

/// 模组要求的 Java 大版本，和这个实例会用的那个对不上。
///
/// 加载器把 Java 当成一个版本号等于 `java.specification.version` 的内置模组，
/// 所以 `depends: { "java": ">=22" }` 是一条真的会让它拒绝启动的约束，而它和
/// 「这个游戏版本需要 Java 几」是两回事——后者由启动时的 requirement 管。
///
/// 同一个大版本只说一次：十个模组要求 Java 22，要做的仍然只有一件事。
fn wrong_java(loadable: &[(&ModJar, &Manifest)], java_major: Option<u16>) -> Vec<Finding> {
    let Some(current) = java_major else {
        return Vec::new();
    };
    let running = current.to_string();
    let mut findings = Vec::new();
    let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (jar, manifest) in loadable {
        for dependency in manifest.required() {
            if dependency.mod_id != "java" || ranges::satisfies(&dependency.range, &running) {
                continue;
            }
            if !reported.insert(dependency.range.clone()) {
                continue;
            }
            findings.push(Finding {
                id: format!("{}:{}", kind::WRONG_JAVA, dependency.range),
                kind: kind::WRONG_JAVA.to_owned(),
                severity: Severity::Blocking,
                args: args([
                    ("mod", jar.name.clone()),
                    ("range", dependency.range.clone()),
                    ("java", running.clone()),
                ]),
                // 区间读不出一个大版本时不给按钮：一个点了会跳到错误版本的
                // 「换 Java」比没有这颗按钮更糟。
                action: wanted_major(&dependency.range).map(|major| Action::UseJava { major }),
            });
        }
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// 区间里那个大版本：`>=22` 与 `[17,)` 都是 17/22，`>=1.8` 是 8。
///
/// 只取区间里第一个数字。它可能是下界（`>=22`）也可能是上界（`<=17`），但两种
/// 情况下要换的都正是这个版本。
fn wanted_major(range: &str) -> Option<u16> {
    let start = range.find(|c: char| c.is_ascii_digit())?;
    let token: String = range[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let token = token.strip_prefix("1.").unwrap_or(token.as_str());
    token.split('.').next()?.parse().ok()
}

/// 缺了这个 id，实际要装的是哪个模组。
///
/// 只有一条规则，因为只有一处名实不符：Fabric API 的模块 id 长成
/// `fabric-<名字>-v<数字>`（外加一个 `fabric-api-base`），它们没有一个是能单独
/// 下载的东西，全都在 `fabric-api` 这一个 jar 里。`fabric-language-kotlin` 这类
/// 真的独立模组不带 `-v<数字>` 结尾，落不进这条规则。
///
/// 崩溃诊断那边绑定 `install-mod` 时也走这一条（`crash::rules::bind`）：日志里
/// 点名的同样是模块 id，而「实际要装什么」是一件关于世界的事实，不该在两处
/// 各写一份。
pub(crate) fn supplier(mod_id: &str) -> &str {
    if mod_id == "fabric-api-base" {
        return "fabric-api";
    }
    let module = mod_id.starts_with("fabric-")
        && mod_id.rsplit_once("-v").is_some_and(|(head, tail)| {
            !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())
        });
    if module { "fabric-api" } else { mod_id }
}

/// 加载器自带的那些 id。模组会把它们写进 depends，但它们不是 mods 里的文件。
///
/// **`fabric` 不在这里面。** 它是 Fabric API 自己的 id（新版写成 `fabric-api`
/// 加一条 `provides: ["fabric"]`），不是加载器提供的——fabric-loader 注册的内置
/// 模组只有 `minecraft`、`java`、`fabricloader` 三个。把它当成自带，等于让所有
/// 写 `depends: fabric` 的模组永远报不出缺前置，而那是最常见的一种缺前置。
///
/// `java` 留在这里：它永远存在，只可能版本不对，那件事由 [`wrong_java`] 说。
fn builtin(loader: LoaderKind) -> Vec<String> {
    let mut names = vec!["minecraft".to_owned(), "java".to_owned()];
    names.extend(
        match loader {
            LoaderKind::Fabric => ["fabricloader", "fabric-loader"].as_slice(),
            LoaderKind::Quilt => ["quilt_loader", "quilt_base", "fabricloader"].as_slice(),
            LoaderKind::Forge => ["forge", "fml"].as_slice(),
            LoaderKind::NeoForge => ["neoforge", "forge", "fml"].as_slice(),
            LoaderKind::Vanilla => [].as_slice(),
        }
        .iter()
        .map(|name| (*name).to_owned()),
    );
    names
}

/// 加载器的机器名。界面上显示成什么由界面决定——「原版」要翻译，Fabric 不用。
fn tag(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Vanilla => "vanilla",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::instance::jar::{Dependency, Relation};

    fn jar(name: &str, mod_id: &str, loader: LoaderKind) -> ModJar {
        ModJar {
            file_name: format!("{name}.jar"),
            enabled: true,
            mod_id: Some(mod_id.to_owned()),
            name: name.to_owned(),
            version: Some("1.0".to_owned()),
            manifests: vec![Manifest {
                loader,
                depends: Vec::new(),
            }],
            provides: Vec::new(),
            packages: Vec::new(),
        }
    }

    /// 往第一份清单里加一条依赖。
    fn needs(jar: ModJar, mod_id: &str, range: &str) -> ModJar {
        declares(jar, 0, mod_id, range)
    }

    /// 往第 `index` 份清单里加一条必需依赖——多加载器打包的模组，各家写各家的。
    fn declares(jar: ModJar, index: usize, mod_id: &str, range: &str) -> ModJar {
        related(jar, index, mod_id, range, Relation::Required)
    }

    fn related(
        mut jar: ModJar,
        index: usize,
        mod_id: &str,
        range: &str,
        relation: Relation,
    ) -> ModJar {
        jar.manifests[index].depends.push(Dependency {
            mod_id: mod_id.to_owned(),
            range: range.to_owned(),
            relation,
        });
        jar
    }

    #[test]
    fn a_missing_dependency_is_found_before_the_game_starts() {
        let jars = vec![needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "fabric-api",
            "*",
        )];
        let findings = inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::MISSING_DEPENDENCY);
        assert_eq!(findings[0].args["dependency"], "fabric-api");
        assert_eq!(
            findings[0].action,
            Some(Action::InstallMod {
                query: "fabric-api".to_owned()
            })
        );
    }

    /// 加载器和游戏自己也写在 depends 里，但它们不在 mods 目录里。
    #[test]
    fn the_loader_and_the_game_are_not_missing_dependencies() {
        let mut sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "minecraft",
            "*",
        );
        sodium = needs(sodium, "fabricloader", ">=0.15");
        assert!(
            inspect(
                &[sodium],
                LoaderKind::Fabric,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );
    }

    /// 装了但被关掉了，该说的是「打开它」，不是「去下一份」。
    #[test]
    fn a_disabled_dependency_asks_to_be_switched_on_not_downloaded() {
        let mut api = jar("Fabric API", "fabric-api", LoaderKind::Fabric);
        api.enabled = false;
        api.file_name = "fabric-api.jar.disabled".to_owned();
        let jars = vec![
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "fabric-api",
                "*",
            ),
            api,
        ];
        let findings = inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None);
        assert_eq!(findings[0].id, "disabled-dependency:fabric-api");
        assert_eq!(findings[0].kind, kind::DISABLED_DEPENDENCY);
        assert!(findings[0].action.is_none());
    }

    /// `fabric` 是 Fabric API 的 id，不是加载器给的。
    ///
    /// 真实的日志里，Fabric 自己说的是「模组 'Common Network' 需要 fabric 的
    /// 任意版本，但没有安装它」——把它当成自带，这条就永远报不出来。
    #[test]
    fn depending_on_fabric_api_by_its_bare_id_is_still_a_missing_dependency() {
        let jars = vec![needs(
            jar("Common Network", "commonnetworking", LoaderKind::Fabric),
            "fabric",
            "*",
        )];
        let findings = inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::MISSING_DEPENDENCY);
        assert_eq!(findings[0].args["dependency"], "fabric");
    }

    /// 装着的 Fabric API 顶得上 `fabric`——`provides` 就是干这个的。
    #[test]
    fn an_installed_fabric_api_supplies_the_bare_id() {
        let mut api = jar("Fabric API", "fabric-api", LoaderKind::Fabric);
        api.provides = vec!["fabric".to_owned()];
        let jars = vec![
            needs(
                jar("Common Network", "commonnetworking", LoaderKind::Fabric),
                "fabric",
                "*",
            ),
            api,
        ];
        assert!(inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None).is_empty());
    }

    /// 模组要的 Java 比这个实例会用的那份新。加载器会因此拒绝启动。
    #[test]
    fn a_mod_that_needs_a_newer_java_than_the_instance_will_use() {
        let jars = vec![needs(
            jar("C2ME", "c2me-opts-natives-math", LoaderKind::Fabric),
            "java",
            ">=22",
        )];
        let findings = inspect(
            &jars,
            LoaderKind::Fabric,
            &Game::of("1.21.5", None),
            Some(21),
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::WRONG_JAVA);
        assert_eq!(findings[0].args["java"], "21");
        assert_eq!(findings[0].action, Some(Action::UseJava { major: 22 }));

        // 满足了就不说话，挑不出 Java 时也不说——那是另一回事。
        assert!(
            inspect(
                &jars,
                LoaderKind::Fabric,
                &Game::of("1.21.5", None),
                Some(22)
            )
            .is_empty()
        );
        assert!(inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.5", None), None).is_empty());
    }

    #[test]
    fn the_wanted_java_major_comes_out_of_either_kind_of_bound() {
        assert_eq!(wanted_major(">=22"), Some(22));
        assert_eq!(wanted_major("[17,)"), Some(17));
        assert_eq!(wanted_major(">=1.8"), Some(8));
        assert_eq!(wanted_major("*"), None);
    }

    /// 装着的模组把「自动」要挑的 Java 抬高了。要得最狠的那一条说了算。
    #[test]
    fn the_mods_raise_the_java_the_instance_will_pick() {
        let mut off = needs(jar("旧的", "old", LoaderKind::Fabric), "java", ">=99");
        off.enabled = false;
        let jars = vec![
            needs(jar("Sodium", "sodium", LoaderKind::Fabric), "java", ">=21"),
            needs(jar("C2ME", "c2me", LoaderKind::Fabric), "java", ">=25"),
            // 关掉的那些不参与：它们不会被加载，也就提不出要求。
            off,
        ];
        assert_eq!(java_floor(&jars), Some(25));
        assert_eq!(java_floor(&[]), None);
    }

    /// 只认读得懂的下界。把一个上界当成下界去挑 Java，比不挑更糟。
    #[test]
    fn only_a_lower_bound_counts_as_one() {
        assert_eq!(lower_bound(">=25"), Some(25));
        assert_eq!(lower_bound("[25,)"), Some(25));
        assert_eq!(lower_bound("25"), Some(25));
        assert_eq!(lower_bound("=25"), Some(25));
        // 开区间的下界是下一个。
        assert_eq!(lower_bound(">24"), Some(25));
        // 上界、通配、看不懂的写法，以及 `1.8` 那种老写法，一概不算。
        assert_eq!(lower_bound("<=17"), None);
        assert_eq!(lower_bound("*"), None);
        assert_eq!(lower_bound(">=1.8"), None);
        assert_eq!(lower_bound("谁知道"), None);
    }

    /// 快照上照样比，而且比得对——两个方向都要对。
    ///
    /// 这条检查在快照上一度是全错的：拿 `25w14craftmine` 去比区间，比出来的是
    /// 「装的每一个模组都不兼容」。翻译（`mcversion`）之后它才有意义，而有意义
    /// 意味着**两种结论都出得来**：为这个快照发布的说没事，为上一个正式版编译
    /// 的照报。
    #[test]
    fn mods_are_judged_against_the_snapshot_they_are_written_for() {
        let ready = |range: &str| {
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "minecraft",
                range,
            )
        };
        // 愚人节版本在 loader 的对照表里，client jar 说什么都不改变答案。
        let craftmine = Game::of("25w14craftmine", Some("1.21.6"));
        assert_eq!(
            craftmine.semantic.as_deref(),
            Some("1.21.6-alpha.25.14.craftmine")
        );

        // 为它发布的模组：不报。
        assert!(
            inspect(
                &[ready(">=1.21.6-alpha.25.14.craftmine")],
                LoaderKind::Fabric,
                &craftmine,
                None
            )
            .is_empty()
        );
        // 为上一个正式版编译的：照报，而且说给人听的仍然是他认得的那个 id。
        let findings = inspect(&[ready("1.21.5")], LoaderKind::Fabric, &craftmine, None);
        assert_eq!(findings[0].kind, kind::WRONG_GAME_VERSION);
        assert_eq!(findings[0].args["minecraft"], "25w14craftmine");

        // 周更快照走的是另一条路（年/周表），结论同样是两个方向都出得来。
        let snapshot = Game::of("25w15a", None);
        assert!(inspect(&[ready(">=1.21.6-")], LoaderKind::Fabric, &snapshot, None).is_empty());
        assert_eq!(
            inspect(&[ready(">=1.21.6")], LoaderKind::Fabric, &snapshot, None)[0].kind,
            kind::WRONG_GAME_VERSION
        );
    }

    /// 一个模组可以写好几段区间，满足其中一段就算适配。
    ///
    /// LambDynamicLights 在 `fabric.mod.json` 里写的是两段：
    /// `["~1.21.5- <1.21.6-", "=1.21.6-alpha.25.14.craftmine"]`——第一段是
    /// 1.21.5 那一支，第二段单独把愚人节快照加回来。两段之间是「或」，拉平成
    /// 一条按「与」去解，得出的是「一个版本都不满足」，于是这个明明为这个快照
    /// 出过构建的模组，在这个快照上被报成可能不适配。
    #[test]
    fn a_mod_that_lists_several_ranges_only_has_to_match_one() {
        let craftmine = Game::of("25w14craftmine", Some("1.21.6"));
        assert_eq!(
            craftmine.semantic.as_deref(),
            Some("1.21.6-alpha.25.14.craftmine")
        );
        let lambda = needs(
            jar("LambDynamicLights", "lambdynlights", LoaderKind::Fabric),
            "minecraft",
            "~1.21.5- <1.21.6- || =1.21.6-alpha.25.14.craftmine",
        );
        assert!(inspect(&[lambda], LoaderKind::Fabric, &craftmine, None).is_empty());

        // 两段都不沾的版本照报，这条检查没有被架空。
        let lambda = needs(
            jar("LambDynamicLights", "lambdynlights", LoaderKind::Fabric),
            "minecraft",
            "~1.21.5- <1.21.6- || =1.21.6-alpha.25.14.craftmine",
        );
        let findings = inspect(
            &[lambda],
            LoaderKind::Fabric,
            &Game::of("1.20.1", None),
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::WRONG_GAME_VERSION);
    }

    /// 认不出来的版本仍然不比：那时说什么都是猜的。
    #[test]
    fn a_version_we_cannot_place_reports_nothing() {
        let sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "minecraft",
            ">=1.21.6",
        );
        let unknown = Game::of("我的整合包", None);
        assert!(unknown.semantic.is_none());
        assert!(inspect(&[sodium], LoaderKind::Fabric, &unknown, None).is_empty());
    }

    /// Forge 那边比的是版本号原样，不是 loader 归一化过的那个。
    ///
    /// FML 把 minecraft 当成一个版本号就是 `1.21.1` 的内置模组，`versionRange`
    /// 直接拿它比。把 Fabric 的写法套上去，`[1.21.6,)` 会对上
    /// `1.21.6-alpha.25.15.a`——语义化版本号里快照比正式版旧，于是整屋子模组
    /// 又全成了不兼容。
    #[test]
    fn the_forge_side_compares_the_plain_version_number() {
        let applied = needs(
            jar("AE2", "ae2", LoaderKind::NeoForge),
            "minecraft",
            "[1.21.6,1.22)",
        );

        // 正式版照常比，两边都比得对。
        assert!(
            inspect(
                std::slice::from_ref(&applied),
                LoaderKind::NeoForge,
                &Game::of("1.21.6", None),
                None
            )
            .is_empty()
        );
        let findings = inspect(
            std::slice::from_ref(&applied),
            LoaderKind::NeoForge,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings[0].kind, kind::WRONG_GAME_VERSION);

        // 快照上不比：这两个加载器只为正式版出构建，落到这里的是我们不认识的
        // 东西，而 Maven 区间容不下一个快照 id。
        assert!(
            inspect(
                &[applied],
                LoaderKind::NeoForge,
                &Game::of("25w15a", Some("1.21.6")),
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn two_copies_of_one_mod_are_reported_once() {
        let mut second = jar("Sodium", "sodium", LoaderKind::Fabric);
        second.file_name = "sodium-old.jar".to_owned();
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric), second],
            LoaderKind::Fabric,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::DUPLICATE);
        assert_eq!(findings[0].args["count"], "2");
    }

    #[test]
    fn a_fabric_mod_in_a_forge_instance_is_reported() {
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric)],
            LoaderKind::NeoForge,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::WRONG_LOADER);
        assert_eq!(findings[0].args["instanceLoader"], "neoforge");
        // Quilt 读得了 Fabric 的模组，那一种不该报。
        assert!(
            inspect(
                &[jar("Sodium", "sodium", LoaderKind::Fabric)],
                LoaderKind::Quilt,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );
    }

    /// 「装了它才起不来」不是「缺了它起不来」。
    ///
    /// AllTheLeaks 那样的修补型模组会列出一串 `type = "incompatible"`，说的是
    /// 那些模组的老版本和它装在一起会崩。读成必需依赖，预检查就会对着一个
    /// 一切正常的整合包报出一串「缺少 ambientsounds」——而用户照着去装，装回来
    /// 的正好是那个会崩的东西。
    #[test]
    fn an_incompatibility_is_never_a_missing_dependency() {
        let leaks = related(
            jar("All The Leaks", "alltheleaks", LoaderKind::NeoForge),
            0,
            "ambientsounds",
            "(,6.1.0]",
            Relation::Incompatible,
        );
        let leaks = related(leaks, 0, "occultism", "*", Relation::Discouraged);
        let leaks = related(leaks, 0, "jei", "*", Relation::Optional);
        assert!(
            inspect(
                &[leaks],
                LoaderKind::NeoForge,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );
    }

    /// 装了会起不来的那一条，现在会说出口。
    ///
    /// 只在三件事都成立时说：那个模组确实装着、版本号读得出来、而且那个版本确定
    /// 落在点名的区间里。
    #[test]
    fn an_incompatibility_is_reported_when_the_other_mod_is_actually_there() {
        let leaks = || {
            related(
                jar("All The Leaks", "alltheleaks", LoaderKind::NeoForge),
                0,
                "ambientsounds",
                "(,6.1.0]",
                Relation::Incompatible,
            )
        };
        let ambient = |version: &str| {
            let mut jar = jar("AmbientSounds", "ambientsounds", LoaderKind::NeoForge);
            jar.version = Some(version.to_owned());
            jar
        };
        let look = |jars: Vec<ModJar>| {
            inspect(&jars, LoaderKind::NeoForge, &Game::of("1.21.1", None), None)
        };

        // 装的那一份落在区间里：这是加载器真的会拒绝启动的组合。
        let findings = look(vec![leaks(), ambient("6.0.9")]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::INCOMPATIBLE);
        assert_eq!(findings[0].severity, Severity::Blocking);
        assert_eq!(findings[0].args["mod"], "All The Leaks");
        assert_eq!(findings[0].args["other"], "AmbientSounds");
        assert_eq!(findings[0].args["version"], "6.0.9");
        assert_eq!(findings[0].args["range"], "(,6.1.0]");
        // 删哪一个、还是把另一个升上去，这里判断不了，所以不给按钮。
        assert!(findings[0].action.is_none());

        // 已经升上去了就没有话说——这才是绝大多数整合包的样子。
        assert!(look(vec![leaks(), ambient("6.2.0")]).is_empty());
        // 根本没装那个模组，更没有话说。
        assert!(look(vec![leaks()]).is_empty());
        // 版本号读不出来，判断不了它落不落在区间里，不猜。
        let mut nameless = ambient("6.0.9");
        nameless.version = None;
        assert!(look(vec![leaks(), nameless]).is_empty());
    }

    /// 区间看不懂时不许报，这一条和别处的兜底方向正好相反。
    #[test]
    fn an_unreadable_range_never_becomes_a_conflict() {
        let mut ambient = jar("AmbientSounds", "ambientsounds", LoaderKind::NeoForge);
        ambient.version = Some("6.0.9".to_owned());
        let leaks = related(
            jar("All The Leaks", "alltheleaks", LoaderKind::NeoForge),
            0,
            "ambientsounds",
            "谁知道这是什么",
            Relation::Incompatible,
        );
        assert!(
            inspect(
                &[leaks, ambient],
                LoaderKind::NeoForge,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );
    }

    /// NeoForge 20.5 之前读的就是 Forge 那份 `META-INF/mods.toml`。
    ///
    /// 1.20.1 的 NeoForge 整合包里，每一个模组带的都是 `mods.toml`——那个年代
    /// 两家的清单一模一样。按「NeoForge 读不了 Forge 的」一刀切，那一屋子模组
    /// 会整整齐齐地全被判出局，而它们全都在正常工作。
    #[test]
    fn neoforge_read_the_forge_manifest_until_it_renamed_the_file() {
        let old_mod = || jar("JEI", "jei", LoaderKind::Forge);
        for version in ["1.20.1", "1.20.4"] {
            assert!(
                inspect(
                    &[old_mod()],
                    LoaderKind::NeoForge,
                    &Game::of(version, None),
                    None
                )
                .is_empty(),
                "{version} 上 NeoForge 读的就是 mods.toml"
            );
        }
        // 20.5 起只认 neoforge.mods.toml，那之后这一条才重新算数。
        for version in ["1.20.5", "1.21.1"] {
            let findings = inspect(
                &[old_mod()],
                LoaderKind::NeoForge,
                &Game::of(version, None),
                None,
            );
            assert_eq!(findings.len(), 1, "{version}");
            assert_eq!(findings[0].kind, kind::WRONG_LOADER, "{version}");
        }
        // 反过来永远不成立：Forge 从来没读过 neoforge.mods.toml。
        let findings = inspect(
            &[jar("JEI", "jei", LoaderKind::NeoForge)],
            LoaderKind::Forge,
            &Game::of("1.20.1", None),
            None,
        );
        assert_eq!(findings[0].kind, kind::WRONG_LOADER);
    }

    /// 一个文件通吃三家的模组，装在哪个实例里都不该被说三道四。
    ///
    /// Explorify 就是这样一份 jar：它自己的 `fabric.mod.json` 要 `fabric-api`，
    /// 它自己的 `neoforge.mods.toml` 只要 `minecraft`。一度我们读到第一份就
    /// 收工，于是一个 NeoForge 整合包里冒出两条拦路的红字——「它是 Fabric 的
    /// 模组」和「缺 Fabric API」——而游戏点下去照常起来，因为 NeoForge 读的
    /// 从来是另一份。
    #[test]
    fn a_jar_that_carries_several_loaders_manifests_fits_any_of_them() {
        let explorify = || {
            let mut jar = jar("Explorify", "explorify", LoaderKind::Fabric);
            jar.manifests.push(Manifest {
                loader: LoaderKind::NeoForge,
                depends: Vec::new(),
            });
            // 各家写各家的：Fabric 那份要 Fabric API，NeoForge 那份不要。
            declares(
                declares(jar, 0, "fabric-api", "*"),
                1,
                "minecraft",
                "[1.20,)",
            )
        };

        // NeoForge 实例读的是那份只要 minecraft 的清单，没有话说。
        assert!(
            inspect(
                &[explorify()],
                LoaderKind::NeoForge,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );

        // 同一个文件装进 Fabric 实例，那条 fabric-api 才真的算数——依赖是跟着
        // 清单走的，不是跟着文件走的。Quilt 读的也是这一份。
        for loader in [LoaderKind::Fabric, LoaderKind::Quilt] {
            let findings = inspect(&[explorify()], loader, &Game::of("1.21.1", None), None);
            assert_eq!(findings.len(), 1, "{loader:?}");
            assert_eq!(findings[0].kind, kind::MISSING_DEPENDENCY, "{loader:?}");
            assert_eq!(findings[0].args["dependency"], "fabric-api", "{loader:?}");
        }

        // 一家都对不上时照报，而且把它写着的那几家都说出来。只报这一条：
        // 它不会被加载，那份 Fabric 清单里的 fabric-api 不该跟着冒出来。
        let findings = inspect(
            &[explorify()],
            LoaderKind::Forge,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::WRONG_LOADER);
        assert_eq!(findings[0].args["modLoader"], "fabric,neoforge");
    }

    #[test]
    fn a_mod_built_for_another_game_version_is_a_warning_not_a_blocker() {
        let sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "minecraft",
            "1.20.x",
        );
        let findings = inspect(
            &[sodium],
            LoaderKind::Fabric,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    /// 看不懂的区间一律当作满足。宁可漏报。
    #[test]
    fn an_unreadable_version_range_is_not_reported() {
        let sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "minecraft",
            "谁知道这是什么",
        );
        assert!(
            inspect(
                &[sodium],
                LoaderKind::Fabric,
                &Game::of("1.21.1", None),
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn mods_in_a_vanilla_instance_are_never_loaded() {
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric)],
            LoaderKind::Vanilla,
            &Game::of("1.21.1", None),
            None,
        );
        assert_eq!(findings[0].kind, kind::NO_LOADER);
    }

    /// Fabric API 的模块不是单独的 jar，它们在 Fabric API 里面。装了 Fabric
    /// API 却被告知缺 `fabric-block-getter-api-v2`，用户是修不了的——那个名字
    /// 在任何一个模组站上都搜不到。
    #[test]
    fn the_modules_inside_fabric_api_count_as_installed() {
        let mut api = jar("Fabric API", "fabric-api", LoaderKind::Fabric);
        api.provides = vec![
            "fabric-block-getter-api-v2".to_owned(),
            "fabric-rendering-v1".to_owned(),
        ];
        let jars = vec![
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "fabric-rendering-v1",
                "*",
            ),
            api,
        ];
        assert!(inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None).is_empty());
    }

    /// 没装的时候要说的是「装 Fabric API」，而且只说一次——十个模块缺了，
    /// 要做的仍然只有一件事。
    #[test]
    fn missing_fabric_api_modules_are_reported_as_fabric_api() {
        let mut sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "fabric-rendering-v1",
            "*",
        );
        sodium = needs(sodium, "fabric-block-getter-api-v2", "*");
        sodium = needs(sodium, "fabric-api-base", "*");
        // 名字里带 fabric- 的独立模组不该被并进去。
        sodium = needs(sodium, "fabric-language-kotlin", "*");

        let findings = inspect(
            &[sodium],
            LoaderKind::Fabric,
            &Game::of("1.21.1", None),
            None,
        );
        let missing: Vec<&str> = findings
            .iter()
            .map(|finding| finding.args["dependency"].as_str())
            .collect();
        assert_eq!(missing, vec!["fabric-api", "fabric-language-kotlin"]);
        assert_eq!(
            findings[0].action,
            Some(Action::InstallMod {
                query: "fabric-api".to_owned()
            })
        );
    }

    #[test]
    fn a_healthy_instance_says_nothing() {
        let jars = vec![
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "fabric-api",
                "*",
            ),
            jar("Fabric API", "fabric-api", LoaderKind::Fabric),
        ];
        assert!(inspect(&jars, LoaderKind::Fabric, &Game::of("1.21.1", None), None).is_empty());
    }
}
