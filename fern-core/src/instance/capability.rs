//! 这个 jar 里的代码能做到什么。
//!
//! `class.rs` 把每个 class 引用到的方法和字符串列出来，这一层只做一件事：把那
//! 张长表收成几个名字。收得**很窄**——只留那些在正常模组里罕见、而在已经发生
//! 过的投毒事件里必然出现的几项。
//!
//! ## 为什么它不是一份「权限清单」
//!
//! 「这个模组会启动外部程序」单独摆出来没有意义：真有模组要这么做（开日志目
//! 录、调外部工具），而看到这行字的用户既不知道该不该允许，也没有别的选择。
//! 一份长期挂在界面上、绝大多数条目都无害的清单，唯一的效果是把用户训练成不
//! 再读它。
//!
//! 有意义的是**差**。fractureiser 那类东西的做法是往已经装好的 jar 里追加
//! class：文件名不变、模组声明的版本号不变，但那份代码里多出了原本没有的调
//! 用。所以这一层的产物从不单独示人，只在 `integrity` 发现一个文件被静默改写
//! 时，拿改动前后的两份清单相减——多出来的那几项才是要说的话。
//!
//! 相减要求改动**之前**那一份当时就扫过。清单按 sha1 存（内容定址，同一个
//! sha1 的内容永远是同一份，存下来就再也不会过期），基线由游戏退出后那一遍
//! 彻底对账建立，见 [`Known`] 与 `integrity::Depth::Full`。
//!
//! ## 它有多贵
//!
//! 每个 class 都要解压出头部，实测约 35 MB/s：一个三百个 Mod 的整合包第一遍要
//! 十几秒。所以它只在游戏退出之后那一遍扫全部——那时没有人在等；打开实例和点
//! 启动之前只查清单，外加现扫那几个真的变了的文件。sha1 相同就再也不扫，于是
//! 这十几秒一台机器上只付一次。
//!
//! ## 它看不见什么
//!
//! 反射能绕开全部方法引用，`Class.forName("java.lang.Runtime")` 在常量池里只
//! 是一个字符串；走 OkHttp 而不是 `java.net` 的网络调用这里也认不出来。这一层
//! 不是判定，是**一条额外的依据**：绕过它要多花一道工夫，而它几乎免费。

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek},
    net::Ipv4Addr,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{DataPaths, instance::class};

/// 会说出口的那几项。取值直接进文案 id 的参数，界面按它查显示名。
pub(crate) mod name {
    /// 启动另一个进程。
    pub(crate) const RUN_PROGRAM: &str = "run-program";
    /// 在运行时把一段字节变成可执行的类。
    pub(crate) const LOAD_CODE: &str = "load-code";
    /// 主动建立对外连接。
    pub(crate) const NETWORK: &str = "network";
    /// 把字节流还原成对象。
    pub(crate) const DESERIALIZE: &str = "deserialize";
    /// 代码里写死了一个公网地址。
    pub(crate) const PUBLIC_ADDRESS: &str = "public-address";
}

/// 方法引用到能力的对照表。
///
/// 以 `.` 开头的条目只比方法名，不管是谁的方法——`defineClass` 在
/// `ClassLoader`、`MethodHandles$Lookup` 和各家自定义加载器上都叫这个名字，
/// 一条一条列出来只会漏。其余的比全名。
const CALLS: [(&str, &str); 14] = [
    ("java/lang/Runtime.exec", name::RUN_PROGRAM),
    ("java/lang/ProcessBuilder.start", name::RUN_PROGRAM),
    ("java/lang/ProcessBuilder.startPipeline", name::RUN_PROGRAM),
    (".defineClass", name::LOAD_CODE),
    (".defineHiddenClass", name::LOAD_CODE),
    (".defineAnonymousClass", name::LOAD_CODE),
    ("java/net/URLClassLoader.<init>", name::LOAD_CODE),
    ("java/net/URLClassLoader.newInstance", name::LOAD_CODE),
    ("java/net/Socket.<init>", name::NETWORK),
    ("java/net/URL.openStream", name::NETWORK),
    ("java/net/URL.openConnection", name::NETWORK),
    ("java/nio/channels/SocketChannel.open", name::NETWORK),
    ("java/io/ObjectInputStream.readObject", name::DESERIALIZE),
    ("java/io/ObjectInputStream.readUnshared", name::DESERIALIZE),
];

