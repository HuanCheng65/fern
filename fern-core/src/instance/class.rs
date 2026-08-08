//! 一个 class 文件引用了什么。
//!
//! **只读常量池，不碰一条指令。** 要回答「这段代码里出现过 `Runtime.exec` 吗」，
//! 不需要反汇编：JVM 的每一次方法调用，被调者的类名和方法名都必须以
//! `Methodref` 的形式写在常量池里，字符串字面量同样如此。常量池就在文件头部，
//! 紧跟四个字节的魔数和两个版本号——读到它结束就可以停手，后面的字段、方法和
//! 属性一个都不用看。
//!
//! 于是这一层只有一件事要做对：把常量池那张表按格式走一遍。表项是变长的，
//! 每一项以一个 tag 开头，长度由 tag 决定；`Long` 和 `Double` **各占两格**，
//! 这是这张表上唯一的陷阱——漏掉它，从那一项之后的所有下标就全错了位。
//!
//! ## 读不完不是错误
//!
//! 上游只喂进来一个头部（见 `capability::HEAD`），常量池可能被截断在半路。
//! 那时候就用已经读到的那些：漏掉几条引用会少一条依据，而因为读不完就整个
//! 放弃，会连那些完整读到的也一起丢掉。

/// 一个 class 引用到的东西。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Referenced {
    /// `java/lang/Runtime.exec` 这样的方法引用。类名用 `/` 分段，是 class 文件
    /// 里的原样写法；构造函数的方法名是 `<init>`。
    pub(crate) methods: Vec<String>,
    /// 源码里写死的字符串字面量。**只取 `String` 常量指向的那些**——类名、
    /// 方法名、类型描述符也都躺在同一张表里，把它们一并算成字符串，得到的会是
    /// 一堆没人写过的「字面量」。
    pub(crate) strings: Vec<String>,
}

/// 一份 class 字节里引用到的东西。不是 class 就是空的。
pub(crate) fn referenced(bytes: &[u8]) -> Referenced {
    let mut cursor = Cursor { bytes, at: 0 };
    // 0xCAFEBABE，然后是 minor 和 major 两个版本号。
    if cursor.u4() != Some(0xCAFE_BABE) || cursor.u4().is_none() {
        return Referenced::default();
    }
    let Some(count) = cursor.u2() else {
        return Referenced::default();
    };

    // 下标从 1 开始，0 号格子空着占位，好让后面按下标直接取。
    let mut pool = vec![Slot::Empty];
    let mut index = 1u32;
    while index < u32::from(count) {
        let Some((slot, width)) = slot(&mut cursor) else {
            break;
        };
        pool.push(slot);
        // Long 和 Double 占两格，第二格谁也不许用。
        for _ in 1..width {
            pool.push(Slot::Empty);
        }
        index += width;
    }
    resolve(&pool)
}

/// 常量池里的一项。只留下用得着的那几种，其余的读过就扔——但**必须读过**，
/// 因为下一项的位置由这一项的长度决定。
enum Slot {
    Utf8(String),
    /// 指向一个 `Utf8`：类名。
    Class(u16),
    /// 指向一个 `Utf8`：字符串字面量。
    Text(u16),
    /// `Methodref` / `InterfaceMethodref`：指向一个 `Class` 和一个 `NameAndType`。
    Member(u16, u16),
    /// 指向两个 `Utf8`：方法名和类型描述符。只留方法名——同一个方法的不同重载
    /// 描述符不同，而「这段代码引用了 `Runtime.exec`」和它引用的是哪一个重载
    /// 无关。
    NameAndType(u16),
    Empty,
}

/// 读一项，返回它和它占几格。格式不认识就到此为止——继续往下读只会读到垃圾，
/// 而垃圾解析出来的「引用」会变成凭空捏造的依据。
fn slot(cursor: &mut Cursor) -> Option<(Slot, u32)> {
    let tag = cursor.u1()?;
    let slot = match tag {
        1 => Slot::Utf8(cursor.text()?),
        3 | 4 => {
            cursor.skip(4)?;
            Slot::Empty
        }
        5 | 6 => {
            cursor.skip(8)?;
            return Some((Slot::Empty, 2));
        }
        7 => Slot::Class(cursor.u2()?),
        8 => Slot::Text(cursor.u2()?),
        // Fieldref：形状和方法引用一样，但字段访问不构成一次调用。
        9 => {
            cursor.skip(4)?;
            Slot::Empty
        }
        10 | 11 => Slot::Member(cursor.u2()?, cursor.u2()?),
        12 => {
            let method = cursor.u2()?;
            cursor.skip(2)?;
            Slot::NameAndType(method)
        }
        15 => {
            cursor.skip(3)?;
            Slot::Empty
        }
        16 | 19 | 20 => {
            cursor.skip(2)?;
            Slot::Empty
        }
        17 | 18 => {
            cursor.skip(4)?;
            Slot::Empty
        }
        _ => return None,
    };
    Some((slot, 1))
}

