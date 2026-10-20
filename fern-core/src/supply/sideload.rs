//! 只能手动下载的文件。
//!
//! CurseForge 上有些作者关掉了「允许第三方分发」：API 照样告诉我们这个文件
//! 叫什么、多大、sha1 是多少，只是不给下载地址。启动器能做的是把用户送到那
//! 个文件的网页上，等他下完，再把那份文件找回来。
//!
//! 找回来有两条路，都要过 sha1：
//!
//! - **下载目录。** 浏览器默认存到那里，文件名就是上游的文件名（重名时浏览器
//!   会加 ` (1)`）。下完回来点一下「重新检查」就够了，不用自己去翻。
//! - **用户自己选的文件。** 浏览器问过存哪、或者用户早就下过一份放在别处。
//!   这时文件名不可信，只认内容。
//!
//! 拼一个 CDN 地址绕过作者的设置是做得到的，有的启动器就这么做。我们不做：
//! 那是作者明确说了不要的事。

use std::path::{Path, PathBuf};

/// 去哪里找、用户另外指了哪些文件。
#[derive(Debug, Clone, Default)]
pub struct Sideload {
    /// 按文件名找的目录。一般就是系统的下载目录。
    pub places: Vec<PathBuf>,
    /// 用户用文件选择器指的文件。不看名字，只看内容。
    pub picked: Vec<PathBuf>,
}

/// 浏览器给重名文件加的后缀最多试到几。
const DUPLICATES: u32 = 9;

impl Sideload {
    /// 找一份内容对得上的文件。
    ///
    /// `sha1` 有就只认 sha1；没有（CurseForge 的老文件偶尔不给）就退一步认
    /// 大小——两样都没有就不认，宁可让用户再指一次，也不把一个同名的别的
    /// 东西装进去。
    pub(crate) fn locate(&self, name: &str, sha1: Option<&str>, size: u64) -> Option<PathBuf> {
        let matches = |path: &Path| -> bool {
            let Ok(metadata) = std::fs::metadata(path) else {
                return false;
            };
            if !metadata.is_file() {
                return false;
            }
            match sha1 {
                Some(expected) => crate::backup::sha1_of(path)
                    .is_ok_and(|actual| actual.eq_ignore_ascii_case(expected)),
                None => size > 0 && metadata.len() == size,
            }
        };

        // 先看用户指的：那是他明确说「就是这个」的。
        if let Some(found) = self.picked.iter().find(|path| matches(path)) {
            return Some(found.clone());
        }
        // 文件名来自上游，拼路径之前确认它只是一个名字，不是一段路径。
        if Path::new(name).file_name().and_then(|it| it.to_str()) != Some(name) {
            return None;
        }
        for place in &self.places {
            for candidate in duplicates(name) {
                let path = place.join(&candidate);
                if matches(&path) {
                    return Some(path);
                }
            }
        }
        None
    }
}

/// `mod.jar`、`mod (1).jar` … `mod (9).jar`。
fn duplicates(name: &str) -> Vec<String> {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    std::iter::once(name.to_owned())
        .chain((1..=DUPLICATES).map(|n| format!("{stem} ({n}){extension}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fern-sideload-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");
        root
    }

    fn sha1(bytes: &[u8]) -> String {
        use sha1::{Digest, Sha1};
        format!("{:x}", Sha1::digest(bytes))
    }

    #[test]
    fn a_browser_renamed_duplicate_is_still_found() {
        let root = root("dup");
        std::fs::write(root.join("mod.jar"), b"stale").expect("write");
        std::fs::write(root.join("mod (2).jar"), b"right").expect("write");
        let sideload = Sideload {
            places: vec![root.clone()],
            picked: Vec::new(),
        };
        // 同名的那份内容不对，要往下找到 ` (2)` 那份。
        assert_eq!(
            sideload.locate("mod.jar", Some(&sha1(b"right")), 5),
            Some(root.join("mod (2).jar"))
        );
        std::fs::remove_dir_all(root).expect("clean");
    }

    #[test]
    fn a_picked_file_is_matched_by_content_not_by_name() {
        let root = root("picked");
        let path = root.join("whatever.bin");
        std::fs::write(&path, b"right").expect("write");
        let sideload = Sideload {
            places: Vec::new(),
            picked: vec![path.clone()],
        };
        assert_eq!(
            sideload.locate("mod.jar", Some(&sha1(b"right")), 5),
            Some(path)
        );
        assert_eq!(sideload.locate("mod.jar", Some(&sha1(b"other")), 5), None);
        std::fs::remove_dir_all(root).expect("clean");
    }

    #[test]
    fn a_name_from_upstream_cannot_reach_outside_the_folder() {
        let root = root("escape");
        std::fs::write(root.join("secret"), b"x").expect("write");
        let sideload = Sideload {
            places: vec![root.join("downloads")],
            picked: Vec::new(),
        };
        assert_eq!(sideload.locate("../secret", None, 1), None);
        std::fs::remove_dir_all(root).expect("clean");
    }

    #[test]
    fn without_a_hash_or_a_size_nothing_is_accepted() {
        let root = root("blind");
        std::fs::write(root.join("mod.jar"), b"x").expect("write");
        let sideload = Sideload {
            places: vec![root.clone()],
            picked: Vec::new(),
        };
        assert_eq!(sideload.locate("mod.jar", None, 0), None);
        assert_eq!(
            sideload.locate("mod.jar", None, 1),
            Some(root.join("mod.jar"))
        );
        std::fs::remove_dir_all(root).expect("clean");
    }
}
