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
    let mut hits: BTreeMap<String, usize> = BTreeMap::new();
    // 根因异常类 → 几份没命中的，以及其中几个文件名做样本。
    let mut misses: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();

    for entry in walk(Path::new(&directory)) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        total += 1;
        // 语料里既有崩溃报告也有控制台日志，一律当控制台喂进去——规则的 scope
        // 守卫会自己挑。加载器和版本未知，所以只跑不带守卫的那些规则。
        let found = fern_core::diagnose_crash(&text, Default::default());
        if let Some(first) = found.first() {
            *hits.entry(first.id.clone()).or_default() += 1;
            continue;
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
