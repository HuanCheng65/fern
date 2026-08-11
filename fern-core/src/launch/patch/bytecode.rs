//! 一个**受限**的 class 改写器。
//!
//! 它只会做一件事：把某个方法体里的一条 `invokestatic` 换成一段语义等价的
//! 直线代码。做不到就报错，绝不猜——见 [`super`] 开头说的失败模式。
//!
//! 硬天花板是**不接受带跳转的方法**。一旦插入的代码里有分支，或者原方法里
//! 本来就有分支，字节偏移一变，StackMapTable 的每一帧都要重算，而算错的表
//! 会让 JVM 在加载类的那一刻抛 `VerifyError`——那已经是另一个量级的工程。
//! 所以这里宁可拒绝：文档 §3.3 说得很清楚，**第二条需要分支的补丁出现时，
//! 就引入一个用 ASM 写的安装期工具**，而不是把这个文件写复杂。
//!
//! 拒绝的判据全部是保守的：有异常表、有跳转、有 `tableswitch` /
//! `lookupswitch` / `wide`、目标调用不是恰好一处，任何一条都直接失败。

use anyhow::{Result, anyhow, bail};

/// 常量池里的一项。`body` 含 tag 那一字节，原样写回。
#[derive(Debug, Clone)]
struct Constant {
    tag: u8,
    body: Vec<u8>,
}

/// 类文件里除常量池以外的部分原样留着，只在需要的地方动。
pub(crate) struct ClassFile {
    /// magic + minor + major。
    header: Vec<u8>,
    /// 索引从 1 起，`long` / `double` 占两格，第二格是 `None`。
    constants: Vec<Option<Constant>>,
    access_flags: u16,
    this_class: u16,
    super_class: u16,
    interfaces: Vec<u16>,
    fields: Vec<Member>,
    methods: Vec<Member>,
    attributes: Vec<Attribute>,
}

#[derive(Debug, Clone)]
struct Member {
    access_flags: u16,
    name: u16,
    descriptor: u16,
    attributes: Vec<Attribute>,
}

#[derive(Debug, Clone)]
struct Attribute {
    name: u16,
    body: Vec<u8>,
}

/// 一条方法引用：所有者、名字、描述符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MethodRef {
    pub owner: &'static str,
    pub name: &'static str,
    pub descriptor: &'static str,
    /// 接口方法走 `invokeinterface`，常量池的 tag 也不一样。
    pub interface: bool,
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| anyhow!("class 文件在偏移 {} 处提前结束", self.offset))?;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

impl ClassFile {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let header = reader.take(8)?.to_vec();
        if header[..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
            bail!("不是一个 class 文件");
        }

        let count = reader.u16()?;
        let mut constants: Vec<Option<Constant>> = vec![None];
        while constants.len() < count as usize {
            let tag = reader.u8()?;
            let payload = match tag {
                1 => {
                    let length = reader.u16()?;
                    let mut body = length.to_be_bytes().to_vec();
                    body.extend_from_slice(reader.take(length as usize)?);
                    body
                }
                7 | 8 | 16 | 19 | 20 => reader.take(2)?.to_vec(),
                15 => reader.take(3)?.to_vec(),
                3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => reader.take(4)?.to_vec(),
                5 | 6 => reader.take(8)?.to_vec(),
                other => bail!("常量池里有未知的项：tag {other}"),
            };
            let mut body = vec![tag];
            body.extend_from_slice(&payload);
            let wide = matches!(tag, 5 | 6);
            constants.push(Some(Constant { tag, body }));
            // long 和 double 占两格，第二格永远空着。
            if wide {
                constants.push(None);
            }
        }

        let access_flags = reader.u16()?;
        let this_class = reader.u16()?;
        let super_class = reader.u16()?;
        let interface_count = reader.u16()?;
        let mut interfaces = Vec::with_capacity(interface_count as usize);
        for _ in 0..interface_count {
            interfaces.push(reader.u16()?);
        }
        let fields = read_members(&mut reader)?;
        let methods = read_members(&mut reader)?;
        let attributes = read_attributes(&mut reader)?;
        if reader.offset != bytes.len() {
            bail!(
                "class 文件末尾有 {} 字节读不懂",
                bytes.len() - reader.offset
            );
        }

