# Fern 设计文档：自动内存分配与 JVM 参数生成

状态：草案 v1
日期：2026-08-07
模块归属：Rust core（启动管线）
关联系统：实例详情页（分配预览）、状态岛（运行时内存指示）

---

## 1. 目标与定位

为每个实例在每次启动时计算一个合适的 `-Xmx`，并生成与之配套的完整 JVM 参数列表。用户默认不需要理解"内存分配"这个概念，也不需要接触滑块；同时保留完全的手动控制路径，且手动路径的优先级绝对高于一切自动逻辑。

设计的核心判断：**静态估算只负责第一次启动的合理性，真实运行数据负责之后的精确性。** 一旦自适应层存在，静态层的精度价值大幅下降，因此静态层刻意保持简单，工程投入集中在反馈闭环上。这是本方案与 HMCL（纯静态、不看实例内容）和 PCL2（静态但精细）的根本区别，也是超过两者的地方。

---

## 2. 背景调研摘要

### 2.1 现有启动器的实现

**HMCL**（`HMCLGameRepository.getAutoAllocatedMemory()`）：输入仅有启动时刻可用物理内存。预留 512 MiB 给堆外与自身，前 8 GiB 按 80% 分配，超出部分边际率降为 20%，硬上限 16 GiB。完全不感知实例内容。一个必须继承的细节：若用户自定义 JVM 参数中已含 `-Xmx`，自动分配主动让位。

**PCL2**（`PageInstanceSetup.GetRam()`）：两层模型。第一层按实例类型确定四个需求锚点（GB）：

| 实例类型 | Min | T1 勉强带动 | T2 没什么问题 | T3 重度扩展 |
|---|---|---|---|---|
| 可装 Mod（N = mods 目录内 jar/zip/litemod 计数） | 0.5 + N/150 | 1.5 + N/90 | 2.7 + N/50 | 4.5 + N/25 |
| 仅 OptiFine | 0.5 | 1.5 | 3 | 5 |
| 原版 | 0.5 | 1.5 | 2.5 | 4 |

第二层将启动时可用内存按边际递减填入：0→T1 段 100%，T1→T2 段 70%，T2→T3 段 40%，T3→2×T3 段 15%，最终不低于 Min。每次启动重算。此外 PCL2 在设置页用条形图可视化"已用 / 分配给游戏 / 剩余"。

**Prism / MultiMC**：无自动分配，固定默认值；官方 wiki 立场是除大型整合包外 4 GB 足够，且明确提示"分配更多内存并不等于更好的性能"。

**CurseForge**：提供"使用整合包作者推荐的内存设置"开关，整合包 manifest 可携带作者声明的推荐值。这是实例内容之外唯一的权威信号源。

### 2.2 Mojang 官方基线的变化（26.1，2026-03）

自 26.1（新的 year.drop.hotfix 版本格式的首个正式版）起：

- 默认最大堆从 2 GB 提升至 4 GB
- 默认 GC 从 G1GC 换为（分代）ZGC，兼容设备上生效
- 捆绑运行时为 Microsoft OpenJDK 25，26.1 起要求 Java 25
- 默认参数新增 `-XX:+UseCompactObjectHeaders -XX:+AlwaysPreTouch -XX:+UseStringDeduplication`
- 后续 snapshot 将初始堆（Xms）调低以减少崩溃

含义：Mojang 自己已经完成了 ZGC 时代的默认参数调校。**26.1+ 的原版实例，Fern 不做任何 GC 参数干预，完整沿用版本 JSON 自带的默认参数，只按需覆盖 -Xmx。** Fern 的 GC 决策树仅服务于老版本与 Mod 环境。

另一个推论：brucethemoose 在 G1 时代的客户端基准结论（StringDeduplication 更慢、不建议 AlwaysPreTouch）只适用于 G1 路径，与 ZGC 路径的取舍已经分化，两套结论不要混用。

### 2.3 社区经验值（用于校验算法输出，非直接输入）

| 场景 | 常见 -Xmx |
|---|---|
| 老版本原版 ≤1.12.2 | 2–3 GB |
| 现代原版 | 4 GB |
| 轻量 Mod（几十个） | 4–6 GB |
| 中型整合包 | 6–8 GB |
| 大型整合包（ATM 级，300+ Mod） | 8–12 GB |
| HD 材质 / 光影 | 在原值上加约 1–1.5 GB 余量 |

