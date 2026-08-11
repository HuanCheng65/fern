//! 产物变换：在安装期做出一份打好补丁的 jar，启动时用它替掉原件。
//!
//! ## 为什么是安装期，不是运行时 agent
//!
//! 决定性的一条是**失败模式**。`ClassFileTransformer` 抛出的异常会被 JVM 吞
//! 掉，然后用原始字节继续加载——补丁没打上的时候，现象和根本没有补丁完全一
//! 样，日志里没有任何线索。这里改写失败则是一个普通的错误，带得上「哪个
//! jar、哪个方法、为什么拒绝」，而且游戏根本不会被拉起来。
//!
//! 其余理由：产物有 hash，能 `javap`、能 diff、能写测试断言改写结果；每个
//! 实例只做一次而不是每次启动都做一次；不往命令行里塞 `-javaagent`（它会进
//! 崩溃报告，还要和 authlib-injector 的 agent 排先后）；jar mod 本来就只能走
//! 安装期，不必维护两套补丁机制。
//!
//! ## 四条规矩（文档 §3.4）
//!
//! 1. **原件永不覆盖**，产物另存在 `patched/` 下——否则文件完整性校验会把它
//!    判成损坏的下载。
//! 2. **剥签名**，见 [`jar`]。
//! 3. **缓存 key = 原 jar 的 sha1 + 补丁 id + 补丁版本**，任一变化就重做。
//! 4. **产物登记进自己的清单**（产物旁边那份 `.fern-patch.json`），下次启动
//!    据此确认磁盘上那份还是我们做出来的那一份。

pub(crate) mod bytecode;
mod jar;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fern_meta::RuleContext;
use serde::{Deserialize, Serialize};

use crate::DataPaths;

/// 一条补丁。
///
/// `coordinate` 只是**候选**的筛子，真正决定打不打的是产物内容：`rewrite`
/// 一条都没换上就说明这个 jar 不需要它（上游已经修过了）。版本号不参与判
/// 断——「哪些版本有这个毛病」是一份查完就过期的清单，而那一句字节码在不在
/// 是当场看得见的事实。
pub(crate) struct Patch {
    pub id: &'static str,
    /// 补丁本身改了就加一，缓存据此作废。
    pub version: u32,
    /// `group:artifact`，不含版本。
    pub coordinate: &'static str,
    /// 拿到条目名和内容，返回 `Some(新内容)` 就换掉。
    pub rewrite: Rewrite,
}

/// 一条补丁改写 jar 里某个条目的那个函数。
///
/// 返回 `None` 有两种意思，都表示「这一条不归我管」：条目名对不上，或者内容
/// 里根本没有要改的东西（上游那一版已经是好的）。
pub(crate) type Rewrite = fn(&str, &[u8]) -> Result<Option<Vec<u8>>>;

/// 全部补丁。今天只有一件事，写成两条是因为 1.6.4 那一代的坐标不叫 forge。
pub(crate) const PATCHES: &[Patch] = &[
    FML_SORT_TWEAKS,
    Patch {
        coordinate: "net.minecraftforge:minecraftforge",
        ..FML_SORT_TWEAKS
    },
];

/// 老 FML 在 Java 8u20 之后必崩的那一句。
///
/// FML 7.x / 9.x 的 `CoreModManager.sortTweakList()` 直接对 LaunchWrapper 正
/// 拿着迭代器遍历的那张 tweaker 表做原地排序：
///
/// ```text
/// Collections.sort(list, cmp)
/// ```
///
/// `Collections.sort` 从 Java 8u20 起委托给 `List.sort`，原地排会动
/// `modCount`，于是 LaunchWrapper 下一句 `it.remove()` 抛
/// `ConcurrentModificationException`——退出码 1，游戏日志一行都没有。Forge
/// 自己在 1.7.10 改成了拷一份排完再写回，1.7.9 及更早再没有发过版，所以这个
/// 坑只能由启动器来填。换上去的是同一件事的另一种写法，不动 `modCount`：
///
/// ```text
/// Object[] a = list.toArray();
/// Arrays.sort(a, cmp);
/// Collections.copy(list, Arrays.asList(a));
/// ```
///
/// 同一批版本还有第二个坑（FML 的防篡改检查），但那一条不在这里：它是
/// 「LaunchWrapper 时代 + 任何 jar 改动」的通用前提，由兼容规则表统一给出，
/// 见 `rules/compat.toml` 的 `old-fml-refuses-an-unsigned-client`。
const FML_SORT_TWEAKS: Patch = Patch {
    id: "fml-sort-tweaks",
    version: 1,
    coordinate: "net.minecraftforge:forge",
    rewrite: rewrite_core_mod_manager,
};