        Ok(Self {
            header,
            constants,
            access_flags,
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
        })
    }

    pub(crate) fn serialize(&self) -> Vec<u8> {
        let mut output = self.header.clone();
        push_u16(&mut output, self.constants.len() as u16);
        for constant in self.constants.iter().flatten() {
            output.extend_from_slice(&constant.body);
        }
        push_u16(&mut output, self.access_flags);
        push_u16(&mut output, self.this_class);
        push_u16(&mut output, self.super_class);
        push_u16(&mut output, self.interfaces.len() as u16);
        for interface in &self.interfaces {
            push_u16(&mut output, *interface);
        }
        write_members(&mut output, &self.fields);
        write_members(&mut output, &self.methods);
        write_attributes(&mut output, &self.attributes);
        output
    }

    fn utf8(&self, index: u16) -> Option<&str> {
        let constant = self.constants.get(index as usize)?.as_ref()?;
        if constant.tag != 1 {
            return None;
        }
        std::str::from_utf8(&constant.body[3..]).ok()
    }

    fn add(&mut self, tag: u8, payload: Vec<u8>) -> Result<u16> {
        // 先找现成的，免得同一个字符串进去十遍。
        let mut body = vec![tag];
        body.extend_from_slice(&payload);
        if let Some(index) = self
            .constants
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|constant| constant.body == body))
        {
            return Ok(index as u16);
        }
        let index = u16::try_from(self.constants.len())
            .map_err(|_| anyhow!("常量池已经满了，放不下新的项"))?;
        self.constants.push(Some(Constant { tag, body }));
        Ok(index)
    }

    fn add_utf8(&mut self, text: &str) -> Result<u16> {
        let raw = text.as_bytes();
        let mut payload = u16::try_from(raw.len())
            .map_err(|_| anyhow!("常量太长"))?
            .to_be_bytes()
            .to_vec();
        payload.extend_from_slice(raw);
        self.add(1, payload)
    }

    fn add_class(&mut self, name: &str) -> Result<u16> {
        let name = self.add_utf8(name)?;
        self.add(7, name.to_be_bytes().to_vec())
    }

    fn add_method_ref(&mut self, method: &MethodRef) -> Result<u16> {
        let class = self.add_class(method.owner)?;
        let name = self.add_utf8(method.name)?;
        let descriptor = self.add_utf8(method.descriptor)?;
        let mut name_and_type = name.to_be_bytes().to_vec();
        name_and_type.extend_from_slice(&descriptor.to_be_bytes());
        let name_and_type = self.add(12, name_and_type)?;
        let mut payload = class.to_be_bytes().to_vec();
        payload.extend_from_slice(&name_and_type.to_be_bytes());
        self.add(if method.interface { 11 } else { 10 }, payload)
    }

    /// 常量池里已有的那条方法引用，没有就是 `None`——这个类根本没调用过它。
    fn find_method_ref(&self, method: &MethodRef) -> Option<u16> {
        let want_tag = if method.interface { 11 } else { 10 };
        self.constants
            .iter()
            .position(|slot| {
                let Some(constant) = slot else { return false };
                if constant.tag != want_tag {
                    return false;
                }
                let class = u16::from_be_bytes([constant.body[1], constant.body[2]]);
                let name_and_type = u16::from_be_bytes([constant.body[3], constant.body[4]]);
                let Some(class) = self.constants.get(class as usize).and_then(Option::as_ref)
                else {
                    return false;
                };
                let owner = self.utf8(u16::from_be_bytes([class.body[1], class.body[2]]));
                let Some(pair) = self
                    .constants
                    .get(name_and_type as usize)
                    .and_then(Option::as_ref)
                else {
                    return false;
                };
                let name = self.utf8(u16::from_be_bytes([pair.body[1], pair.body[2]]));
                let descriptor = self.utf8(u16::from_be_bytes([pair.body[3], pair.body[4]]));
                owner == Some(method.owner)
                    && name == Some(method.name)
                    && descriptor == Some(method.descriptor)
            })
            .map(|index| index as u16)
    }

    /// 某个方法的方法体，测试用。
    #[cfg(test)]
    fn code_of(&self, method_name: &str) -> Vec<u8> {
        let method = self
            .methods
            .iter()
            .find(|method| self.utf8(method.name) == Some(method_name))
            .expect("方法在");
        let code = method
            .attributes
            .iter()
            .find(|attribute| self.utf8(attribute.name) == Some("Code"))
            .expect("方法体在");
        let length = u32::from_be_bytes([code.body[4], code.body[5], code.body[6], code.body[7]]);
        code.body[8..8 + length as usize].to_vec()
    }
}

