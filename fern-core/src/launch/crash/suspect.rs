//! 栈里最上面那个非原版的帧来自哪个模组。
//!
//! **这一层和规则表无关。** 一次崩溃可能一条规则都不命中，但栈几乎总在，而
//! 「崩在 Sodium 0.6.0 的代码里」本身就是有用的一句话——上一版那句「认不出
//! 原因」因此基本消失。
//!
//! 判据是包名前缀。原版的类名会随版本混淆（1.16 那会儿是 `bao.a()`），但
//! **模组自己的包名从来不混淆**——而要归因的正是模组，所以混淆对这一层不构成
//! 问题。
//!
//! 三条证据，从确凿到间接：
//!
//! 1. **加载器自己点名的**（Forge 的 `-- MOD <modid> --` 段）。这是最硬的一条，
//!    不用翻栈，也不要求本地装着那个 jar。
//! 2. **失败的 mixin 配置**（`sodium.mixins.json`），前缀就是 modid。
//! 3. **栈帧的包名**落在某个已装模组的包里。

use serde::{Deserialize, Serialize};

use super::parse::Facts;

/// 一定不是模组的包。栈顶几乎全是这些，跳过它们才找得到有意义的那一帧。
const NOT_A_MOD: [&str; 10] = [
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "com.sun.",
    "net.minecraft.",
    "com.mojang.",
    "org.spongepowered.asm.", // mixin 自己的转发帧
    "net.fabricmc.loader.",
    "cpw.mods.",
];

/// 一个可能有关的模组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suspect {
    pub mod_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 它在栈里第几帧出现。越小越可疑。
    pub depth: usize,
    /// 凭什么怀疑它。
    pub reason: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// 加载器自己在崩溃报告里点了它的名。
    Declared,
    /// 失败的 mixin 配置是它的。
    Mixin,
    /// 栈帧的包名落在它的包里。
    Stack,
}

/// 这个模组自报的包前缀。
///
/// 由调用方提供：`instance/mods.rs` 读 jar 的时候顺手扫出来，这一层不碰磁盘，
/// 于是它是纯函数，能对着一段文本单独测。
pub struct Known {
    pub mod_id: String,
    pub name: String,
    pub version: Option<String>,
    /// 这个 jar 里的顶层包，例如 `net.caffeinemc.mods.sodium`。
    pub packages: Vec<String>,
    /// 它还替哪些 id 作数（打包在它里面的那些模块）。失败的 mixin 配置报的是
    /// 模块名——`fabric-rendering-v1.mixins.json` 要能指回 Fabric API。
    pub provides: Vec<String>,
}

impl Known {
    fn answers_to(&self, name: &str) -> bool {
        // Mixin 配置名里的 `-` 有时写成 `_`，两种都认。
        let matches = |id: &String| id == name || id.replace('-', "_") == name;
        matches(&self.mod_id) || self.provides.iter().any(matches)
    }
}

/// 按可疑程度排好序。空表示栈里没有一帧落在已知的模组上。
pub fn identify(facts: &Facts, known: &[Known]) -> Vec<Suspect> {
    let mut suspects: Vec<Suspect> = Vec::new();

    // 加载器自己点的名最硬，排在最前面。本地没装那个 jar 也照样成立——分析
    // 别人贴过来的日志时只有这一条能用。
    for failed in &facts.failed_mods {
        let entry = known.iter().find(|entry| entry.mod_id == failed.mod_id);
        suspects.push(Suspect {
            mod_id: failed.mod_id.clone(),
            name: entry.map_or_else(|| failed.mod_id.clone(), |entry| entry.name.clone()),
            version: entry.and_then(|entry| entry.version.clone()),
            depth: 0,
            reason: Reason::Declared,
        });
    }

    // 再看 mixin：它说得出名字，不用猜。
    for throwable in &facts.chain {
        let Some(message) = &throwable.message else {
            continue;
        };
        for config in mixin_configs(message) {
            if let Some(entry) = known.iter().find(|entry| entry.answers_to(&config)) {
                push(&mut suspects, entry, 0, Reason::Mixin);
            }
        }
    }

    // 再翻栈。根因那一条，从上往下。
    if let Some(root) = facts.root() {
        for (depth, frame) in root.frames.iter().enumerate() {
            if NOT_A_MOD
                .iter()
                .any(|prefix| frame.class.starts_with(prefix))
            {
                continue;
            }
            if let Some(entry) = known.iter().find(|entry| {
                entry
                    .packages
                    .iter()
                    .any(|package| frame.class.starts_with(&format!("{package}.")))
            }) {
                push(&mut suspects, entry, depth, Reason::Stack);
            }
        }
    }

    // 同一个模组只留一条，理由取最硬的那个（枚举的顺序就是硬度）。
    suspects.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    suspects.dedup_by(|later, earlier| later.mod_id == earlier.mod_id);
    suspects
}