/// 每个 class 只读这么多字节。
///
/// 常量池在文件最前面，读到它结束就该停手。几乎所有 class 的常量池都在几十 KB
/// 以内；给到半兆是为了兜住那些生成出来的巨型类。超出这个长度的部分会让
/// `class::referenced` 提前收尾——少几条引用，不会错几条。
const HEAD: u64 = 512 * 1024;

/// 一个压缩包里最多看这么多个 class。
///
/// 挡的是「声称自己有一百万个条目」的那种包。真实的模组 jar 到不了这个量级，
/// 到得了的那些也不是我们该花时间的地方。
const MOST: usize = 20_000;

/// 一个 jar 里的代码引用了哪些能力。
///
/// 读不动（不是压缩包、文件没了）就是 `None`。**`None` 和空清单不是一回事**：
/// 前者是「不知道」，后者是「看过，什么都没有」，拿前者去做减法会凭空多出一批
/// 能力。
pub(crate) fn scan(path: &Path) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut found = BTreeSet::new();
    let mut budget = MOST;
    walk(&mut archive, 0, &mut found, &mut budget);
    Some(found.into_iter().map(str::to_owned).collect())
}

fn walk<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    depth: usize,
    found: &mut BTreeSet<&'static str>,
    budget: &mut usize,
) {
    // 打包在里面的那些 jar 也算：把 stage 2 塞进一个嵌套模块，外层看起来一个
    // class 都没动。一层就够——攻击者没有理由把载荷藏得比加载器读得更深。
    const MAX_DEPTH: usize = 1;

    let names: Vec<String> = archive
        .file_names()
        .filter(|entry| entry.ends_with(".class") || (depth < MAX_DEPTH && entry.ends_with(".jar")))
        .map(str::to_owned)
        .collect();

    for entry in names {
        if *budget == 0 {
            return;
        }
        *budget -= 1;

        if entry.ends_with(".jar") {
            const WHOLE: u64 = 64 * 1024 * 1024;
            let Some(bytes) = read(archive, &entry, WHOLE) else {
                continue;
            };
            if let Ok(mut nested) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                walk(&mut nested, depth + 1, found, budget);
            }
            continue;
        }

        let Some(bytes) = read(archive, &entry, HEAD) else {
            continue;
        };
        let referenced = class::referenced(&bytes);
        for reference in &referenced.methods {
            if let Some(capability) = from_call(reference) {
                found.insert(capability);
            }
        }
        if referenced.strings.iter().any(|text| public_address(text)) {
            found.insert(name::PUBLIC_ADDRESS);
        }
    }
}

fn read<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry: &str,
    limit: u64,
) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    archive
        .by_name(entry)
        .ok()?
        .take(limit)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn from_call(reference: &str) -> Option<&'static str> {
    // 类名用 `/` 分段，所以整条引用里有且只有一个 `.`，就是类名和方法名之间那个。
    let (_, method) = reference.split_once('.')?;
    CALLS.iter().find_map(|(pattern, capability)| {
        let hit = match pattern.strip_prefix('.') {
            Some(bare) => method == bare,
            None => reference == *pattern,
        };
        hit.then_some(*capability)
    })
}

/// 这个字符串字面量里写死了一个公网 IPv4 地址。
///
/// 只认**确实当地址在用**的那些：整个字面量就是一个地址、跟在 `//` 或 `@` 后
/// 面、或者后面跟着 `:端口`。这一条是为了把版本号挡掉——`1.20.1.4` 也能解析成
/// 一个地址，而模组里的版本号比 IP 常见得多。
///
/// 内网、回环、组播这些地址不算：模组连本机的伴随进程是正常做法。
fn public_address(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'.') {
            at += 1;
        }
        let piece = text[start..at].trim_end_matches('.');
        if let Ok(address) = piece.parse::<Ipv4Addr>()
            && !reserved(&address)
            && addressed(text, start, start + piece.len())
        {
            return true;
        }
    }
    false
}