fn read_members(reader: &mut Reader<'_>) -> Result<Vec<Member>> {
    let count = reader.u16()?;
    let mut members = Vec::with_capacity(count as usize);
    for _ in 0..count {
        members.push(Member {
            access_flags: reader.u16()?,
            name: reader.u16()?,
            descriptor: reader.u16()?,
            attributes: read_attributes(reader)?,
        });
    }
    Ok(members)
}

fn write_members(output: &mut Vec<u8>, members: &[Member]) {
    push_u16(output, members.len() as u16);
    for member in members {
        push_u16(output, member.access_flags);
        push_u16(output, member.name);
        push_u16(output, member.descriptor);
        write_attributes(output, &member.attributes);
    }
}

fn read_attributes(reader: &mut Reader<'_>) -> Result<Vec<Attribute>> {
    let count = reader.u16()?;
    let mut attributes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = reader.u16()?;
        let length = reader.u32()? as usize;
        attributes.push(Attribute {
            name,
            body: reader.take(length)?.to_vec(),
        });
    }
    Ok(attributes)
}

fn write_attributes(output: &mut Vec<u8>, attributes: &[Attribute]) {
    push_u16(output, attributes.len() as u16);
    for attribute in attributes {
        push_u16(output, attribute.name);
        output.extend_from_slice(&(attribute.body.len() as u32).to_be_bytes());
        output.extend_from_slice(&attribute.body);
    }
}

/// 指令长度表：值是「操作码 + 操作数」的总字节数。
///
/// `tableswitch`、`lookupswitch`、`wide` 长度不定，不在表里——碰上就拒绝，
/// 它们出现在这类补丁的目标方法里的概率本来就是零。
fn instruction_length(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0xAA | 0xAB | 0xC4 => return None,
        0x10 | 0x12 | 0x15..=0x19 | 0x36..=0x3A | 0xA9 | 0xBC => 2,
        0x11
        | 0x13
        | 0x14
        | 0x84
        | 0x99..=0xA8
        | 0xB2..=0xB8
        | 0xBB
        | 0xBD
        | 0xC0
        | 0xC1
        | 0xC6
        | 0xC7 => 3,
        0xC5 => 4,
        0xB9 | 0xBA | 0xC8 | 0xC9 => 5,
        0x00..=0xC9 => 1,
        // 0xCA 之后是调试器和实现私有的操作码，正常的 class 里不会有。
        _ => return None,
    })
}

/// 会改变控制流的那些指令。带上它们中任何一条，偏移一变就要重算
/// StackMapTable。`ret` 一并算进来：它只跟 `jsr` 成对出现。
fn is_jump(opcode: u8) -> bool {
    matches!(opcode, 0x99..=0xA9 | 0xC6..=0xC9)
}

/// 一段要插进去的直线代码。
pub(crate) struct Splice {
    bytes: Vec<u8>,
    /// 相对原指令，栈最深要多用几格。
    extra_stack: u16,
    /// 要占用几个新的局部变量槽。
    extra_locals: u16,
}

