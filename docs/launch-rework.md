# 启动链路的重构、改造与兼容

> 范围：让 Fern 覆盖 Mojang 元数据能给出的全部历史版本，以及主流历史加载器；
> 并且能启动别的启动器建出来的复杂实例。
>
> 这是一份**计划**，不是现状描述。现状在
> [launcher-core-dev.md](launcher-core-dev.md)，那份里写的是「现在如此」，
> 这份里写的是「打算如此」。第 1 章是两者的交界：实测出来的基线。
>
> 每条事实标了证据强度：**实测**（本仓库跑出来的）、**上游**（issue 或官方
> 文档）、**未核实**（听说，动手前要先验）。**不要把未核实的当成前提去设计。**

---

## 1. 现状：实测出来的覆盖面

2026-08-11 在 Linux x86_64、无显示器的机器上逐个真启动。判据是**停在哪一
步**：停在「打不开显示」说明它前面的每一环（Java、classpath、natives、资源、
参数、变量替换）都对了，换一台有显示器的机器就会开窗口。

| 版本 | 时代 | 停在哪 | 结论 |
|---|---|---|---|
| c0.30_01c | Classic / Applet | `MinecraftApplet.<init>` → `HeadlessException` | 通 |
| b1.7.3 | Legacy Desktop | `Minecraft.main` → `HeadlessException` | 通 |
| 1.5.2 | Legacy Desktop | 同上；`resources/` 摆出 49 MB | 通 |
| 1.6.4 | Launcher 1.x | `Setting user` → LWJGL 要 X display | 通 |
| 1.7.10 Forge | LaunchWrapper | `Setting user` → LWJGL 2.9.1 要 X display | 通 |
| 1.12.2 Forge | LaunchWrapper | FML 14.23.5.2864 → LWJGL 2.9.4 要 X display | 通 |
| 1.21.1 Fabric | Modern | LWJGL 3.3.3 → GLFW 找不到平台 | 通 |
| 1.7.2 Forge | LaunchWrapper | 见 §3.1，要打补丁才通 | 需补丁 |

2026-08-11 补测（第 2、3 步做完之后，同一台机器）：

| 版本 | 停在哪 | 结论 |
|---|---|---|
| 1.7.2 Forge | `Setting user` → LWJGL 2.9.0 要 X display | 通（补丁生效） |
| 1.16.5 Forge 36.2.39 | FML → GLFW 起不来 | 通（另见下） |

1.16.5 Forge 顺带发现两件事，都已经修掉：安装器在自己的 `maven/` 下带着
`net.minecraftforge:forge:<版本>`（那个 212 KB 的 jar，`fmlclient` 这个启动目标
在里面），而它只写在版本描述的库清单里、不在 `install_profile` 的那一份里——只看
后者就会少掉它，报出来是「Cannot find launch target fmlclient」；以及这台机器上
的系统 Java 是 8u492，落在 §4.3 那条线之外，现在会自动改用 `jre-legacy`。

**结论一：六个时代的原版现在就已经全部覆盖，走的是同一条流水线，没有任何
legacy adapter。** 原因是 Mojang 已经把老版本的元数据回填成了现代格式——
c0.30_01c 今天也有 `libraries`、`assetIndex`、`downloads.client`。MultiMC 那套
`instMods/`、全局 `lwjgl/2.9.0`、applet 特判，是官方还没回填时的产物。

**结论二：唯一必须自己做的历史适配是旧资源布局**，而 `virtual` 与
`map_to_resources` 两条我们都在跑（见 `prepare::materialize_legacy_assets`）。

已经在做、后面不要重复造的东西：

- natives 完全由元数据决定，没有版本号特判（`Library::file` 返回的
  `LibraryFile.native`）。
- rules 求值含 `os.name` / `os.arch` / `os.version` 正则 / features。
- 参数按字段能力分叉：有 `arguments` 走结构化，有 `minecraftArguments` 走旧的。
- `LaunchPlan` 已经是归一点，原版与各家加载器最后都落到它。
- 26.1 起的原版不干预 GC，用 Mojang 自己的调校。
- Forge 安装读 `install_profile` / `processors` 的语义，不猜 ZIP 结构。
- 崩溃识别已经是数据表（`fern-core/rules/crash.toml` + fixture + 文案表）。