大型整合包官方建议：ATM 系列 8–12 GB；Enigmatica 10 约 6.5 GB（HD/光影 8 GB）；FTB 建议通常控制在 10 GB 以内。普遍共识：分配不超过物理内存一半；过量分配本身有害（拖慢 GC、挤压页缓存与堆外）。

---

## 3. 决策优先级链

自上而下，命中即停止：

```
0. 用户手动设置的内存值
   （绝对优先，自动逻辑完全静默）

1. 用户自定义 JVM 参数中检测到 -Xmx
   （自动分配让位，不注入任何内存参数；沿用 HMCL 行为）

2. 整合包作者推荐值
   （仅整合包导入的实例；CurseForge manifest 的推荐内存字段，
    FTB 包的 min/max 配置。仍受 §5.4 双重约束截断）

3. 历史实测值
   （该实例存在有效运行历史，且 mod 列表哈希未变；见 §6）

4. 静态估算
   （首次启动、历史失效、或以上均不可用时的兜底；见 §5）
```

层 2 与层 3 的关系：作者推荐值作为首次启动的初值，之后被历史实测值接管。作者比玩家更了解包，但实测数据比作者更了解这台机器。

---

## 4. 输入信号

启动时刻采集，全部为本地零成本操作：

| 信号 | 来源 | 用途 |
|---|---|---|
| 可用物理内存 | sysinfo（启动时刻实时值） | 边际填充与实时约束 |
| 总物理内存 | sysinfo | 静态上限 |
| Mod 数量 N | mods 目录文件计数（jar/zip/litemod，含 disabled 后缀排除） | 锚点计算 |
| 加载器类型 | 实例元数据 | 区分原版/可 Mod |
| 光影环境 | 检测 Iris/Oculus/OptiFine + shaderpacks 目录非空 | 锚点上浮 |
| 渲染距离 | 解析 options.txt（renderDistance） | 弱修正 |
| MC 版本 → Java 大版本 | 实例元数据 | GC 路径选择、26.1+ 豁免 |
| GPU 类型（核显/独显） | 系统信息 | 核显机器额外保守（共享显存） |

明确排除的信号（评估后砍掉）：

- **重型 Mod 分类**（查询 Modrinth/CurseForge 元数据给 worldgen、科技类单独加权）：需要指纹匹配与网络往返，权重数值缺乏实测依据，而其修正的误差在自适应层一个 session 后即被覆盖。性价比不成立。
- **材质包 PNG 分辨率扫描**：材质压力主要在 VRAM，堆内影响集中于图集构建的瞬时峰值，为此解压 zip 不值得。光影信号已覆盖主要场景。
- **Mod 数量阶梯表**（如 31–80 个加 1 GB）：阶梯有断崖效应，连续函数严格更优。

---

## 5. 静态估算层

形状沿用 PCL2 的锚点 + 边际递减模型，参数按 2026 年的版本现状与社区经验值重校。

### 5.1 需求锚点

```
基础锚点（GB）：

原版：
  ≤1.12.2        Min=0.5  T1=1.0  T2=2.0  T3=3.0
  1.13–1.16.5    Min=0.5  T1=1.5  T2=2.5  T3=4.0
  1.17+          Min=1.0  T1=2.0  T2=4.0  T3=5.0
  26.1+          直接采用 Mojang 默认 4 GB 作为 T2，
                 Min=2.0  T1=3.0  T2=4.0  T3=6.0

可装 Mod（在对应版本段原版锚点上叠加）：
  T1 += N/90
  T2 += N/50
  T3 += N/25
  Min += N/150

修正项：
  检测到光影环境：      T2 += 0.5，T3 += 1.0
  渲染距离 > 16 区块：  T2 += 0.5（>28 时 += 1.0）
```

版本段基础值的依据：1.17+ 的 Java 16/17 迁移与 1.18 世界高度扩展抬高了原版基线；26.1 的 4 GB 是 Mojang 实测后的官方判断，直接采信为 T2。

### 5.2 边际递减填充

设启动时刻可用内存为 A，先扣除堆外预留（见 §5.3）得到预算 B，按四段填充：

```
阶段一  0 → T1        边际率 100%
阶段二  T1 → T2       边际率 70%
阶段三  T2 → T3       边际率 40%
阶段四  T3 → 2×T3     边际率 15%
结果 clamp 到 [Min, hardCap]
```

（边际率 r 的含义：该段内每分配 1 GB 堆，要求预算中扣减 1/r GB。）

### 5.3 堆外预留