fn reserved(address: &Ipv4Addr) -> bool {
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_unspecified()
        || address.is_documentation()
        // 0.0.0.0/8 与运营商级 NAT 的 100.64.0.0/10。
        || address.octets()[0] == 0
        || (address.octets()[0] == 100 && (64..128).contains(&address.octets()[1]))
}

fn addressed(text: &str, start: usize, end: usize) -> bool {
    let before = &text[..start];
    let after = &text[end..];
    if before.is_empty() && after.is_empty() {
        return true;
    }
    if before.ends_with("//") || before.ends_with('@') {
        return true;
    }
    after
        .strip_prefix(':')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|first| first.is_ascii_digit())
}

/// 扫过的清单，按内容存。
///
/// 键是文件的 sha1：同一个 sha1 的内容永远是同一份，所以这份缓存没有失效一说，
/// 也不需要记路径——一个 jar 从别处复制过来，它的清单直接就是现成的。
///
/// 删掉它只是下一遍彻底对账慢一点。
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Known {
    #[serde(default)]
    entries: BTreeMap<String, Vec<String>>,
    #[serde(skip)]
    dirty: bool,
}

impl Known {
    pub(crate) fn open(paths: &DataPaths) -> Self {
        std::fs::read(Self::path(paths))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// 这份内容扫过没有。没扫过就是 `None`——**不去扫**，这一条是给便宜那一档
    /// 用的。
    pub(crate) fn recorded(&self, sha1: &str) -> Option<Vec<String>> {
        self.entries.get(sha1).cloned()
    }

    /// 这份内容引用了哪些能力，没扫过就现扫一遍。
    pub(crate) fn of(&mut self, sha1: &str, path: &Path) -> Option<Vec<String>> {
        if let Some(known) = self.entries.get(sha1) {
            return Some(known.clone());
        }
        let found = scan(path)?;
        self.entries.insert(sha1.to_owned(), found.clone());
        self.dirty = true;
        Some(found)
    }

    pub(crate) fn save(&self, paths: &DataPaths) {
        if !self.dirty {
            return;
        }
        let _ = std::fs::create_dir_all(&paths.cache);
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = std::fs::write(Self::path(paths), bytes);
        }
    }

    fn path(paths: &DataPaths) -> std::path::PathBuf {
        paths.cache.join("jar-capabilities.json")
    }
}

/// 攒一份能力清单认得出来的 class，给测试用。`integrity` 那边也要一份。
#[cfg(test)]
pub(crate) mod fixture {
    /// 一份只有常量池的 class：一次方法调用，外加一个字符串字面量。
    ///
    /// 常量池之后的字段、方法、属性一概不写——解析读到常量池结束就停手，所以
    /// 「后面什么都没有」正是它该能处理的情况。
    pub(crate) fn class_calling(owner: &str, method: &str, literal: &str) -> Vec<u8> {
        let mut pool: Vec<u8> = Vec::new();
        let mut count: u16 = 1;
        let utf8 = |pool: &mut Vec<u8>, count: &mut u16, value: &str| {
            pool.push(1);
            pool.extend_from_slice(&(value.len() as u16).to_be_bytes());
            pool.extend_from_slice(value.as_bytes());
            let index = *count;
            *count += 1;
            index
        };

        let owner_index = utf8(&mut pool, &mut count, owner);
        let method_index = utf8(&mut pool, &mut count, method);
        let descriptor = utf8(&mut pool, &mut count, "()V");
        let literal_index = (!literal.is_empty()).then(|| utf8(&mut pool, &mut count, literal));

        pool.push(7);
        pool.extend_from_slice(&owner_index.to_be_bytes());
        let class_index = count;
        count += 1;

        pool.push(12);
        pool.extend_from_slice(&method_index.to_be_bytes());
        pool.extend_from_slice(&descriptor.to_be_bytes());
        let signature = count;
        count += 1;

        pool.push(10);
        pool.extend_from_slice(&class_index.to_be_bytes());
        pool.extend_from_slice(&signature.to_be_bytes());
        count += 1;

        if let Some(index) = literal_index {
            pool.push(8);
            pool.extend_from_slice(&index.to_be_bytes());
            count += 1;
        }

        let mut out = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&pool);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{fixture::class_calling, *};

