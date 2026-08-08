//! 规则表在一堆真实崩溃报告上认出了多少。
//!
//! ```bash
//! cargo run -p fern-core --example crash_coverage -- corpus/
//! ```
//!
//! 这是写规则的工作方式，不是一次性的脚本。不看语料写正则等于猜那行字长什么
//! 样；有几百份真报告之后这件事变成可测量的：**看最大的那一簇没命中的，读三份，
//! 写一条规则，再跑一遍看覆盖率涨了多少。**
//!
//! 没命中的按**根因异常类**聚类。解析器本来就把它拆出来了，所以聚类不要钱，
//! 而它直接回答「下一条规则该写什么、能覆盖多少份」。
//!
//! 语料自己不进仓库（几百份别人的日志没必要压进 git，而且里面有玩家名、路径、
//! 有时还有 access token）。进仓库的只有**支撑某一条规则的那一份**，洗干净之后
//! 放进 `rules/fixtures/`。抓取与脱敏见 `.github/collect-crashes.py`。

use std::{collections::BTreeMap, path::Path};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(directory) = arguments.next() else {
        eprintln!("用法：crash_coverage <目录>");
        std::process::exit(2);
    };

    let mut total = 0usize;
    // 认不出原因、但仍然说得出「崩在谁那里」的那些。这才是归因的增量。
    let mut named_without_rule = 0usize;
    let mut mod_frame_without_rule = 0usize;
    let mut hits: BTreeMap<String, usize> = BTreeMap::new();
    // 根因异常类 → 几份没命中的，以及其中几个文件名做样本。
    let mut misses: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();

    for entry in walk(Path::new(&directory)) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        total += 1;
        // 语料里既有崩溃报告也有控制台日志，一律当控制台喂进去——规则的 scope
        // 守卫会自己挑。
        //
        // 加载器和版本要从报告里认出来：不认的话 `Default` 是原版，带 loader
        // 守卫的规则一条都不会被考虑，覆盖率会被系统性地低估。
        let found = fern_core::diagnose_crash(&text, context_of(&text));
        if let Some(first) = found.first() {
            *hits.entry(first.id.clone()).or_default() += 1;
            continue;
        }
        // 归因和规则无关，所以要单独数：认不出原因、但说得出崩在谁那里，对
        // 用户仍然是有用的一句话。
        if !fern_core::attribute_crash(&text).is_empty() {
            named_without_rule += 1;
        } else if has_mod_frame(&text) {
            // 栈里有非原版的帧。语料里没有对应的 jar，所以这里只是上界——真实
            // 场景下那些 jar 就在本地，能落到具体某个模组上。
            mod_frame_without_rule += 1;
        }
        let cluster = root_exception(&text).unwrap_or_else(|| "（没有异常行）".to_owned());
        let record = misses.entry(cluster).or_default();
        record.0 += 1;
        if record.1.len() < 3 {
            record.1.push(entry.display().to_string());
        }
    }

    let matched: usize = hits.values().sum();
    println!(
        "{total} 份报告 · 命中 {matched}（{:.0}%）\n",
        if total == 0 {
            0.0
        } else {
            matched as f64 * 100.0 / total as f64
        }
    );
    let mut ranked: Vec<_> = hits.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (id, count) in ranked {
        println!("  {count:>5}  {id}");
    }

    if misses.is_empty() {
        return;
    }
    println!(
        "\n认不出原因的那些里：\n  {named_without_rule:>5}  报告里已经点了名，直接说得出是谁\n  {mod_frame_without_rule:>5}  栈里有非原版的帧，本地装着那个 jar 时能落到具体模组"
    );
    println!("\n未命中 {}，按根因异常聚类：", total - matched);
    let mut clusters: Vec<_> = misses.into_iter().collect();
    clusters.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
    for (exception, (count, samples)) in clusters.into_iter().take(25) {
        println!("  {count:>5}  {exception}");
        for sample in samples {
            println!("            {sample}");
        }
    }
}

/// 栈里有没有一帧不属于原版、JDK 或加载器自己。
///
/// 有的话，本地装着那些 jar 时就能归因到具体模组；语料里没有 jar，所以这只是
/// 一个上界。
fn has_mod_frame(text: &str) -> bool {
    const NOT_A_MOD: [&str; 10] = [
        "java.",
        "javax.",
        "jdk.",
        "sun.",
        "com.sun.",
        "net.minecraft.",
        "com.mojang.",
        "org.spongepowered.asm.",
        "net.fabricmc.loader.",
        "cpw.mods.",
    ];
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("at "))
        .filter_map(|frame| frame.split('(').next())
        .any(|class| {
            class.contains('.') && !NOT_A_MOD.iter().any(|prefix| class.starts_with(prefix))
        })
}

/// 从报告里认出加载器和游戏版本。
///
/// 崩溃报告自己带着这两样：`Minecraft Version:` 是一行键值，而模组表的**写法**
/// 就说明了加载器——Fabric 写 `Fabric Mods:`，Forge 一系写 `Mod List:` 的竖线表。
fn context_of(text: &str) -> fern_core::CrashContext {
    let minecraft = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Minecraft Version:"))
        .map(|value| value.trim().to_owned())
        .unwrap_or_default();
    let loader = if text.contains("Fabric Mods:") || text.contains("net.fabricmc.loader") {
        fern_core::LoaderKind::Fabric
    } else if text.contains("neoforge") || text.contains("net.neoforged") {
        fern_core::LoaderKind::NeoForge
    } else if text.contains("Mod List:") || text.contains("net.minecraftforge") {
        fern_core::LoaderKind::Forge
    } else {
        fern_core::LoaderKind::Vanilla
    };
    fern_core::CrashContext { loader, minecraft }
}

/// 最后一个 `Caused by:`，没有就是第一条异常。归因和聚类看的都是它。
fn root_exception(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        let candidate = line.strip_prefix("Caused by: ").unwrap_or(line);
        let Some(head) = candidate.split_whitespace().next() else {
            continue;
        };
        let head = head.trim_end_matches(':');
        let looks_like_throwable = head.contains('.')
            && (head.ends_with("Exception")
                || head.ends_with("Error")
                || head.ends_with("Throwable"));
        if looks_like_throwable && (line.starts_with("Caused by: ") || found.is_none()) {
            found = Some(head.to_owned());
        }
    }
    found
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "txt" || extension == "log")
        {
            files.push(path);
        }
    }
    files
}