/// 把 `sortTweakList` 那一句 `Collections.sort(list, cmp)` 换成拷一份排完
/// 再写回。见 [`super::forge`] 里那条补丁的说明。
///
/// 返回 `Ok(None)` 表示这个类里根本没有那一句——上游已经修过了，不该改。
pub(crate) fn replace_call(
    bytes: &[u8],
    method_name: &str,
    target: &MethodRef,
    build: impl FnOnce(&mut ClassFile, u16) -> Result<Splice>,
) -> Result<Option<Vec<u8>>> {
    let mut class = ClassFile::parse(bytes)?;
    let Some(target_index) = class.find_method_ref(target) else {
        return Ok(None);
    };
    let Some(position) = class
        .methods
        .iter()
        .position(|method| class.utf8(method.name) == Some(method_name))
    else {
        return Ok(None);
    };

    let Some(code_position) = class.methods[position]
        .attributes
        .iter()
        .position(|attribute| class.utf8(attribute.name) == Some("Code"))
    else {
        bail!("{method_name} 没有方法体");
    };
    let code_attribute = class.methods[position].attributes[code_position].clone();
    let mut reader = Reader::new(&code_attribute.body);
    let max_stack = reader.u16()?;
    let max_locals = reader.u16()?;
    let length = reader.u32()? as usize;
    let code = reader.take(length)?.to_vec();
    let exception_entries = reader.u16()?;
    if exception_entries != 0 {
        bail!("{method_name} 带异常表，改了偏移会错位");
    }

    // 逐条走一遍再决定动不动：长度表对不上、有跳转、有 switch，一律不动。
    let mut offset = 0;
    let mut occurrences = Vec::new();
    while offset < code.len() {
        let opcode = code[offset];
        let size = instruction_length(opcode)
            .ok_or_else(|| anyhow!("{method_name} 里有长度不定的指令 {opcode:#04x}"))?;
        if is_jump(opcode) {
            bail!("{method_name} 里有跳转指令 {opcode:#04x}，改了要重算 StackMapTable");
        }
        if opcode == 0xB8
            && offset + 3 <= code.len()
            && u16::from_be_bytes([code[offset + 1], code[offset + 2]]) == target_index
        {
            occurrences.push(offset);
        }
        offset += size;
    }
    if offset != code.len() {
        bail!("{method_name} 的最后一条指令越过了方法体末尾");
    }
    match occurrences.len() {
        0 => return Ok(None),
        1 => {}
        found => bail!("{method_name} 里有 {found} 处 {}，预期一处", target.name),
    }

    let slot = max_locals;
    let splice = build(&mut class, slot)?;
    let at = occurrences[0];
    let mut patched = Vec::with_capacity(code.len() + splice.bytes.len());
    patched.extend_from_slice(&code[..at]);
    patched.extend_from_slice(&splice.bytes);
    patched.extend_from_slice(&code[at + 3..]);

    let mut body = Vec::with_capacity(patched.len() + 12);
    push_u16(&mut body, max_stack + splice.extra_stack);
    push_u16(&mut body, max_locals + splice.extra_locals);
    body.extend_from_slice(&(patched.len() as u32).to_be_bytes());
    body.extend_from_slice(&patched);
    push_u16(&mut body, 0); // 异常表，上面已经确认是空的
    // Code 的子属性全丢掉：行号表和局部变量表记的偏移已经不对了，而
    // StackMapTable 在一个没有跳转的方法里本来就没有帧。留着错的比没有更糟。
    push_u16(&mut body, 0);

    let name = class.add_utf8("Code")?;
    class.methods[position].attributes[code_position] = Attribute { name, body };
    Ok(Some(class.serialize()))
}