---

## 2. 组件模型

### 2.1 为什么必须做

**驱动力不是「一个实例装两个加载器」，是外部实例。** Prism / MultiMC 的
`mmc-pack.json` + `patches/*.json` 本身就是一份有序组件表。我们的模型如果是
「一个游戏版本 + 一个加载器」，导入只能把它压平成一份合并好的 JSON——能启动，
但从此改不动：换加载器版本、加一个组件、删掉某个 patch，全都做不了。

多加载器共存（Forge + LiteLoader）是顺带解决的，不是理由。

### 2.2 要改什么

| 位置 | 现在 | 改成 |
|---|---|---|
| `InstanceProfile` | `loader: LoaderKind` + `loader_profile: Option<LoaderProfile>` | 有序的 `components: Vec<Component>`；schema 升版并迁移旧实例 |
| `version::effective_id` | 返回「那一个版本 id」 | 取消。不再存在单一 id |
| `version::resolve` | 跟 `inheritsFrom` 链 | 按序合并组件列表；两级继承是它的特例 |
| `java::requirement(_, loader, _)` | 单个 `LoaderKind` | 组件集合 |
| `crash.toml` 的 `loader = [...]` 守卫 | 单个加载器 | 组件集合 |
| 实例设置界面 | 加载器单选 | 可增删的组件表 |

`Component` 至少要表达：

```
id / version / 顺序
mainClass                 有则覆盖
libraries[]               合并
tweakers[]                有序追加
jvm_args / game_args      追加（带 rules）
jar_transforms[]          见 §3
runtime 约束              下限、上限、update 粒度（见 §4）
install 配方              见 §5
```

### 2.3 合并语义要逐字段定死

现在是「子整体覆盖父」。组件化之后必须逐字段说清楚，否则叠加的结果不可预测：

| 字段 | 策略 |
|---|---|
| `mainClass` | 最后一个有值的赢 |
| `libraries` | 合并后按坐标去重，留版本高的（已实现，见 `effective_libraries`） |
| `assetIndex` / `downloads` / `logging` | 最后一个有值的赢；通常只有原版有 |
| `arguments.jvm` / `arguments.game` | 按序追加，不去重 |
| **`minecraftArguments`** | **不能整串覆盖**，见下 |
| `javaVersion` | 取交集，冲突时报错而不是猜 |

**`minecraftArguments` 是叠加的唯一硬门槛。** 它是一整串字符串，现在子覆盖
父。两个 LaunchWrapper 系加载器各自带 `--tweakClass`，后一个会把前一个整串吃
掉。要把它拆成「基础参数 + 有序 tweaker 列表」两部分：基础参数按 key 覆盖，
tweaker **追加、保序、不去重**（LaunchWrapper 自己按顺序加载，顺序有语义）。

这一步很小，不改任何现有行为，而且是组件化和 LiteLoader 的共同前置，**建议先
单独做掉**。

---

## 3. 产物变换流水线

### 3.1 为什么需要：1.7.2 Forge 的两个坑（实测）

**坑一，`ConcurrentModificationException`。** FML 7.2.x 的
`CoreModManager.sortTweakList()` 直接对 LaunchWrapper 正在用迭代器遍历的那张
tweaker 表做原地排序：

```
Collections.sort(list, cmp)      // Java 8u20 起委托给 List.sort，原地排会动 modCount
```

于是 LaunchWrapper 的 `it.remove()` 抛 CME。**上游**：`Collections.sort` 委托给
`List.sort` 是 8u20 引入的（写规则时把对应的 JDK bug 号补上）。**实测**：反编译
对比过 1.7.10 的同一个方法，Forge 自己在那一版改成了
`toArray` → `Arrays.sort` → `List.set`。
1.7.9 及更早没有再发版。

**实测**：现存的每一个 LaunchWrapper（1.7 / 1.8 / 1.9 / 1.11 / 1.12）都是「边
遍历边删」，换版本无用。各代 Forge 钉的版本：1.6.4 → 1.8，1.7.2 → 1.9，
1.7.10 / 1.8.9 / 1.11.2 → 1.12。