fn rewrite_core_mod_manager(name: &str, bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    // 两个包名都要认：1.7.x 是 cpw.mods.fml，1.8 之后搬到了
    // net.minecraftforge.fml。
    if !name.ends_with("/relauncher/CoreModManager.class") {
        return Ok(None);
    }
    bytecode::replace_call(
        bytes,
        "sortTweakList",
        &bytecode::MethodRef {
            owner: "java/util/Collections",
            name: "sort",
            descriptor: "(Ljava/util/List;Ljava/util/Comparator;)V",
            interface: false,
        },
        bytecode::copy_sort_copy_back,
    )
    .with_context(|| format!("改写 {name}"))
}

/// 产物旁边那份登记。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    patch: String,
    patch_version: u32,
    source: PathBuf,
    source_sha1: String,
    output_sha1: String,
}

/// 补丁产物放在哪。
///
/// 在 Fern 自己的数据根下，**不在** `libraries/` 里：那里的每个文件都对应上
/// 游的一个 sha1，混进一份我们自己造的会让完整性检查无从判断。外部实例同理
/// ——`DataPaths::scoped` 保证 `root` 始终是我们的地盘，绝不往别人的目录里
/// 写东西。
fn root(paths: &DataPaths) -> PathBuf {
    paths.root.join("patched")
}

/// 需要的话产出补丁产物，不需要就返回原件。
///
/// `allowed` 是兼容规则点名要打的那些补丁 id（见 [`super::compat`]）——**表里
/// 有这条补丁不等于该打它**，什么时候打是规则说了算，能不能打才由产物内容
/// 说了算。
///
/// 补全和启动调的是同一个函数，所以两边看到的 classpath 一定一致。做过一遍
/// 之后它只是几次文件检查加一次 sha1。
pub(crate) fn applied(
    paths: &DataPaths,
    coordinate: &str,
    original: &Path,
    allowed: &[&str],
) -> Result<PathBuf> {
    let plain = || original.to_owned();
    let candidates: Vec<&Patch> = PATCHES
        .iter()
        .filter(|patch| allowed.contains(&patch.id))
        .filter(|patch| matches_coordinate(patch.coordinate, coordinate))
        .collect();
    if candidates.is_empty() || !original.is_file() {
        return Ok(plain());
    }

    let source = std::fs::read(original).with_context(|| format!("读取 {}", original.display()))?;
    let source_sha1 = fern_download::sha1_hex(&source);
    let mut current = plain();
    for patch in candidates {
        let output = output_path(paths, patch, &source_sha1, original);
        // 缓存里已经有一份，而且就是我们做出来的那一份。
        if reuse(&output, patch, &source_sha1)? {
            current = output;
            continue;
        }
        // 这个 jar 不需要这条补丁的话，rewrite 什么都不落盘——上游那一版已经
        // 是好的（1.7.10 起的 Forge 就是）。
        if !jar::rewrite(original, &output, patch.rewrite)
            .with_context(|| format!("为 {} 打补丁 {}", original.display(), patch.id))?
        {
            continue;
        }
        let produced = std::fs::read(&output)?;
        let record = Record {
            patch: patch.id.to_owned(),
            patch_version: patch.version,
            source: original.to_owned(),
            source_sha1: source_sha1.clone(),
            output_sha1: fern_download::sha1_hex(&produced),
        };
        std::fs::write(record_path(&output), serde_json::to_vec_pretty(&record)?)?;
        current = output;
    }
    Ok(current)
}

