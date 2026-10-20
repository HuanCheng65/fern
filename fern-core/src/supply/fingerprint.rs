//! CurseForge 的文件指纹。
//!
//! CurseForge 不按 sha1 反查，按它自己的指纹：把文件里的空白字节（`\t` `\n`
//! `\r` 和空格）全部去掉，剩下的字节做一遍 32 位 MurmurHash2，种子是 1。
//! 去空白是历史遗留——它最初是给文本文件设计的，后来 jar 也沿用了同一套。
//!
//! 「先去空白、再哈希」意味着要先知道去掉之后有多长（MurmurHash2 的初值里
//! 有长度），所以一个文件读两遍：第一遍数，第二遍算。几百个模组里最大的也就
//! 几十兆，两遍顺序读比把整个文件搬进内存更稳。

use std::{
    io::{self, Read},
    path::Path,
};

const SEED: u32 = 1;
const M: u32 = 0x5bd1_e995;

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, 9 | 10 | 13 | 32)
}

/// 一个文件的指纹。
pub(crate) fn of_file(path: &Path) -> io::Result<u32> {
    let mut length: u32 = 0;
    each_kept_byte(path, |_| length = length.wrapping_add(1))?;
    let mut hasher = Murmur2::new(length);
    each_kept_byte(path, |byte| hasher.push(byte))?;
    Ok(hasher.finish())
}

/// 一段内存的指纹。测试和小文件用。
#[cfg(test)]
pub(crate) fn of_bytes(bytes: &[u8]) -> u32 {
    let kept: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|byte| !is_whitespace(*byte))
        .collect();
    let mut hasher = Murmur2::new(kept.len() as u32);
    for byte in kept {
        hasher.push(byte);
    }
    hasher.finish()
}

fn each_kept_byte(path: &Path, mut visit: impl FnMut(u8)) -> io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        for &byte in &buffer[..read] {
            if !is_whitespace(byte) {
                visit(byte);
            }
        }
    }
}

/// 逐字节喂的 32 位 MurmurHash2（Austin Appleby 的原版），小端读四字节一组。
struct Murmur2 {
    hash: u32,
    pending: [u8; 4],
    filled: usize,
}

impl Murmur2 {
    fn new(length: u32) -> Self {
        Self {
            hash: SEED ^ length,
            pending: [0; 4],
            filled: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        self.pending[self.filled] = byte;
        self.filled += 1;
        if self.filled == 4 {
            let mut k = u32::from_le_bytes(self.pending);
            k = k.wrapping_mul(M);
            k ^= k >> 24;
            k = k.wrapping_mul(M);
            self.hash = self.hash.wrapping_mul(M) ^ k;
            self.filled = 0;
        }
    }

    fn finish(mut self) -> u32 {
        let tail = &self.pending[..self.filled];
        if !tail.is_empty() {
            if tail.len() >= 3 {
                self.hash ^= u32::from(tail[2]) << 16;
            }
            if tail.len() >= 2 {
                self.hash ^= u32::from(tail[1]) << 8;
            }
            self.hash ^= u32::from(tail[0]);
            self.hash = self.hash.wrapping_mul(M);
        }
        let mut hash = self.hash;
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(M);
        hash ^= hash >> 15;
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_does_not_change_the_fingerprint() {
        // 这是 CurseForge 这套指纹的定义：空白不算内容。
        assert_eq!(of_bytes(b"ab cd\n"), of_bytes(b"abcd"));
        assert_eq!(of_bytes(b"\t\r\n "), of_bytes(b""));
    }

    #[test]
    fn every_tail_length_is_mixed_in() {
        // 四字节一组之外剩下的 1、2、3 个字节各走一条分支，少算一个就会撞。
        let values: Vec<u32> = ["abcd", "abcde", "abcdef", "abcdefg"]
            .iter()
            .map(|text| of_bytes(text.as_bytes()))
            .collect();
        for (index, value) in values.iter().enumerate() {
            assert!(!values[index + 1..].contains(value));
        }
    }

    #[test]
    fn reading_from_disk_agrees_with_reading_from_memory() {
        let path = std::env::temp_dir().join(format!("fern-fingerprint-{}", std::process::id()));
        let body: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        std::fs::write(&path, &body).expect("write");
        assert_eq!(of_file(&path).expect("read"), of_bytes(&body));
        std::fs::remove_file(path).expect("clean");
    }
}