    #[test]
    fn the_calls_that_matter_map_to_a_name() {
        assert_eq!(from_call("java/lang/Runtime.exec"), Some(name::RUN_PROGRAM));
        // 谁的 defineClass 都算。
        assert_eq!(
            from_call("net/example/Loader.defineClass"),
            Some(name::LOAD_CODE)
        );
        assert_eq!(from_call("java/net/Socket.<init>"), Some(name::NETWORK));
    }

    #[test]
    fn ordinary_calls_map_to_nothing() {
        assert_eq!(from_call("java/util/List.add"), None);
        assert_eq!(from_call("java/lang/String.format"), None);
        // 方法名对得上、类名对不上的，全名匹配那几条不该命中。
        assert_eq!(from_call("net/example/Thing.exec"), None);
    }

    #[test]
    fn a_hardcoded_public_address_is_recognised() {
        assert!(public_address("85.217.144.130"));
        assert!(public_address("http://85.217.144.130/dl"));
        assert!(public_address("85.217.144.130:8080"));
    }

    /// 版本号也能解析成地址，而模组里的版本号比 IP 常见得多。挡住它靠的是
    /// 上下文：一个版本号既不跟在 `//` 后面，也不带端口。
    #[test]
    fn a_version_number_is_not_an_address() {
        assert!(!public_address("fabric-api 1.20.1.4 for Minecraft"));
        assert!(!public_address("v0.14.21.3-beta"));
    }

    #[test]
    fn addresses_on_this_machine_or_this_network_do_not_count() {
        assert!(!public_address("127.0.0.1:25565"));
        assert!(!public_address("192.168.1.10"));
        assert!(!public_address("10.0.0.1:8080"));
        assert!(!public_address("0.0.0.0"));
    }

    #[test]
    fn a_jar_that_calls_exec_says_so_and_a_plain_one_says_nothing() {
        let root = std::env::temp_dir().join(format!("fern-capability-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");

        let quiet = root.join("quiet.jar");
        jar(
            &quiet,
            &[(
                "net/example/Quiet.class",
                &class_calling("java/util/List", "add", ""),
            )],
        );
        assert_eq!(scan(&quiet), Some(Vec::new()));

        let loud = root.join("loud.jar");
        jar(
            &loud,
            &[(
                "net/example/Loud.class",
                &class_calling("java/lang/Runtime", "exec", "http://203.0.113.9/x"),
            )],
        );
        // 203.0.113.0/24 是文档保留段，不该算成公网地址。
        assert_eq!(scan(&loud), Some(vec![name::RUN_PROGRAM.to_owned()]));

        let staged = root.join("staged.jar");
        jar(
            &staged,
            &[(
                "net/example/Staged.class",
                &class_calling("java/net/URL", "openStream", "http://85.217.144.130/dl"),
            )],
        );
        assert_eq!(
            scan(&staged),
            Some(vec![
                name::NETWORK.to_owned(),
                name::PUBLIC_ADDRESS.to_owned()
            ])
        );

        assert_eq!(scan(&root.join("missing.jar")), None);
    }

    /// 载荷藏进一个嵌套 jar，外层一个 class 都没动。
    #[test]
    fn a_nested_jar_is_looked_into_as_well() {
        let root =
            std::env::temp_dir().join(format!("fern-capability-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");

        let inner = root.join("inner.jar");
        jar(
            &inner,
            &[(
                "net/example/Inner.class",
                &class_calling("java/lang/Runtime", "exec", ""),
            )],
        );
        let bytes = std::fs::read(&inner).expect("read");

        let outer = root.join("outer.jar");
        jar(&outer, &[("META-INF/jars/inner.jar", &bytes)]);
        assert_eq!(scan(&outer), Some(vec![name::RUN_PROGRAM.to_owned()]));
    }

    fn jar(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("create");
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            std::io::Write::write_all(&mut writer, bytes).expect("write");
        }
        writer.finish().expect("finish");
    }
}
