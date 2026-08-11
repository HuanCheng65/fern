//! zip 层：重打一个 jar，顺带把签名剥干净。
//!
//! 为什么必须剥（文档 §3.4 第二条）：Forge 的 universal jar 自己是签名的
//! （`FORGE.SF` / `FORGE.DSA`，条目摘要 SHA-256）。只换掉一个 class 而把签名
//! 留在原地，得到的是一份**签名无效**的 jar；它今天不炸，只是因为那份 2014
//! 年的签名本身已经不被现代 JVM 校验——不能靠这个。剥成一份干净的未签名 jar
//! 才是说得出口的状态。

use std::{
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result};

/// 逐条过一遍这个 jar。
///
/// `rewrite` 拿到条目名和内容，返回 `Some(新内容)` 就换掉，`None` 表示这一条
/// 不归它管。返回值是「**补丁**到底换过没有」——一次都没换的话，这份补丁对这
/// 个 jar 不适用，产物不会落盘，调用方继续用原件。
///
/// 剥签名不算「换过」。它是打补丁的**后果**，不是打补丁的理由：Forge 的每一
/// 个 universal jar 都是签名的，把剥签名也算进去的话，每一个 Forge 实例都会
/// 凭空多出一份和原件只差 `META-INF/` 的产物，还跟着多一个用不上的
/// `-Dfml.ignoreInvalidMinecraftCertificates=true`。
pub(crate) fn rewrite(
    source: &Path,
    destination: &Path,
    mut rewrite: impl FnMut(&str, &[u8]) -> Result<Option<Vec<u8>>>,
) -> Result<bool> {
    let reader =
        std::fs::File::open(source).with_context(|| format!("打开 {}", source.display()))?;
    let mut archive =
        zip::ZipArchive::new(reader).with_context(|| format!("读取 {}", source.display()))?;

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 写到旁边再改名：中途失败不会在缓存目录里留下一份看着能用的半成品。
    let temporary = destination.with_extension("part");
    let mut writer = zip::ZipWriter::new(
        std::fs::File::create(&temporary)
            .with_context(|| format!("创建 {}", temporary.display()))?,
    );
    // 时间戳固定，同样的输入要得到同样的 sha1——缓存和清单都指望这一点。
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut patched = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if entry.is_dir() {
            writer.add_directory(&name, options)?;
            continue;
        }
        if is_signature_file(&name) {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        if name.eq_ignore_ascii_case("META-INF/MANIFEST.MF") {
            bytes = strip_digests(&bytes);
        } else if let Some(replacement) =
            rewrite(&name, &bytes).with_context(|| format!("改写 {name}"))?
        {
            bytes = replacement;
            patched = true;
        }
        writer.start_file(&name, options)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    if !patched {
        // 白忙一场，别在缓存目录里留下一份没人要的 jar。
        let _ = std::fs::remove_file(&temporary);
        return Ok(false);
    }
    std::fs::rename(&temporary, destination)?;
    Ok(true)
}

/// `META-INF/` 下那几份签名文件。
///
/// 只认直接放在 `META-INF/` 下的：签名规范就是这么定的，而模组的资源里完全
/// 可能有一个叫 `assets/…/foo.rsa` 的文件。
fn is_signature_file(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("META-INF/")
        .or_else(|| name.strip_prefix("meta-inf/"))
    else {
        return false;
    };
    if rest.contains('/') {
        return false;
    }
    let Some((_, extension)) = rest.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension.to_ascii_uppercase().as_str(),
        "SF" | "RSA" | "DSA" | "EC"
    )
}