/// 把这个实例的 jar mod 叠进 client jar，产出另一份。没有 jar mod 就返回原件。
///
/// 1.6 之前的模组就是这么装的：把 class 覆盖进游戏本体。规矩有两条，缺一条
/// 游戏就起不来——**后叠的盖住先叠的**（层的顺序是有语义的），以及**叠完要
/// 删掉 `META-INF/`**：那里面是 Mojang 对每个条目的签名，class 一换就对不上，
/// JVM 会在加载第一个被改过的类时抛 `SecurityException`。剥签名这件事
/// [`jar::rewrite`] 本来就在做。
///
/// 产物和别的补丁产物摆在一起，缓存 key 是 client jar 的 sha1 加上每一份
/// jar mod 的 sha1——换掉其中任何一个都要重做。
pub(crate) fn with_jar_mods(
    paths: &DataPaths,
    profile: &crate::InstanceProfile,
    client_jar: &Path,
) -> Result<PathBuf> {
    let mods: Vec<&Path> = profile
        .components
        .iter()
        .flat_map(|component| component.jar_mods.iter())
        .map(PathBuf::as_path)
        .collect();
    if mods.is_empty() || !client_jar.is_file() {
        return Ok(client_jar.to_owned());
    }

    // 先把要叠的内容读进来，顺带算出缓存 key。
    let mut key = fern_download::sha1_hex(
        &std::fs::read(client_jar).with_context(|| format!("读取 {}", client_jar.display()))?,
    );
    let mut overlay: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for path in &mods {
        let bytes =
            std::fs::read(path).with_context(|| format!("读取 jar mod {}", path.display()))?;
        key.push('-');
        key.push_str(&fern_download::sha1_hex(&bytes));
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .with_context(|| format!("读取 jar mod {}", path.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            // jar mod 里那份 META-INF 同样不要：它是模组作者打包时留下的，
            // 盖到 client jar 上只会让签名更乱。
            if name.starts_with("META-INF/") {
                continue;
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut bytes)?;
            // 后叠的盖住先叠的。
            overlay.insert(name, bytes);
        }
    }

    let name = client_jar
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "client.jar".to_owned());
    let key = fern_download::sha1_hex(key.as_bytes());
    let output = root(paths).join("jar-mods").join(&key).join(name);
    if output.is_file() {
        return Ok(output);
    }

    jar::overlay(client_jar, &output, &overlay)?;
    Ok(output)
}

/// 补全时把该做的产物都做出来。
///
/// 启动那一刻再做也不会错（[`applied`] 是幂等的），但那是在用户按下按钮之后
/// ——几百毫秒的重打 jar 应该发生在「正在补全文件」里，而且改写失败要在那时
/// 就说出来。
pub(crate) fn prepare_all(
    paths: &DataPaths,
    metadata: &fern_meta::VersionMetadata,
    context: &RuleContext,
    allowed: &[&str],
) -> Result<()> {
    for library in metadata.effective_libraries(context) {
        let Some(file) = library.file(context) else {
            continue;
        };
        if file.native {
            continue;
        }
        let path = fern_download::safe_join(&paths.libraries, Path::new(&file.path))?;
        applied(paths, &library.name, &path, allowed)?;
    }
    Ok(())
}

/// 缓存里那一份还算不算数。
///
/// 判据是产物旁边的登记：补丁版本对得上、产物的 sha1 也对得上。少了任何一
/// 条就重做——一份改坏了的 jar 比没有补丁危险得多。
fn reuse(output: &Path, patch: &Patch, source_sha1: &str) -> Result<bool> {
    let Ok(bytes) = std::fs::read(record_path(output)) else {
        return Ok(false);
    };
    let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
        return Ok(false);
    };
    if record.patch != patch.id
        || record.patch_version != patch.version
        || record.source_sha1 != source_sha1
    {
        return Ok(false);
    }
    let Ok(produced) = std::fs::read(output) else {
        return Ok(false);
    };
    Ok(fern_download::sha1_hex(&produced) == record.output_sha1)
}

