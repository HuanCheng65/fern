//! 这块盘有多大。
//!
//! 只为一件事存在：快照的占用上限要摆成一根尺，而一根尺得有右端。写死一个数
//! （「最多 200 GB」）在一台 128 GB 的笔记本和一台 8 TB 的机器上都是错的，而
//! 「这块盘一共多少」恰好是那个既真实又人人看得懂的右端——和内存那根尺拿物理
//! 内存当量程是同一个道理。
//!
//! 读不到就是 `None`。界面那时退回一个输入框，不编一个数出来。

use std::path::Path;

/// 一块盘的容量与剩余，字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub total: u64,
    /// 当前进程能用的剩余空间。Unix 上取 `f_bavail` 而不是 `f_bfree`——后者
    /// 含着只有 root 才动得了的保留块，对普通用户是个够不到的数。
    pub free: u64,
}

/// `path` 所在的那块盘。目录不存在时往上找到第一个存在的祖先。
pub fn space(path: &Path) -> Option<DiskSpace> {
    let mut probe = path;
    loop {
        if probe.exists() {
            return space_of(probe);
        }
        probe = probe.parent()?;
    }
}

#[cfg(unix)]
fn space_of(path: &Path) -> Option<DiskSpace> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let raw = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: 传进去的是一个以 NUL 结尾的合法路径，statvfs 只往这块栈上的
    // 结构里写。返回 0 之前不读它。
    let stats = unsafe {
        let mut stats = std::mem::zeroed::<libc::statvfs>();
        if libc::statvfs(raw.as_ptr(), &mut stats) != 0 {
            return None;
        }
        stats
    };
    // 块大小用 f_frsize：f_bsize 是「推荐的 IO 大小」，和块数不配套。
    let block = if stats.f_frsize > 0 {
        stats.f_frsize as u64
    } else {
        stats.f_bsize as u64
    };
    Some(DiskSpace {
        total: (stats.f_blocks as u64).saturating_mul(block),
        free: (stats.f_bavail as u64).saturating_mul(block),
    })
}

#[cfg(windows)]
fn space_of(path: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    let mut total = 0u64;
    // SAFETY: `wide` 以 NUL 结尾，两个输出参数是本地变量，第三个传空表示不要。
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    Some(DiskSpace {
        total,
        // 配额之下这个数才是「我还能写多少」，比整盘剩余更接近事实。
        free: available,
    })
}

#[cfg(not(any(unix, windows)))]
fn space_of(_: &Path) -> Option<DiskSpace> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disk_under_the_temporary_directory_has_a_size() {
        let Some(space) = space(&std::env::temp_dir()) else {
            // 读不出来是允许的结果，那时界面退回输入框。
            return;
        };
        assert!(space.total > 0, "一块盘不可能是 0 字节");
        assert!(space.free <= space.total);
    }

    /// 数据目录第一次启动时还不存在，那时候要回答的是「它将来落在哪块盘上」。
    ///
    /// 只比容量，不比剩余：剩余在两次调用之间就会变——别的测试正在同一块盘上
    /// 写临时文件。
    #[test]
    fn a_directory_that_does_not_exist_yet_answers_from_its_parent() {
        let missing = std::env::temp_dir().join("fern-disk-not-here/nor-here");
        assert_eq!(
            space(&missing).map(|space| space.total),
            space(&std::env::temp_dir()).map(|space| space.total)
        );
    }
}