实际进程 RSS 通常比 Xmx 高 0.5–1.5 GB：Metaspace、DirectByteBuffer、LWJGL native、显卡驱动映射均在堆外。预留规则：

```
reserve = 1.0 GB                    基础
        + 0.5 GB  若核显（共享显存直接吃系统内存）
        + 0.5 GB  若走 ZGC 路径（着色指针与并发回收的额外开销）
```

### 5.4 双重约束

```
staticCap  = min(totalRAM × 0.5, 16 GB)       静态上限：最多敢要多少
liveCap    = available − reserve              实时约束：现在实际能给多少
hardCap    = min(staticCap, liveCap)
```

静态上限管长期合理性（不超总量一半是 Mojang 排障建议与社区共识；16 GB 沿用 HMCL 的封顶，覆盖 400 Mod 级巨型包仍有余量）。实时约束管当下——用户开着浏览器和 IDE 时不把系统压进 swap。仅用总量做上限（忽略实时可用量）是常见方案的缺陷，此处显式修正。

若 liveCap < Min（内存极度紧张），仍按 Min 分配并在启动前给出一次非阻塞提示。

---

## 6. 自适应层

本模块最有价值的部分。目标：第二次启动起，分配值由该实例在这台机器上的真实行为决定。

### 6.1 数据源：GC 日志注入

Fern 是拉起 JVM 的一方，注入日志参数零成本、零 Mod 依赖、全版本可行：

```
Java 9+ ：-Xlog:gc*:file=<instance>/logs/fern-gc.log:time,uptime:filecount=3,filesize=10M
Java 8  ：-Xloggc:<instance>/logs/fern-gc.log -XX:+PrintGCDetails -XX:+PrintGCTimeStamps
```

排除的替代方案：JMX 需注入 management agent 并开放端口，侵入性过高；依赖 spark 等 Mod 无法作为基础设施；仅采 OS 层 RSS 无法区分堆内水位与堆外开销（但 RSS 可作为"是否超售物理内存"的辅助信号保留采集）。

### 6.2 采集指标

进程退出后解析日志（运行中 tail 的用途见 §8）：

```
peakHeap        会话内堆使用峰值
afterGcHeap     每次完整回收后的堆水位序列 → 取会话 p90 作为 live set 估计
gcPauseP99      停顿时长 p99
gcFrequency     单位时间回收次数
allocStall      （ZGC）allocation stall 出现次数
oom             退出码 + 日志扫描 OutOfMemoryError / hs_err 判定
```

### 6.3 历史的组织与失效

```
存储键：  (instance_id, modlist_hash)
modlist_hash = hash(排序后的 mods 目录文件名 + 文件大小)

滚动窗口：最近 8 次会话
生效条件：窗口内 ≥ 2 次有效会话（时长 > 5 分钟，排除启动即退出）
失效：    modlist_hash 变化 → 历史降级为参考，回到静态估算重新学习
```

单次会话噪声很大（探图与蹲家的内存曲线差异显著），因此目标值基于窗口内 afterGcHeap 的 p90 而非任何单次值。mod 列表失效判定是必需品——玩家往 mods 目录塞 40 个新 Mod 后，上个月的统计已经失去意义。

### 6.4 目标值与调整规则

```
liveSet   = 窗口内 afterGcHeap 的 p90
factor    = 1.6 （G1 路径） / 1.9 （ZGC 路径）
targetXmx = liveSet × factor
```

live set × 1.5–2 是 JVM 堆定容的经典启发式。ZGC 系数更高的原因：并发回收在分配速率追上回收速率时产生 allocation stall，需要比 G1 更大的余量吸收突发分配。

在 target 基础上叠加事件驱动的快速修正：

```
上次会话 OOM                    → 立即 +2 GB（越过滞回）
peak > 90% Xmx 或出现 stall     → +1 GB
peak > 80% Xmx                  → +0.5 GB
peak 在 60%–80%                 → 保持
连续 3 次会话 peak < 55%        → −0.5 GB
```

下调必须满足连续性条件（滞回），避免分配值在 8 → 7.5 → 8 之间震荡。上调即时生效——宁可多给半 G，也不让玩家再撞一次 OOM。所有结果仍受 §5.4 双重约束截断。

### 6.5 GC 行为作为健康信号

正常曲线是锯齿：堆升至高位、回收、落回 live set 附近。需要干预的形态是回收后水位依然贴近 Xmx（live set 本身逼近上限），此时上调有效。反之，若停顿频繁但水位健康，问题在 GC 参数而非堆大小，上调无效——这个区分避免"卡顿就加内存"的社区常见误判被自动化固化。