/// 去掉 MANIFEST 里的逐条摘要。
///
/// 签名 jar 的 MANIFEST 在主段之后跟着一串 `Name:` 段，每段里是那个条目的
/// 摘要。主段留着（`Main-Class` 之类真正有用的东西在那儿），带摘要的段整段
/// 丢掉；不带摘要的 `Name:` 段留着——那是别人写的普通属性，不归签名管。
fn strip_digests(bytes: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        // 解不出 UTF-8 的 MANIFEST 不去猜，原样留着。
        return bytes.to_vec();
    };
    let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut output = String::with_capacity(text.len());
    let mut first = true;
    for section in text.split("\r\n\r\n").flat_map(|part| part.split("\n\n")) {
        if section.trim().is_empty() {
            continue;
        }
        let has_digest = section.lines().any(|line| {
            line.split(':')
                .next()
                .is_some_and(|key| key.ends_with("-Digest"))
        });
        if has_digest && !first {
            continue;
        }
        if !first {
            output.push_str(line_ending);
        }
        output.push_str(section.trim_end_matches(['\r', '\n']));
        output.push_str(line_ending);
        first = false;
    }
    output.push_str(line_ending);
    output.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).expect("create jar"));
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(bytes).expect("write entry");
        }
        writer.finish().expect("finish jar");
    }

    fn names(path: &Path) -> Vec<String> {
        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(path).expect("open")).expect("read");
        (0..archive.len())
            .map(|index| archive.by_index(index).expect("entry").name().to_owned())
            .collect()
    }

    #[test]
    fn signatures_are_stripped_and_the_target_class_is_replaced() {
        let root = std::env::temp_dir().join(format!("fern-jar-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create root");
        let source = root.join("forge.jar");
        write_jar(
            &source,
            &[
                (
                    "META-INF/MANIFEST.MF",
                    b"Manifest-Version: 1.0\nMain-Class: cpw.mods.fml.Start\n\n\
                      Name: cpw/mods/fml/relauncher/CoreModManager.class\n\
                      SHA-256-Digest: ZmFrZQ==\n\n" as &[u8],
                ),
                ("META-INF/FORGE.SF", b"Signature-Version: 1.0\n"),
                ("META-INF/FORGE.DSA", b"\x30\x82"),
                ("cpw/mods/fml/relauncher/CoreModManager.class", b"original"),
                ("assets/logo.png", b"png"),
            ],
        );

        let destination = root.join("patched.jar");
        let changed = rewrite(&source, &destination, |name, _| {
            Ok(name
                .ends_with("CoreModManager.class")
                .then(|| b"patched".to_vec()))
        })
        .expect("rewrite");
        assert!(changed);

        let entries = names(&destination);
        assert!(!entries.iter().any(|name| name.ends_with(".SF")));
        assert!(!entries.iter().any(|name| name.ends_with(".DSA")));
        assert!(entries.iter().any(|name| name == "assets/logo.png"));

        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(&destination).expect("open")).expect("read");
        let mut manifest = String::new();
        archive
            .by_name("META-INF/MANIFEST.MF")
            .expect("manifest")
            .read_to_string(&mut manifest)
            .expect("read manifest");
        assert!(manifest.contains("Main-Class: cpw.mods.fml.Start"));
        assert!(!manifest.contains("SHA-256-Digest"));

        // 原件一个字节都没动过。
        let mut original =
            zip::ZipArchive::new(std::fs::File::open(&source).expect("open")).expect("read source");
        let mut class = String::new();
        original
            .by_name("cpw/mods/fml/relauncher/CoreModManager.class")
            .expect("class")
            .read_to_string(&mut class)
            .expect("read class");
        assert_eq!(class, "original");

        std::fs::remove_dir_all(root).expect("remove root");
    }

    /// 一条都没换上就说明这份补丁跟这个 jar 无关：不算改过，也不留下产物。
    ///
    /// 这个 jar **是签名的**，正是曾经出错的地方——把「剥掉了签名」也算成
    /// 「改过了」，于是每一个 Forge 实例都多出一份和原件只差 `META-INF/` 的
    /// 产物（1.7.10 和 1.12.2 的 `sortTweakList` 早就是好的），还跟着多一个
    /// 用不上的 `-Dfml.ignoreInvalidMinecraftCertificates=true`。
    #[test]
    fn a_signed_jar_the_patch_does_not_touch_produces_nothing() {
        let root = std::env::temp_dir().join(format!("fern-jar-none-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create root");
        let source = root.join("plain.jar");
        write_jar(
            &source,
            &[
                (
                    "META-INF/MANIFEST.MF",
                    b"Manifest-Version: 1.0\n\nName: a/B.class\nSHA-256-Digest: x\n\n" as &[u8],
                ),
                ("META-INF/FORGE.SF", b"Signature-Version: 1.0\n"),
                ("a/B.class", b"bytes"),
            ],
        );
        let output = root.join("out.jar");
        assert!(!rewrite(&source, &output, |_, _| Ok(None)).expect("rewrite"));
        assert!(!output.exists());
        assert!(!output.with_extension("part").exists());
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn only_signature_files_directly_under_meta_inf_count() {
        assert!(is_signature_file("META-INF/FORGE.SF"));
        assert!(is_signature_file("META-INF/forge.rsa"));
        assert!(!is_signature_file("META-INF/services/foo.SF"));
        assert!(!is_signature_file("assets/pack/thing.rsa"));
        assert!(!is_signature_file("META-INF/MANIFEST.MF"));
    }
}