fn push(suspects: &mut Vec<Suspect>, entry: &Known, depth: usize, reason: Reason) {
    if let Some(existing) = suspects
        .iter_mut()
        .find(|suspect| suspect.mod_id == entry.mod_id)
    {
        existing.depth = existing.depth.min(depth);
        return;
    }
    suspects.push(Suspect {
        mod_id: entry.mod_id.clone(),
        name: entry.name.clone(),
        version: entry.version.clone(),
        depth,
        reason,
    });
}

/// 消息里提到的 mixin 配置名，去掉 `.mixins.json`。
fn mixin_configs(message: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (index, _) in message.match_indices(".mixins.json") {
        let head = &message[..index];
        let start = head
            .rfind(|character: char| {
                !character.is_alphanumeric() && character != '_' && character != '-'
            })
            .map(|offset| offset + 1)
            .unwrap_or(0);
        let name = &head[start..];
        if !name.is_empty() && !found.iter().any(|existing| existing == name) {
            found.push(name.to_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::crash::parse::{Evidence, extract};

    fn sodium() -> Known {
        Known {
            mod_id: "sodium".to_owned(),
            name: "Sodium".to_owned(),
            version: Some("0.6.0".to_owned()),
            packages: vec!["net.caffeinemc.mods.sodium".to_owned()],
            provides: Vec::new(),
        }
    }

    /// Fabric API 里的模块崩了，报出来的是模块名；能指认的只有 Fabric API。
    fn fabric_api() -> Known {
        Known {
            mod_id: "fabric-api".to_owned(),
            name: "Fabric API".to_owned(),
            version: Some("0.100.0".to_owned()),
            packages: vec!["net.fabricmc.fabric".to_owned()],
            provides: vec!["fabric-rendering-v1".to_owned()],
        }
    }

    #[test]
    fn a_module_inside_a_mod_is_attributed_to_the_mod_that_ships_it() {
        let facts = extract(&Evidence {
            report: None,
            console: "org.spongepowered.asm.mixin.injection.throwables.InjectionError: \
                      Critical injection failure in fabric-rendering-v1.mixins.json\n",
            hs_err: None,
        });
        let suspects = identify(&facts, &[fabric_api()]);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].mod_id, "fabric-api");
        assert_eq!(suspects[0].reason, Reason::Mixin);
    }

    #[test]
    fn the_topmost_frame_that_is_not_vanilla_names_the_mod() {
        let facts = extract(&Evidence {
            report: None,
            console: "java.lang.NullPointerException: boom\n\
                 \tat net.minecraft.client.renderer.LevelRenderer.render(LevelRenderer.java:1)\n\
                 \tat net.caffeinemc.mods.sodium.client.render.SodiumWorldRenderer.draw(SodiumWorldRenderer.java:2)\n",
            hs_err: None,
        });
        let suspects = identify(&facts, &[sodium()]);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].mod_id, "sodium");
        assert_eq!(suspects[0].reason, Reason::Stack);
        // 原版那一帧在最上面，但它不该被算成嫌疑。
        assert_eq!(suspects[0].depth, 1);
    }

    #[test]
    fn a_failing_mixin_names_its_owner_without_reading_the_stack() {
        let facts = extract(&Evidence {
            report: None,
            console: "org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: \
                 @Inject could not find any targets [PREINJECT -> sodium.mixins.json:MixinLevelRenderer]\n",
            hs_err: None,
        });
        let suspects = identify(&facts, &[sodium()]);
        assert_eq!(suspects[0].mod_id, "sodium");
        assert_eq!(suspects[0].reason, Reason::Mixin);
    }

    /// 加载器自己点了名，本地有没有装那个 jar 都不影响。
    #[test]
    fn the_loader_naming_a_mod_is_enough_on_its_own() {
        let report = "---- Minecraft Crash Report ----\n\
             Description: Mod loading error has occurred\n\n\
             -- MOD sodium --\n\
             Details:\n\
             \tMod File: sodium-0.6.0.jar\n\
             \tFailure message: Sodium (sodium) has failed to load correctly\n";
        let facts = extract(&Evidence {
            report: Some(report),
            console: "",
            hs_err: None,
        });
        assert_eq!(facts.failed_mods[0].mod_id, "sodium");

        // 本地装着它时用它的展示名和版本，没装就退回 modid。
        let named = identify(&facts, &[sodium()]);
        assert_eq!(named[0].name, "Sodium");
        assert_eq!(named[0].reason, Reason::Declared);
        assert_eq!(identify(&facts, &[])[0].name, "sodium");
    }

    #[test]
    fn a_pure_vanilla_stack_accuses_nobody() {
        // 认不出来就什么都不说。指一个无辜的模组比不指更糟。
        let facts = extract(&Evidence {
            report: None,
            console: "java.lang.NullPointerException\n\tat net.minecraft.client.Minecraft.run(Minecraft.java:1)\n",
            hs_err: None,
        });
        assert!(identify(&facts, &[sodium()]).is_empty());
    }
}