补丁形态（**实测可行**）：把那一句换成语义等价、但不动 `modCount` 的写法。

```
Object[] a = list.toArray();
Arrays.sort(a, cmp);
Collections.copy(list, Arrays.asList(a));
```

字节码上是 20 字节换 3 字节，**全是直线代码**——不需要重算 StackMapTable，不需
要新增类。原方法本来就没有跳转和异常表。

**坑二，FML 的防篡改检查。** 打完补丁之后 FML 报「CRITICAL TAMPERING」，主语是
**client jar**（我们一个字节都没动，是按 Mojang 清单 sha1 校验过的）。真正原因
是那份 2013 年的 `MOJANGCS` 签名是 SHA-1 的，现代 JVM 一律当作未签名，FML 拿到
`0 certificates`。加上官方开关 `-Dfml.ignoreInvalidMinecraftCertificates=true`
即可。**这个开关在我们这里站得住**：jar 的完整性由我们按 Mojang 的 sha1 保证，
比被绕过的那个检查更强。

两条都做上之后（**实测**）：FML 走到 `Launching wrapped minecraft` →
`Setting user` → 停在打不开显示。1.7.2 Forge 是可以救活的。

### 3.2 安装期改写，不用运行时 agent

结论：**在安装期产出补丁产物，不用 `-javaagent` 做运行时字节码变换。**

决定性的一条是**失败模式**：`ClassFileTransformer` 抛异常会被 JVM 吞掉，然后用
原始字节继续加载——补丁没打上时，现象和没有补丁完全一样，日志里没有任何线索。
安装期改写失败则是一个普通的错误，带得上「哪个 jar、哪个方法、为什么拒绝」，
而且游戏根本不会被拉起来。

其余理由：产物有 hash，能 `javap`、能 diff、能写测试断言改写结果；每个实例只做
一次而不是每次启动都做一次；不往命令行里塞 `-javaagent`（它会进崩溃报告、要和
authlib-injector 的 agent 排序、和 Mixin / ModLauncher 自己的 instrumentation
同处一层）；jar mod 本来就必须走安装期，不必维护两套补丁机制。

agent 唯一的独门能力是改「磁盘上不存在最终形态」的类。第 6 章那份清单里没有一
条需要它。**出现下列情况再回来重新评估**：需要改反混淆之后才成形的类；需要在
完全不能碰用户文件的场景下打补丁；补丁必须依赖只有运行时才知道的事实。

### 3.3 分层

| 层 | 做什么 | 用什么 |
|---|---|---|
| zip 级 | jar mod 叠加、剥签名、整个 artifact 替换 | Rust，几十行，不需要 Java |
| 字节码级 | §3.1 那种改写 | 见下 |

字节码级现在只有一个用例。可以先用手写的 Rust 改写器顶着，但它有一条硬天花
板：**只接受没有跳转的方法**，一旦要插入分支就得自己算 StackMapTable。因此把
话说在前面——**第二条需要分支的补丁出现时，就引入一个用 ASM 写的安装期工具**
（由补全阶段拉起 Java 执行，我们本来就为 Forge processors 起 Java 进程）。这样
拿到 ASM 的鲁棒性，同时保住安装期的失败模式。**不要因为想用 ASM 就改用 agent，
这是两条独立的轴。**

### 3.4 四条必须做对的规矩

1. **原件永不覆盖**，产物另存。否则 Fern 自己的文件完整性校验会把它判成损坏。
2. **剥签名**：删掉 `META-INF/*.SF|RSA|DSA` 与 MANIFEST 里的逐条摘要，做成干净
   的未签名 jar。Forge 的 universal jar 自己是签名的（`FORGE.SF` / `FORGE.DSA`，
   条目摘要 SHA-256）；只换 class 不动签名会得到一个**签名无效**的 jar，它今天
   不炸只是因为那个 2014 年的签名本身已经不被 JVM 校验——不能靠这个。
3. **缓存 key = 原 jar 的 sha1 + 补丁 id + 补丁版本**，任一变化就重做。
4. **产物登记进我们自己的清单**，让完整性检查知道它该长什么样。