/// `list.toArray()` → `Arrays.sort(a, cmp)` → `Collections.copy(list, asList(a))`。
///
/// 进来时栈顶是 `[…, list, cmp]`，和 `Collections.sort(List, Comparator)` 一
/// 样；出去时栈回到 `[…]`。全程没有跳转。
pub(crate) fn copy_sort_copy_back(class: &mut ClassFile, slot: u16) -> Result<Splice> {
    let to_array = class.add_method_ref(&MethodRef {
        owner: "java/util/List",
        name: "toArray",
        descriptor: "()[Ljava/lang/Object;",
        interface: true,
    })?;
    let sort = class.add_method_ref(&MethodRef {
        owner: "java/util/Arrays",
        name: "sort",
        descriptor: "([Ljava/lang/Object;Ljava/util/Comparator;)V",
        interface: false,
    })?;
    let as_list = class.add_method_ref(&MethodRef {
        owner: "java/util/Arrays",
        name: "asList",
        descriptor: "([Ljava/lang/Object;)Ljava/util/List;",
        interface: false,
    })?;
    let copy = class.add_method_ref(&MethodRef {
        owner: "java/util/Collections",
        name: "copy",
        descriptor: "(Ljava/util/List;Ljava/util/List;)V",
        interface: false,
    })?;

    let slot = u8::try_from(slot).map_err(|_| anyhow!("局部变量槽超过 255，装不进一字节操作数"))?;
    let mut bytes = Vec::with_capacity(20);
    bytes.extend_from_slice(&[0x3A, slot]); // astore slot        cmp
    bytes.push(0x59); // dup                list, list
    bytes.push(0xB9); // invokeinterface    list, array
    bytes.extend_from_slice(&to_array.to_be_bytes());
    bytes.extend_from_slice(&[1, 0]); // 参数槽数 + 保留字节
    bytes.push(0x59); // dup                list, array, array
    bytes.extend_from_slice(&[0x19, slot]); // aload slot   …, cmp
    bytes.push(0xB8); // invokestatic Arrays.sort
    bytes.extend_from_slice(&sort.to_be_bytes());
    bytes.push(0xB8); // invokestatic Arrays.asList
    bytes.extend_from_slice(&as_list.to_be_bytes());
    bytes.push(0xB8); // invokestatic Collections.copy
    bytes.extend_from_slice(&copy.to_be_bytes());

    Ok(Splice {
        bytes,
        // 最深的一刻是 aload 之后：list, array, array, cmp——比原来那一句
        // 调用时的深度多两格。
        extra_stack: 2,
        extra_locals: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手搓一个最小的 class：一个 `sortTweakList()` 方法，里面调一次
    /// `Collections.sort(List, Comparator)`。
    fn synthetic(with_jump: bool) -> Vec<u8> {
        let mut constants: Vec<Vec<u8>> = Vec::new();
        let utf8 = |text: &str| {
            let mut body = vec![1u8];
            body.extend_from_slice(&(text.len() as u16).to_be_bytes());
            body.extend_from_slice(text.as_bytes());
            body
        };
        constants.push(utf8("Example")); // 1
        constants.push({
            let mut body = vec![7u8];
            body.extend_from_slice(&1u16.to_be_bytes());
            body
        }); // 2 = class Example
        constants.push(utf8("java/lang/Object")); // 3
        constants.push({
            let mut body = vec![7u8];
            body.extend_from_slice(&3u16.to_be_bytes());
            body
        }); // 4 = class Object
        constants.push(utf8("java/util/Collections")); // 5
        constants.push({
            let mut body = vec![7u8];
            body.extend_from_slice(&5u16.to_be_bytes());
            body
        }); // 6
        constants.push(utf8("sort")); // 7
        constants.push(utf8("(Ljava/util/List;Ljava/util/Comparator;)V")); // 8
        constants.push({
            let mut body = vec![12u8];
            body.extend_from_slice(&7u16.to_be_bytes());
            body.extend_from_slice(&8u16.to_be_bytes());
            body
        }); // 9
        constants.push({
            let mut body = vec![10u8];
            body.extend_from_slice(&6u16.to_be_bytes());
            body.extend_from_slice(&9u16.to_be_bytes());
            body
        }); // 10 = Collections.sort
        constants.push(utf8("sortTweakList")); // 11
        constants.push(utf8("()V")); // 12
        constants.push(utf8("Code")); // 13

        // aconst_null, aconst_null, invokestatic #10, return
        let mut code = vec![0x01, 0x01, 0xB8, 0x00, 0x0A];
        if with_jump {
            // goto +3：一条无害的跳转，只为了让改写器拒绝。
            code.extend_from_slice(&[0xA7, 0x00, 0x03]);
        }
        code.push(0xB1);

        let mut body = Vec::new();
        body.extend_from_slice(&4u16.to_be_bytes()); // max_stack
        body.extend_from_slice(&1u16.to_be_bytes()); // max_locals
        body.extend_from_slice(&(code.len() as u32).to_be_bytes());
        body.extend_from_slice(&code);
        body.extend_from_slice(&0u16.to_be_bytes()); // 异常表
        body.extend_from_slice(&0u16.to_be_bytes()); // 子属性

        let mut out = vec![0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x32];
        out.extend_from_slice(&((constants.len() + 1) as u16).to_be_bytes());
        for constant in &constants {
            out.extend_from_slice(constant);
        }
        out.extend_from_slice(&0x0021u16.to_be_bytes()); // access
        out.extend_from_slice(&2u16.to_be_bytes()); // this
        out.extend_from_slice(&4u16.to_be_bytes()); // super
        out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        out.extend_from_slice(&0u16.to_be_bytes()); // fields
        out.extend_from_slice(&1u16.to_be_bytes()); // methods
        out.extend_from_slice(&0x0009u16.to_be_bytes());
        out.extend_from_slice(&11u16.to_be_bytes());
        out.extend_from_slice(&12u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // 一个属性
        out.extend_from_slice(&13u16.to_be_bytes());
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&0u16.to_be_bytes()); // 类属性
        out
    }

    fn collections_sort() -> MethodRef {
        MethodRef {
            owner: "java/util/Collections",
            name: "sort",
            descriptor: "(Ljava/util/List;Ljava/util/Comparator;)V",
            interface: false,
        }
    }

    /// 读进来再写回去，一个字节都不该变——不成立的话，任何改写都是在赌。
    #[test]
    fn parsing_and_writing_a_class_is_lossless() {
        let bytes = synthetic(false);
        let class = ClassFile::parse(&bytes).expect("parse");
        assert_eq!(class.serialize(), bytes);
    }

    #[test]
    fn the_offending_call_is_replaced_by_a_copy_and_sort() {
        let bytes = synthetic(false);
        let patched = replace_call(
            &bytes,
            "sortTweakList",
            &collections_sort(),
            copy_sort_copy_back,
        )
        .expect("patch")
        .expect("这个类里有那一句");

        let class = ClassFile::parse(&patched).expect("改完还要能读回来");
        // 换进去的三个调用都在常量池里。
        for method in [
            MethodRef {
                owner: "java/util/List",
                name: "toArray",
                descriptor: "()[Ljava/lang/Object;",
                interface: true,
            },
            MethodRef {
                owner: "java/util/Arrays",
                name: "sort",
                descriptor: "([Ljava/lang/Object;Ljava/util/Comparator;)V",
                interface: false,
            },
            MethodRef {
                owner: "java/util/Collections",
                name: "copy",
                descriptor: "(Ljava/util/List;Ljava/util/List;)V",
                interface: false,
            },
        ] {
            assert!(
                class.find_method_ref(&method).is_some(),
                "{} 应该在常量池里",
                method.name
            );
        }
        // 20 字节换 3 字节，方法体只长这么多。
        let before = ClassFile::parse(&bytes).expect("parse");
        assert_eq!(
            class.code_of("sortTweakList").len(),
            before.code_of("sortTweakList").len() + 17
        );
        // 原来那一句必须真的没了。
        let target = before
            .find_method_ref(&collections_sort())
            .expect("原来在常量池里");
        let needle = [0xB8, target.to_be_bytes()[0], target.to_be_bytes()[1]];
        assert!(
            !class
                .code_of("sortTweakList")
                .windows(3)
                .any(|window| window == needle)
        );
    }

    /// 上游已经修过的那些版本里没有这一句，那就什么都不该做。
    #[test]
    fn a_class_without_the_call_is_left_alone() {
        let bytes = synthetic(false);
        let other = MethodRef {
            owner: "java/util/Collections",
            name: "shuffle",
            descriptor: "(Ljava/util/List;)V",
            interface: false,
        };
        assert!(
            replace_call(&bytes, "sortTweakList", &other, copy_sort_copy_back)
                .expect("不该报错")
                .is_none()
        );
        assert!(
            replace_call(
                &bytes,
                "somethingElse",
                &collections_sort(),
                copy_sort_copy_back
            )
            .expect("不该报错")
            .is_none()
        );
    }

    /// 有跳转就得拒绝，而不是改出一份 StackMapTable 对不上的 class。
    #[test]
    fn a_method_with_a_branch_is_refused() {
        let error = replace_call(
            &synthetic(true),
            "sortTweakList",
            &collections_sort(),
            copy_sort_copy_back,
        )
        .expect_err("带跳转的方法必须被拒绝");
        assert!(error.to_string().contains("跳转"), "{error}");
    }
}
