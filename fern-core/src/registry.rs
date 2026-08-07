//! Windows 注册表里登记的 Java（文档 §4.2）。
//!
//! 各家 JDK 安装器都会在 `HKLM\SOFTWARE\JavaSoft\*` 下面写一条，值里带
//! `JavaHome`。这是唯一能找到「装在非默认目录、又不在 PATH 上」那些 JDK 的
//! 办法——而那种情况在 Windows 上并不罕见：安装器让人选目录，很多人就随手改
//! 到别的盘去了。
//!
//! 32 位和 64 位视图要分别看。装了 32 位 JRE 的机器上，条目只在
//! `WOW64_32KEY` 那一侧；反过来也一样。不指定视图的话，看到的取决于我们
//! 自己是多少位的进程，那不是我们想要的。

#![cfg(windows)]

use std::path::PathBuf;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS,
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};

/// 各家安装器写的位置。前两条是老约定，后两条是 JDK 9 之后的。
const ROOTS: [&str; 6] = [
    r"SOFTWARE\JavaSoft\Java Runtime Environment",
    r"SOFTWARE\JavaSoft\Java Development Kit",
    r"SOFTWARE\JavaSoft\JRE",
    r"SOFTWARE\JavaSoft\JDK",
    // Eclipse Adoptium / Temurin 自己也登记一份。
    r"SOFTWARE\Eclipse Adoptium\JRE",
    r"SOFTWARE\Eclipse Adoptium\JDK",
];

/// 注册表里登记过的所有 JavaHome。
///
/// 只负责报路径，能不能用由 `java::probe_home` 判断——这里返回的目录完全
/// 可能指向一个已经被删掉的安装。
pub fn java_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for root in ROOTS {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            collect(root, view, &mut homes);
        }
    }
    homes.sort();
    homes.dedup();
    homes
}

fn collect(root: &str, view: REG_SAM_FLAGS, homes: &mut Vec<PathBuf>) {
    let Some(root_key) = open(HKEY_LOCAL_MACHINE, root, view) else {
        return;
    };
    // 每个子键是一个版本号（`1.8.0_402`、`17.0.9` 等）。
    for version in subkeys(root_key.0) {
        let path = format!("{root}\\{version}");
        let Some(version_key) = open(HKEY_LOCAL_MACHINE, &path, view) else {
            continue;
        };
        if let Some(home) = string_value(version_key.0, "JavaHome") {
            homes.push(PathBuf::from(home));
        }
    }
}

/// 打开就关，别把句柄漏出去。
struct OwnedKey(HKEY);

impl Drop for OwnedKey {
    fn drop(&mut self) {
        // SAFETY: 句柄是 RegOpenKeyExW 成功时给的，且只在这里关一次。
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open(parent: HKEY, path: &str, view: REG_SAM_FLAGS) -> Option<OwnedKey> {
    let wide = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: wide 是以 NUL 结尾的合法 UTF-16；key 只在返回 ERROR_SUCCESS 时被写。
    let status = unsafe { RegOpenKeyExW(parent, wide.as_ptr(), 0, KEY_READ | view, &mut key) };
    (status == ERROR_SUCCESS).then_some(OwnedKey(key))
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut names = Vec::new();
    // 注册表键名上限是 255 个字符，加上结尾的 NUL。
    let mut buffer = [0u16; 256];
    for index in 0.. {
        let mut length = buffer.len() as u32;
        // SAFETY: buffer 有 length 个 u16 的空间，API 会把实际长度写回 length。
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        names.push(String::from_utf16_lossy(&buffer[..length as usize]));
    }
    names
}

fn string_value(key: HKEY, name: &str) -> Option<String> {
    let wide_name = wide(name);
    let mut size: u32 = 0;
    // 先问长度：JavaHome 是路径，可能超过任何我们敢写死的缓冲区。
    // SAFETY: lpdata 为空时 API 只写 size。
    let status = unsafe {
        RegQueryValueExW(
            key,
            wide_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }

    let mut bytes = vec![0u8; size as usize];
    // SAFETY: bytes 至少有 size 个字节。
    let status = unsafe {
        RegQueryValueExW(
            key,
            wide_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    // 值是 UTF-16，字节数要折半；结尾的 NUL 不属于内容。
    let units: Vec<u16> = bytes[..size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let trimmed = text.trim_end_matches('\0').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