### 3.5 连带结论

一旦支持 jar mod，改的就是 client jar 本身，FML 的防篡改检查**必然**失败。所以
`-Dfml.ignoreInvalidMinecraftCertificates=true` 不是 1.7.2 的专用开关，而是
「LaunchWrapper 时代 + 任何 jar 改动」的通用前提，应当由兼容规则统一给出，并在
界面上说明为什么可以给。

---

## 4. 兼容性规则引擎

### 4.1 两张表，不是一张

现在有三处各自为政：`java::requirement` 的区间表、`launch::platform_arguments`
的两条、`rules/crash.toml`。要合并，但**合并成两张**：

- **事前表**：证据是环境（版本、加载器、Java、OS、架构），产出动作。
- **事后表**：证据是日志，产出诊断 id（即现在的 `crash.toml`）。

两张共用同一套 `match` 语法。不合成一张的原因：它们的输入和时机不同，硬合并会
让 `match` 语言同时要表达「环境是什么」和「日志里出现了什么」，规则会变得难写
难读。1.7.2 那条正好横跨两边——事前打补丁加开关，事后万一还是崩了给解释。

### 4.2 动作，按侵入性排序

一条规则写**有序的备选方案**，引擎取第一个可行的：

```
RuntimeSelect        选或下载另一个 Java
JvmArgAdd / Remove   加或删一个参数
EnvSet               设一个环境变量
LoaderVersion        换一个上游已修好的加载器构建
ArtifactReplace      换掉某个库文件
ArtifactPatch        改产物的字节码
Block / Warn         都不行，说清楚为什么
```

### 4.3 判据：什么时候才允许改产物

> **只有当「运行时 × 加载器版本 × 启动参数」这个可选空间里不存在任何可用组合
> 时，才允许改产物。**

两个例子说明这条判据怎么用：

| | 1.7.2 CME | 1.16.3–1.16.5 + Java 8u321+ |
|---|---|---|
| 坏的组合 | 老 FML × **所有** ≥ 8u20 的 Java | ModLauncher 8.1.x × Java ≥ 8u321 |
| 可选空间里有好的组合吗 | 没有（Java 7 拿不到，也不该要求用户装） | 有两个：8u321 以下的 Java 8；Forge 36.2.25+ |
| 动作 | `ArtifactPatch` | `RuntimeSelect`，退而求其次 `LoaderVersion` |

**实测**：Mojang 自己发的 `jre-legacy` 是 8u51（Windows）、8u74（macOS）、
8u202（Linux），全都在 8u321 以下——第一备选永远拿得到。所以这一条**不需要**
改字节码。

### 4.4 Java 探测要补到 update 粒度

`select` 现在只保证 major 落在区间内。机器上装了系统 8u422 的话照样会被选中，
然后 1.16.5 Forge 崩在 `NoSuchMethodError`。`JavaRuntime.version` 里已经有完整
版本号（`1.8.0_492`），只是从来没解析过。

**这不是为了规则好看，它是 §4.3 那条规则能存在的必要条件。** 探测结果还应当补
上：架构位数、是否 headless、厂商——它们各自对应第 6 章里的一条规则。

### 4.5 规则要能表达「备选也落空」

**实测**：`windows-arm64` 与 `mac-os-arm64` 没有 `jre-legacy`——Mojang 根本不为
ARM 发 Java 8。所以 Apple Silicon 上跑 Forge 1.16.5，第一备选直接落空，只能退到
x64 Java 8 走 Rosetta，或者升级 Forge 构建。（第三方 ARM 版 Java 8 最早是不是也
在 8u321 之后，**未核实**，写规则前要验。）

这正是备选必须有序、而且允许全部落空后 `Block` 并说明原因的理由。

---

## 5. 外部实例导入

组件模型解决的是「导进来之后还能改」，导入本身要各自做：

- **Prism / MultiMC**：`mmc-pack.json` → 组件表，`patches/*.json` → 各组件，
  `instMods/` → jar mod 组件，`.minecraft` 就地用。
