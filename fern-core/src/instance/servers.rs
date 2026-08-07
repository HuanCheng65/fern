//! 实例里存的服务器列表。
//!
//! 游戏把「多人游戏」那一屏的条目写在 `.minecraft/servers.dat`，格式是未压缩
//! 的 NBT。这里只读它，不写——写一份坏的 servers.dat 会让玩家的服务器列表整个
//! 消失，而我们没有任何理由去改它。
//!
//! 只实现读取所需的那一部分：一个能跳过任意标签的遍历器，加上取出 `servers`
//! 列表里 `name` 和 `ip` 的那几行。引一个完整的 NBT 库，是为了两个字符串去
//! 背一整套写入、区块编码和压缩策略。

use std::fs;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::DataPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    /// 玩家自己起的名字。没填过就是空的，界面拿地址顶上。
    pub name: String,
    /// `host` 或 `host:port`，原样保留——它要被交回给游戏。
    pub address: String,
}

/// 读一个实例的服务器列表。
///
/// 文件不存在是正常的：没进过多人游戏的实例就没有这份文件。读坏了也只当没有
/// ——这份文件不归我们管，格式对不上不该让搜索整个失败。
pub fn list(paths: &DataPaths, instance_id: &str) -> Vec<ServerEntry> {
    let path = crate::instance::paths_by_id(paths, instance_id)
        .game_directory(instance_id)
        .join("servers.dat");
    let Ok(bytes) = fs::read(&path) else {
        return Vec::new();
    };
    parse(&bytes).unwrap_or_default()
}

fn parse(bytes: &[u8]) -> Result<Vec<ServerEntry>> {
    // 原版写的是未压缩 NBT，但有些工具会 gzip 一份回去。两种都认。
    let owned;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut out)
            .context("解压 servers.dat")?;
        owned = out;
        &owned[..]
    } else {
        bytes
    };

    let mut reader = Reader { bytes, at: 0 };
    // 根标签总是一个带名字的 Compound。
    if reader.u8()? != TAG_COMPOUND {
        return Err(anyhow!("servers.dat 的根不是 compound"));
    }
    reader.string()?;
    let mut servers = Vec::new();
    reader.compound(&mut |reader, tag, name| {
        if tag == TAG_LIST && name == "servers" {
            let element = reader.u8()?;
            let count = reader.i32()?.max(0) as usize;
            for _ in 0..count {
                if element != TAG_COMPOUND {
                    return Err(anyhow!("servers 列表里不是 compound"));
                }
                let mut entry = ServerEntry {
                    name: String::new(),
                    address: String::new(),
                };
                reader.compound(&mut |reader, tag, name| {
                    match (tag, name) {
                        (TAG_STRING, "name") => entry.name = reader.string()?,
                        (TAG_STRING, "ip") => entry.address = reader.string()?,
                        _ => reader.skip(tag)?,
                    }
                    Ok(())
                })?;
                // 没有地址的条目连不上，列出来只会浪费一次点击。
                if !entry.address.is_empty() {
                    servers.push(entry);
                }
            }
            return Ok(());
        }
        reader.skip(tag)
    })?;
    Ok(servers)
}

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| anyhow!("servers.dat 在第 {} 字节处截断", self.at))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Result<i32> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// NBT 的字符串是 u16 长度加 modified UTF-8。
    ///
    /// 按普通 UTF-8 读：两者只在 NUL 和补充平面字符上有分歧，而这里读的是服务器
    /// 名和地址。读不出来就给空串，不是让整份列表失败。
    fn string(&mut self) -> Result<String> {
        let length = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(length)?).into_owned())
    }

    /// 遍历一个 compound 的每一项，直到 TAG_End。
    fn compound(&mut self, visit: &mut dyn FnMut(&mut Self, u8, &str) -> Result<()>) -> Result<()> {
        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                return Ok(());
            }
            let name = self.string()?;
            visit(self, tag, &name)?;
        }
    }

    /// 跳过一个已经读掉类型和名字的标签。
    fn skip(&mut self, tag: u8) -> Result<()> {
        match tag {
            TAG_BYTE => self.take(1).map(|_| ()),
            TAG_SHORT => self.take(2).map(|_| ()),
            TAG_INT | TAG_FLOAT => self.take(4).map(|_| ()),
            TAG_LONG | TAG_DOUBLE => self.take(8).map(|_| ()),
            TAG_BYTE_ARRAY => {
                let count = self.i32()?.max(0) as usize;
                self.take(count).map(|_| ())
            }
            TAG_STRING => self.string().map(|_| ()),
            TAG_LIST => {
                let element = self.u8()?;
                let count = self.i32()?.max(0) as usize;
                for _ in 0..count {
                    if element == TAG_COMPOUND {
                        self.compound(&mut |reader, tag, _| reader.skip(tag))?;
                    } else {
                        self.skip(element)?;
                    }
                }
                Ok(())
            }
            TAG_COMPOUND => self.compound(&mut |reader, tag, _| reader.skip(tag)),
            TAG_INT_ARRAY => {
                let count = self.i32()?.max(0) as usize;
                self.take(count.saturating_mul(4)).map(|_| ())
            }
            TAG_LONG_ARRAY => {
                let count = self.i32()?.max(0) as usize;
                self.take(count.saturating_mul(8)).map(|_| ())
            }
            other => Err(anyhow!("servers.dat 里有未知的标签类型 {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手写一份 servers.dat。真实文件里还夹着 icon 和几个 byte，所以这里
    /// 也放上——解析器必须能跳过它不认识的东西。
    fn sample() -> Vec<u8> {
        let mut out = Vec::new();
        let string = |out: &mut Vec<u8>, value: &str| {
            out.extend((value.len() as u16).to_be_bytes());
            out.extend(value.as_bytes());
        };
        out.push(TAG_COMPOUND);
        string(&mut out, "");
        out.push(TAG_LIST);
        string(&mut out, "servers");
        out.push(TAG_COMPOUND);
        out.extend(2i32.to_be_bytes());

        out.push(TAG_STRING);
        string(&mut out, "name");
        string(&mut out, "生存服");
        out.push(TAG_STRING);
        string(&mut out, "ip");
        string(&mut out, "play.example.com:25566");
        out.push(TAG_STRING);
        string(&mut out, "icon");
        string(&mut out, "iVBORw0KGgo=");
        out.push(TAG_BYTE);
        string(&mut out, "acceptTextures");
        out.push(1);
        out.push(TAG_END);

        out.push(TAG_STRING);
        string(&mut out, "ip");
        string(&mut out, "mc.example.net");
        out.push(TAG_END);

        out.push(TAG_END);
        out
    }

    #[test]
    fn reads_names_and_addresses_past_everything_else() {
        let servers = parse(&sample()).expect("parse");
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "生存服");
        assert_eq!(servers[0].address, "play.example.com:25566");
        // 没起过名字的条目也要在，界面拿地址顶上。
        assert!(servers[1].name.is_empty());
        assert_eq!(servers[1].address, "mc.example.net");
    }

    #[test]
    fn a_truncated_file_fails_instead_of_reading_past_the_end() {
        let full = sample();
        for cut in [4, 20, full.len() - 3] {
            assert!(parse(&full[..cut]).is_err(), "截断到 {cut} 字节应当失败");
        }
    }

    #[test]
    fn an_unreadable_file_is_treated_as_no_servers() {
        // 这份文件不归我们管。格式对不上时搜索该少一类结果，而不是整个失败。
        let paths = DataPaths::new(std::env::temp_dir().join("fern-servers-missing"));
        assert!(list(&paths, "nope").is_empty());
    }
}