fn resolve(pool: &[Slot]) -> Referenced {
    let text = |index: u16| match pool.get(usize::from(index)) {
        Some(Slot::Utf8(value)) => Some(value.as_str()),
        _ => None,
    };

    let mut found = Referenced::default();
    for entry in pool {
        match entry {
            Slot::Member(class, signature) => {
                let Some(Slot::Class(named)) = pool.get(usize::from(*class)) else {
                    continue;
                };
                let Some(Slot::NameAndType(method)) = pool.get(usize::from(*signature)) else {
                    continue;
                };
                if let (Some(owner), Some(method)) = (text(*named), text(*method)) {
                    found.methods.push(format!("{owner}.{method}"));
                }
            }
            Slot::Text(index) => {
                if let Some(value) = text(*index) {
                    found.strings.push(value.to_owned());
                }
            }
            _ => {}
        }
    }

    found.methods.sort();
    found.methods.dedup();
    found.strings.sort();
    found.strings.dedup();
    found
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(|_| ())
    }

    fn u1(&mut self) -> Option<u8> {
        self.take(1).map(|slice| slice[0])
    }

    fn u2(&mut self) -> Option<u16> {
        let slice = self.take(2)?;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn u4(&mut self) -> Option<u32> {
        let slice = self.take(4)?;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    /// 一个 `Utf8` 项：两字节长度加内容。
    ///
    /// 内容是 JVM 那套「修改版 UTF-8」，和标准 UTF-8 在补充平面字符和内嵌 `\0`
    /// 上不一样。这里不去还原它——我们要认的是类名、方法名和地址，那些全是
    /// ASCII，解不出来的部分变成替换字符也影响不到任何一次匹配。
    fn text(&mut self) -> Option<String> {
        let length = usize::from(self.u2()?);
        let raw = self.take(length)?;
        Some(String::from_utf8_lossy(raw).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 攒一份最小的 class 头：魔数、版本号、常量池，后面的一概不写——解析到
    /// 常量池结束就停手，所以「后面什么都没有」正是它该能处理的情况。
    struct Pool {
        bytes: Vec<u8>,
        count: u16,
    }

    impl Pool {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                count: 1,
            }
        }

        fn utf8(&mut self, value: &str) -> u16 {
            self.bytes.push(1);
            self.bytes
                .extend_from_slice(&(value.len() as u16).to_be_bytes());
            self.bytes.extend_from_slice(value.as_bytes());
            self.slot(1)
        }

        fn class(&mut self, name: u16) -> u16 {
            self.bytes.push(7);
            self.bytes.extend_from_slice(&name.to_be_bytes());
            self.slot(1)
        }

        fn text(&mut self, value: u16) -> u16 {
            self.bytes.push(8);
            self.bytes.extend_from_slice(&value.to_be_bytes());
            self.slot(1)
        }

        fn name_and_type(&mut self, name: u16, descriptor: u16) -> u16 {
            self.bytes.push(12);
            self.bytes.extend_from_slice(&name.to_be_bytes());
            self.bytes.extend_from_slice(&descriptor.to_be_bytes());
            self.slot(1)
        }

        fn method(&mut self, class: u16, signature: u16) -> u16 {
            self.bytes.push(10);
            self.bytes.extend_from_slice(&class.to_be_bytes());
            self.bytes.extend_from_slice(&signature.to_be_bytes());
            self.slot(1)
        }

        fn long(&mut self, value: i64) -> u16 {
            self.bytes.push(5);
            self.bytes.extend_from_slice(&value.to_be_bytes());
            self.slot(2)
        }

        fn slot(&mut self, width: u16) -> u16 {
            let index = self.count;
            self.count += width;
            index
        }

        fn finish(&self) -> Vec<u8> {
            let mut out = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
            out.extend_from_slice(&self.count.to_be_bytes());
            out.extend_from_slice(&self.bytes);
            out
        }
    }

    #[test]
    fn a_method_reference_comes_back_as_owner_and_name() {
        let mut pool = Pool::new();
        let owner = pool.utf8("java/lang/Runtime");
        let name = pool.utf8("exec");
        let descriptor = pool.utf8("(Ljava/lang/String;)Ljava/lang/Process;");
        let class = pool.class(owner);
        let signature = pool.name_and_type(name, descriptor);
        pool.method(class, signature);

        let found = referenced(&pool.finish());
        assert_eq!(found.methods, vec!["java/lang/Runtime.exec".to_owned()]);
    }

    #[test]
    fn only_string_constants_count_as_strings() {
        let mut pool = Pool::new();
        // 这一条既是类名又是 Utf8，但没有 `String` 常量指向它。
        let owner = pool.utf8("net/example/Mod");
        pool.class(owner);
        let literal = pool.utf8("http://example.invalid/payload");
        pool.text(literal);

        let found = referenced(&pool.finish());
        assert_eq!(
            found.strings,
            vec!["http://example.invalid/payload".to_owned()]
        );
    }

    /// 这张表上唯一的陷阱：Long 占两格。数错一格，它后面每一条引用的下标都会
    /// 指向别处——解析不会报错，只会安静地给出一份错的答案。
    #[test]
    fn a_long_takes_two_slots_and_what_follows_still_lines_up() {
        let mut pool = Pool::new();
        pool.long(1234);
        let owner = pool.utf8("java/lang/ProcessBuilder");
        let name = pool.utf8("start");
        let descriptor = pool.utf8("()Ljava/lang/Process;");
        let class = pool.class(owner);
        let signature = pool.name_and_type(name, descriptor);
        pool.method(class, signature);

        let found = referenced(&pool.finish());
        assert_eq!(
            found.methods,
            vec!["java/lang/ProcessBuilder.start".to_owned()]
        );
    }

    #[test]
    fn a_pool_cut_off_halfway_keeps_what_was_already_read() {
        let mut pool = Pool::new();
        let owner = pool.utf8("java/lang/Runtime");
        let name = pool.utf8("exec");
        let descriptor = pool.utf8("()V");
        let class = pool.class(owner);
        let signature = pool.name_and_type(name, descriptor);
        pool.method(class, signature);
        // 声称后面还有一百项，其实一个字节都没有。
        pool.count += 100;

        let found = referenced(&pool.finish());
        assert_eq!(found.methods, vec!["java/lang/Runtime.exec".to_owned()]);
    }

    #[test]
    fn something_that_is_not_a_class_file_says_nothing() {
        assert_eq!(referenced(b"PK\x03\x04not a class"), Referenced::default());
        assert_eq!(referenced(&[]), Referenced::default());
    }
}