- **整合包**（CurseForge / Modrinth）：manifest → 组件 + 模组清单。
- **别人带来的 JVM 参数**：十年前的整合包里全是 `-XX:MaxPermSize`、
  `UseConcMarkSweepGC`，在现代 Java 上 JVM 自己就退了，Minecraft 一行日志都没
  有。`filter_jvm_arguments` 现在只有一条规则，要扩成按 Java 大版本的清单，
  **并且区分参数来源**：我们和整合包带来的过期参数静默迁移，用户亲手写的要明说
  「`UseConcMarkSweepGC` 在 Java 17 上不可用」。

---

## 6. 已确证的兼容条目

按证据强度排。**未核实的条目在动手前必须先验**——一句听着像那么回事、其实不对
的诊断，比没有诊断更浪费用户的时间。

| 条目 | 证据 | 动作 | 状态 |
|---|---|---|---|
| 1.7.2 / 1.6.4 Forge 在 8u20+ 上 CME | 实测 | `ArtifactPatch` + `JvmArgAdd` | 见 §3.1，待实现 |
| 老 FML 的 jar 签名是 SHA-1，现代 JVM 判为未签名 | 实测 | `JvmArgAdd` | 同上 |
| Forge 34.1.27–36.2.24（1.16.3–1.16.5）+ Java 8u321+ | 上游（ModLauncher #91、MultiMC #4566） | `RuntimeSelect` → `LoaderVersion` | 待实现，依赖 §4.4 |
| 老 FML + Java 24+（SecurityManager 永久禁用，JEP 486） | 上游 | 已被 Forge 的 Java 上限挡住 | 已覆盖 |
| Log4Shell 分代处理 | 上游（Mojang 公告） | 配置文件 + `formatMsgNoLookups` | 已实现 |
| 32 位 Java + 大 `-Xmx` | 上游 | `Block` / 降 `-Xmx` | 待实现，便宜 |
| headless JRE 跑客户端 | 上游 | `Block` | 待实现，便宜 |
| Apple Silicon + LWJGL 2（只有 x86_64 native） | 上游 | `ArtifactReplace` 或 x64 + Rosetta | 待实现 |
| Linux `/tmp` 挂 noexec，LWJGL 解 native 失败 | 上游 | `JvmArgAdd`（`org.lwjgl.librarypath` 指到我们自己的目录） | 待实现 |
| 老整合包带已被删除的 JVM 参数 | 上游 | `JvmArgRemove` + 诊断 | 待实现，见 §5 |
| macOS 新系统 + LWJGL 2 改窗口大小 SIGSEGV | 未核实 | 待定 | 先验 |
| Linux 混合显卡跑到核显 / GLX vendor 选错 | 未核实 | `EnvSet` | 先验 |
| OpenAL + PipeWire/JACK 崩溃 | 未核实 | 崩溃签名 → 下次启动 `EnvSet` | 先验 |
| `useLegacyMergeSort` | 未核实 | `JvmArgAdd`，且只对确定的 profile | 先验 |
| Java 6/7 的 PermGen | 未核实（我们目前不发 Java 7） | `JvmArgAdd` | 暂不做 |
| 按 client jar hash 的博物馆补丁包 | 未核实 | `ArtifactPatch` | 最后再说 |

一条**顺带发现**（不是 bug，记下来免得以后当成 bug 查）：现代元数据把
`natives-macos-arm64` 写成独立坐标，规则里只写 `os.name`，所以 macOS 上 x64 与
arm64 两份 native jar 都会下载。LWJGL 3 自己会挑对的，能跑，只是多下约 10 MB。
按 classifier 后缀过滤即可。

---

## 7. 实施顺序与验收

每一步的验收标准都是**真跑一次**，不是单测。这个项目最严重的 bug 单元测试一个
都抓不到，理由见 AGENTS.md。