---

## 7. JVM 参数生成

### 7.1 GC 决策树

```
MC 26.1+ 且无 Mod 加载器：
    不干预。沿用版本默认参数（ZGC + UseCompactObjectHeaders
    + AlwaysPreTouch + UseStringDeduplication），仅按需覆盖 -Xmx。

其余情况按 Java 大版本：
    Java 21+       ZGC 分代：-XX:+UseZGC（21/22 加 -XX:+ZGenerational，23+ 默认分代）
    Java 15–20     保守起见走 G1（非分代 ZGC 有实测的客户端 FPS 损失）
    Java ≤14       G1
    Java 24+       追加 -XX:+UseCompactObjectHeaders
    Windows < 10 1809 → 强制回退 G1（ZGC 系统要求）

用户自定义参数中检测到任何 GC 相关旗标 → 整棵树静默让位。
```

### 7.2 G1 参数集（老版本 / 回退路径）

以 Mojang 传统默认为基线，叠加实测有效项：

```
-XX:+UnlockExperimentalVMOptions
-XX:+UseG1GC
-XX:G1NewSizePercent=20
-XX:G1ReservePercent=20
-XX:G1HeapRegionSize=32M
-XX:MaxGCPauseMillis=37
-XX:+PerfDisableSharedMem
Java 12+ 追加：-XX:MinHeapFreeRatio=25 -XX:MaxHeapFreeRatio=40   （允许堆收缩，挂后台时向系统归还内存）
Java ≤7  追加：-XX:MaxPermSize=512m
```

MaxGCPauseMillis 取 37 来自 brucethemoose 的客户端基准（更频繁但感知不到的短停顿优于默认 50 下的偶发长停顿）。明确不采用：Aikar 服务端参数整套照搬（其 G1NewSizePercent 在客户端产生长停顿，且老年代回收对客户端过于激进）；`-XX:+ParallelRefProcEnabled` 在 Java 8 有崩溃报告，不进默认集。

### 7.3 Xms 策略

只设 Xmx，Xms 交给 JVM 或设为小值（如 1 GB）。理由：客户端内存压力多变、玩家会挂后台，固定 Xms=Xmx 加预触碰是服务端独占机器的逻辑；Mojang 26.1 后续 snapshot 主动把初始堆调低（保持 Xmx 4 GB）与此判断一致。G1 路径配合 §7.2 的 HeapFreeRatio 收缩参数构成同一套哲学：堆保持弹性。

### 7.4 必带的非性能参数

```
-Dlog4j2.formatMsgNoLookups=true          Log4Shell 防御，受影响老版本必带

编码（中文用户刚需，按 Java 大版本）：
  Java 21+    固定 UTF-8，无需处理
  Java 18–20  -Dfile.encoding=COMPAT（防部分 Mod 配置不兼容 UTF-8）
  更老        按 native 编码设置 sun.stdout.encoding / sun.stderr.encoding
  命令行参数乱码（JDK-8272352）：参照 PCL2 的 JavaWrapper 方案评估，
  Windows 非 ASCII 路径/玩家名场景需要，实现阶段单独立项
```

---

## 8. 与 Fern 其他系统的接口

**实例详情页 — 分配预览。** 配置页不放滑块，默认展示一行可解释的结论：

> 自动：8 GB
> 基于 186 个 Mod、光影、16 区块渲染距离。上次运行峰值 6.3 GB，当前保留约 27% 余量。

点开后才是手动覆盖入口。判断依据摊开、控件退后，符合界面整体的表达方式。

**状态岛 — 运行时内存指示。** 岛的"游戏运行中"形态 tail fern-gc.log，以极低视觉权重呈现堆压力（例如胶囊底部一条随水位变化的细线）。数据管线与自适应层共用同一条日志流，无额外成本。压力持续贴顶时，岛可给出一次"下次启动将自动增加内存"的轻提示——自适应行为对用户可见但不需要用户操作。

**启动管线。** 参数生成是纯函数：`(InstanceProfile, SystemSnapshot, History) → LaunchArgs`，不持有状态；历史读写由独立的 telemetry store 负责，与 §6.3 的键结构对应。

---

## 9. Rust 模块结构（骨架）