fn output_path(paths: &DataPaths, patch: &Patch, source_sha1: &str, original: &Path) -> PathBuf {
    let name = original
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact.jar".to_owned());
    root(paths)
        .join(patch.id)
        // 缓存 key 全在这一层目录名里：原 jar 的 sha1 + 补丁版本。
        .join(format!("{source_sha1}-{}", patch.version))
        .join(name)
}

fn record_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".fern-patch.json");
    output.with_file_name(name)
}

/// `net.minecraftforge:forge` 认不认 `net.minecraftforge:forge:1.7.2-10.12…`。
fn matches_coordinate(pattern: &str, coordinate: &str) -> bool {
    let mut parts = coordinate.split(':');
    let (Some(group), Some(artifact)) = (parts.next(), parts.next()) else {
        return false;
    };
    pattern == format!("{group}:{artifact}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_under(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    found.push(path);
                }
            }
        }
        found
    }

    #[test]
    fn a_pattern_matches_the_coordinate_regardless_of_version() {
        assert!(matches_coordinate(
            "net.minecraftforge:forge",
            "net.minecraftforge:forge:1.7.2-10.12.2.1161-mc172"
        ));
        assert!(matches_coordinate(
            "net.minecraftforge:minecraftforge",
            "net.minecraftforge:minecraftforge:9.11.1.1345"
        ));
        assert!(!matches_coordinate(
            "net.minecraftforge:forge",
            "net.minecraftforge:forgespi:7.0.1"
        ));
        assert!(!matches_coordinate("net.minecraftforge:forge", "无坐标"));
    }

    /// 不沾边的库连读都不该读一遍，更不该产出任何文件。
    #[test]
    fn a_library_no_patch_applies_to_is_returned_as_is() {
        let root = std::env::temp_dir().join(format!("fern-patch-plain-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        let jar = root.join("brigadier.jar");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(&jar, b"not a jar").expect("write");

        let chosen = applied(
            &paths,
            "com.mojang:brigadier:1.0.18",
            &jar,
            &["fml-sort-tweaks"],
        )
        .expect("applied");
        assert_eq!(chosen, jar);
        assert!(!super::root(&paths).exists());

        std::fs::remove_dir_all(root).expect("remove root");
    }

    /// 坐标对得上、内容里却没有那一句（上游已经修好的那些版本）：什么都不做，
    /// 也不能留下一个空产物。
    #[test]
    fn a_forge_jar_without_the_defect_is_left_alone() {
        use std::io::Write;

        let root = std::env::temp_dir().join(format!("fern-patch-clean-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        std::fs::create_dir_all(&root).expect("create root");
        let jar = root.join("forge.jar");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&jar).expect("create jar"));
        // 签名的，和真的 Forge universal jar 一样——剥签名本身不构成「改过」。
        for (name, bytes) in [
            ("META-INF/FORGE.SF", b"Signature-Version: 1.0\n" as &[u8]),
            ("cpw/mods/fml/Loader.class", b"whatever"),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start");
            writer.write_all(bytes).expect("write");
        }
        writer.finish().expect("finish");

        let chosen = applied(
            &paths,
            "net.minecraftforge:forge:1.7.10-10.13.4.1614",
            &jar,
            &["fml-sort-tweaks"],
        )
        .expect("applied");
        assert_eq!(chosen, jar);
        // 缓存目录里不该多出任何一个文件（空目录无所谓）。
        assert_eq!(files_under(&super::root(&paths)), Vec::<PathBuf>::new());

        std::fs::remove_dir_all(root).expect("remove root");
    }
}