| # | 做什么 | 验收 | 状态 |
|---|---|---|---|
| 1 | tweaker 从字符串覆盖改成有序追加 | 1.7.10 与 1.12.2 Forge 仍然跑到开窗口；两个 tweaker 的顺序在命令行里可见 | 已做 |
| 2 | zip 级变换 + 受限字节码改写器 + 缓存与剥签名 | 1.7.2 Forge 跑到开窗口；补丁产物反汇编出来调用的是 `Collections.copy`；原 jar sha1 不变 | 已做 |
| 3 | Java 探测补 update / 位数 / headless | 一台装了系统 8u321+ 的机器上，1.16.5 Forge 选中的是 `jre-legacy` | 已做 |
| 4 | 事前兼容规则表（先装 §6 前三条 + 32 位那条） | 每条规则一份 fixture，和崩溃规则同样的三件套约束 | 已做 |
| 5 | 组件模型 + Prism/MultiMC 导入 | 导入一个带 Forge + jar mod 的 Prism 实例并启动；导入后还能换加载器版本 | 已做 |
| 6 | JVM 参数按来源分级的 linter | 一份 2014 年的参数被迁移，且界面说得出改了什么 | 已做 |

第 1、2 步做完，1.7.2 就真的能玩；第 5 步做完，别人的复杂实例进来才是活的。

### 第 5 步是怎么落地的

两个问题当时悬着，答案都写进代码了：

- **Prism 的标准组件没有 `patches/*.json`。** 那些版本描述在它自己的全局 `meta/`
  缓存里。所以导入只取「游戏版本 + 加载器 + 加载器版本」这一组事实，描述由 Fern
  自己装一遍——装出来的是同一个上游产物。它记的版本号是规范化过的
  （`10.13.4.1614`），而 Forge 那几年的 maven 目录名带后缀
  （`10.13.4.1614-1.7.10`），中间那一层换算在 `loader::canonical_version`。
- **装出来的东西放在 Fern 这边。** `ExternalGame::shared_versions` 让外部实例的
  `versions/` 留在我们的数据根下，那个 `.minecraft` 只当游戏目录用。**实测**：
  导入之后原目录里没有多出 `versions/` 或 `libraries/`。

jar mod 走 zip 级叠加（`patch::with_jar_mods`），文件从原处复制到 Fern 的实例目录
下——启动要用它，而原处那一份随时会被那边删掉。叠完 client jar 就不是原样了，FML
的防篡改检查**必然**失败，所以 §3.5 那条开关由兼容规则的
`a-jar-mod-fails-the-tamper-check` 按「有没有 jar mod」这个事实给出，而不是按版本
区间猜。**实测**：导入一个 Forge 1.7.10 + jar mod 的 Prism 实例，跑到打不开显示；
产物里有 jar mod 的文件、没有 `META-INF/*.SF`；原 client jar 的 sha1 仍然是 Mojang
清单里那一个。

界面还没有导入入口——命令（`read_prism_instance` / `import_prism_instance`）已经
在了，那一屏怎么摆是设计的事。

---

## 附：这一轮实测记录的原始事实

留档，免得以后重新调研。全部为**实测**。

- 各代 Forge 的 `install_profile` 形态：1.6.4 与 1.7.2 的 `versionInfo` **没有**
  `inheritsFrom`（是一份完整描述）；1.7.10 / 1.8 / 1.11.2 有；1.12.2 起改用
  jar 内的 `version.json`，自己那个 jar 放在安装器的 `maven/` 下，
  `downloads.artifact.url` 是空串。
- 1.12.2 之前的 Forge 库清单里大半条目**只有坐标**，既无 `downloads` 也无
  `url`，按老约定应当去 `https://libraries.minecraft.net/` 取。
- Forge 的 `maven-metadata.xml` 顺序不可靠：1.12.2 那一段最新的在最后，1.7.2
  那一段最新的在最前。
- 1.7.10 到 1.12 需要 `${user_properties}`，缺了它 Gson 在 `main` 第一行就抛
  「Expected BEGIN_OBJECT but was STRING」。
- Mojang 的 `jre-legacy`：Windows 8u51、macOS 8u74、Linux 8u202；
  `windows-arm64` 与 `mac-os-arm64` 没有这个组件。
- 1.7.2 的 client jar 带 2013 年的 `MOJANGCS` SHA-1 签名；Forge 1.7.2 的
  universal jar 带 `FORGE.SF` / `FORGE.DSA`，条目摘要是 SHA-256。