```
fern-core/src/memory/
├── mod.rs              // pub fn resolve_allocation(...) -> AllocationDecision
├── signals.rs          // 输入信号采集（sysinfo、mods 计数、options.txt 解析）
├── estimate.rs         // 静态估算：锚点 + 边际填充 + 双重约束
├── adaptive.rs         // 历史统计、目标值计算、调整规则与滞回
├── gclog.rs            // GC 日志解析（Xlog 统一格式 + Java 8 旧格式）
├── history.rs          // (instance_id, modlist_hash) 键控的滚动窗口存储
└── jvm_args.rs         // GC 决策树 + 参数集生成 + 用户参数让位检测

AllocationDecision {
    xmx_mb: u32,
    source: Manual | UserJvmArgs | PackAuthor | Adaptive | Static,
    explanation: Vec<ExplanationItem>,   // 供 UI 渲染那一行文案
    args: Vec<String>,
}
```

`explanation` 作为一等公民返回：可解释性由数据结构保证，而非 UI 层事后拼凑。

---

## 10. 边界情况

- **liveCap < Min**：按 Min 分配，启动前非阻塞提示内存紧张。
- **32 位 Java**：Xmx 封顶 1.5 GB 并提示更换运行时。
- **多实例并行**：第二个实例启动时 liveCap 已自然反映第一个实例的占用，无需专门处理；但 UI 提示中注明"另一实例运行中"。
- **同实例快速重启**：上一进程内存可能未完全归还，采集 available 前等待或对连续启动使用上次的分配值。
- **历史存在但 Java 版本变更**（用户换了 Java 导致 GC 路径切换）：factor 变化，目标值重算，窗口保留。
- **整合包更新**：modlist_hash 变化走失效逻辑；作者推荐值若随包更新，重新进入优先级链。

---

## 11. 分期

**P1（随启动管线首版）**：优先级链 0/1/4 + 静态估算 + GC 决策树 + 参数生成 + 详情页预览文案。
**P2**：GC 日志注入与解析、历史存储、自适应调整（优先级 3）、状态岛内存指示。
**P3**：整合包作者推荐值接入（优先级 2，依赖整合包导入功能）、JavaWrapper 编码方案评估。

P1 独立可用且已优于 HMCL 的现状；P2 是本设计的差异化所在。

---

## 12. 实现与本文的偏差

P1 与 P2 已实现（`fern-core/src/launch/memory/`）。三处和上文写的不一样，都是实现时被证据推翻的：

**§5.2 的第四段（T3 → 2×T3，边际率 15%）没有实现。** 它和 §2.3 那张验收表直接冲突：在一台空闲 50 G 的机器上第四段会被填满，一个原版 1.21.4 实例拿到 10 G，而 T3 的定义就是「重度扩展也够」。填充到 T3 为止之后，真机输出落回经验值表里（老版本原版 3 G、现代原版 5 G、40 个 Mod 6.5 G）。超过 T3 的判断留给自适应层——它看得见实际用量，静态层看不见。

**§6.4 的下调步长不是固定的 0.5 G，而是「离目标的距离取一半，至少 0.5 G」。** 固定步长在首次估算给多了的时候要几十次会话才收得回来（16 G 收到 3 G 是二十六次），而滞回是为了不震荡，不是为了慢。离目标越远步子越大反而更稳：真正会震荡的是贴着目标反复横跳，那种情况下这个式子给出的正好是最小的 0.5 G。

**§3 优先级链的第 2 层（整合包作者推荐值）没有占位。** 它依赖整合包导入时把 manifest 里的推荐内存留下来，那件事还没做。没有数据源的层不占位。

另有两处实现细节与文中的描述不同但不改变行为：GC 日志落在 Fern 自己的实例日志目录（`logs/instances/<id>/gc.log`）而不是游戏的 `logs/` 下——那是游戏的地方，两边互相清理会误伤；「用什么 GC」在设置里多了一档 `Auto` 并成为默认值，原来写死的 G1 变成三档中的一档。

---

## 附：主要参考

- HMCL `HMCLGameRepository.java`（getAutoAllocatedMemory，源码实读）
- PCL2 `PageInstanceSetup.xaml.vb` GetRam / `ModLaunch.vb` 内存管理段（源码实读）
- Minecraft Java Edition 26.1 更新说明（Mojang，默认 4 GB / ZGC / Java 25）
- Minecraft Wiki: Java Edition 26.1（默认 JVM 参数变更明细）
- brucethemoose / Minecraft-Performance-Flags-Benchmarks（客户端 G1 基准与参数集）
- Prism Launcher Wiki: Java Settings；CurseForge 整合包推荐内存机制
- ATM / Enigmatica / FTB 官方内存建议；r/feedthebeast 社区经验分布
