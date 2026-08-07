# Fern Minecraft 启动器核心功能开发文档

> 范围：启动游戏相关的核心链路——版本元数据、文件补全、账户认证、Java 管理、启动流程、性能调优。
> 不含 UI/UX、mod 管理、多人联机（Pearl 侧）等内容。
>
> 技术栈：Rust 核心 + Tauri 2 应用。核心逻辑与前端仅通过 Tauri command / event 通信，保证核心可独立测试并被其他宿主复用。当前产品只交付桌面应用。

---

## 0. 模块划分

核心拆为三个 Rust crate，按依赖顺序：

| Crate | 职责 | 依赖 |
|---|---|---|
| `meta` | 版本元数据的数据模型、解析、继承合并、rules 求值 | serde |
| `download` | 并发下载、sha1 校验、镜像源切换、进度事件 | tokio, reqwest, `meta` |
| `launch` | 账户认证、Java 管理、命令行拼装、进程管理、日志/崩溃分析 | `meta`, `download` |

前端（Tauri）不包含任何业务逻辑，只做三件事：发起 command、订阅进度/日志 event、渲染状态。

---

## 1. 版本元数据（`meta`）

一切功能的地基。Mojang 的元数据体系本质上是一份公开的启动协议，把数据模型做对，后面所有功能都围绕它转。

### 1.1 数据链条

```
version_manifest_v2.json          版本总表（id、type、url、sha1）
  └─ <version>.json               单版本描述
       ├─ downloads.client        客户端 jar
       ├─ libraries[]             依赖库（Maven 坐标 + 下载信息 + rules）
       ├─ assetIndex              资源索引的引用
       ├─ arguments / minecraftArguments   启动参数
       ├─ javaVersion             Java 版本要求
       └─ logging.client          log4j 配置文件
```

所有 URL 抽象为 `DownloadSource` trait，官方源与 BMCLAPI 镜像只是 URL 改写规则不同：

```rust
trait DownloadSource {
    fn version_manifest(&self) -> Url;
    fn rewrite(&self, official: &Url) -> Url;   // libraries.minecraft.net → bmclapi 等
}
```

失败时自动在源之间切换重试（详见 §2.4）。

### 1.2 `inheritsFrom` 合并器

Forge / NeoForge / Fabric 安装后生成"修改版" JSON，通过 `inheritsFrom` 继承原版 JSON。需要实现合并器，规则：

- **libraries**：子表拼接到父表之前；同 `groupId:artifactId`（忽略 version）时**子版本优先**，父项丢弃。这一条决定了 ASM、log4j 等库的实际加载版本，写错会出现难以排查的类加载问题。
- **arguments**：`game` 与 `jvm` 数组分别做父+子拼接（父在前）。
- **mainClass、type 等标量字段**：子覆盖父。
- 支持多级继承（理论上可链式，实际最多两级）。

### 1.3 arguments 的两代格式

- **1.13+（结构化）**：`arguments.game` / `arguments.jvm`，数组元素可能是纯字符串，也可能是带 `rules` 的对象。serde 用 `#[serde(untagged)]` enum 处理混合类型。
- **旧版（字符串）**：只有 `minecraftArguments` 一整个字符串，且**没有 JVM 参数段**，需要硬编码补上：

```
-Djava.library.path=${natives_directory}
-Dminecraft.launcher.brand=${launcher_name}
-Dminecraft.launcher.version=${launcher_version}
-cp ${classpath}
```

### 1.4 rules 求值器

libraries 与 arguments 共用同一套 rules 结构，写一个求值器到处用：

- 匹配维度：`os.name`（windows/osx/linux）、`os.arch`（x86 等）、`os.version`（正则）、`features`。
- `features` 由启动上下文提供，如 `is_demo_user`、`has_custom_resolution`、`has_quick_plays_support`。
- 语义：无规则 = allow；有规则时从 disallow 默认值出发，按顺序求值，最后一条匹配的规则的 action 生效。

---

## 2. 文件补全（`download`）

补全 = 扫描 → 对账 → 下载。每个文件都有 sha1 与 size，**校验通过即跳过**，因此补全天然幂等，"修复文件"功能等于免费获得（同一入口再跑一遍）。

### 2.1 三类对象

**Libraries**

- `name` 为 Maven 坐标（`com.mojang:brigadier:1.0.18`），落盘路径由坐标推导：`libraries/com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar`。
- **natives 分叉**（重要）：
  - 旧版本（约 1.19 之前）：library 带 `natives` classifier + `extract` 字段，下载后解压到 natives 目录，按 `extract.exclude` 排除 `META-INF/`。
  - 新版本：natives 作为独立 library 直接进 classpath，不再解压。
  - 两条路径都要实现，按 JSON 里有无 `natives` 字段区分。
- 部分第三方库（Forge 的 maven）只有 `url` 前缀没有完整 `downloads`，路径照 Maven 坐标推导。

**Assets**

- 先下载 `assetIndex` 指向的 JSON，其中每个资源为 `{hash, size}`。
- 存储路径：`assets/objects/{hash 前两位}/{hash}`，全局共享、按内容寻址，多实例零重复。
- 1.6 之前的远古版本使用 virtual/legacy 布局（`map_to_resources`）。若不支持 1.5.x 可直接砍掉，省不少代码。

**其余**：客户端 jar（`downloads.client`）、log4j 配置文件（`logging.client.file`）。

### 2.2 下载器设计

- tokio + `Semaphore` 控制并发（64 左右起步）。
- 每个任务：HEAD 可跳过，直接 GET → 写临时文件 → 校验 sha1 → 原子 rename。校验失败换源重试，重试耗尽则记入失败清单。
- 大文件（client jar、Java 运行时）支持断点续传（Range）。

### 2.3 进度事件

进度做成可聚合的事件流发给前端（Tauri event）：

```rust
enum DownloadEvent {
    TaskStarted { total_files: u64, total_bytes: u64 },
    FileDone { path: PathBuf, bytes: u64 },
    Progress { done_bytes: u64, speed_bps: u64 },
    TaskFinished { failed: Vec<PathBuf> },
}
```

这也是生成式封面加载动画的数据来源。

### 2.4 镜像源策略

- 源列表：官方 → BMCLAPI（可配置顺序，海外用户默认官方优先）。
- 按域名做健康度统计（近期失败率、延迟），失败自动降级到下一源；不做启动时的"测速选源"，按需切换即可。

### 2.5 Mod 加载器安装（补全的延伸）

难度差异巨大，分阶段实现：

- **Fabric / Quilt**（第一阶段）：调 meta server 拿 profile JSON，与原版 JSON 合并，下载 loader 与 intermediary 库。纯数据操作，半天工作量。
- **NeoForge**（第二阶段）：install profile 带 processors，安装期需要真的运行若干 Java 进程（jarsplitter、binarypatcher 等）做 deobf 与 patch。实现要点：解析 `install_profile.json` 的 `processors[]`，按序以下载好的 Java 执行，替换 `{VARIABLE}` 占位符，校验每步输出的 sha1。
- **旧版 Forge（1.12.2 及更早）**（暂缓）：格式又不一样，等框架稳定后按需求补。

### 2.6 元数据缓存（`metacache`）

分类的判据只有一条：**内容会不会变。**

| | 是什么 | 落在哪 | 策略 |
|---|---|---|---|
| 不可变 | 版本 JSON、资源索引、运行时文件清单 | 它本来就该在的地方（`versions/<id>/<id>.json`、`assets/indexes/<id>.json`） | 上游连 sha1 一起发布，本地对得上就永远不再拉 |
| 可变 | 版本清单、加载器版本列表、运行时索引 | `cache/` | 它们回答「现在有哪些」，六小时 TTL；用户点刷新走强制 |

三条规则：

- **缓存和成品是同一个文件。** 不可变的那一类不另存副本——否则迟早出现「缓存里有、成品里没有」这种最难查的分叉。可变的那一类才有独立的 `cache/` 目录，它的全部内容都可以随时整个删掉，下次联网自己长回来，所以「清理缓存」是安全的。
- **刷不到就用旧的，但必须留痕。** 列表旧了几个小时，最坏是看不到昨晚那个快照；拉不到就报错，等于网络一抖启动器就不能用。退回旧数据时往 `fern.log` 写一行——不写的话「为什么新版本没出现」将来没有任何线索。
- **准备好的实例必须能离线启动。** 补全的第一步先看本地有没有那份版本 JSON，有就直接读，**连清单都不拉**。为了确认一份我们已经有的东西没变而联网，是给每一次启动加一道无谓的门槛。

新鲜度只有两档：`Within(ttl)` 和 `Force`。缓存里没找到要找的版本时自动升级成 `Force` 再找一次——快照发布十分钟后就来建实例是正常用法。

---

## 3. 账户与认证（`launch::auth`）

三种账户类型统一到一个 trait：

```rust
trait Account {
    async fn ensure_fresh(&mut self) -> Result<()>;   // 静默刷新
    fn launch_credentials(&self) -> Credentials;       // name / uuid / access_token / user_type
    fn extra_jvm_args(&self) -> Vec<String>;           // authlib-injector 用
}
```

### 3.1 微软正版认证

**令牌链**（五段，每段一个 HTTP 请求）：

```
MSA OAuth (device code flow)
  → XBL   POST user.auth.xboxlive.com/user/authenticate
  → XSTS  POST xsts.auth.xboxlive.com/xsts/authorize
  → MC    POST api.minecraftservices.com/authentication/login_with_xbox
  → profile GET api.minecraftservices.com/minecraft/profile
```

**前置手续（尽早办，唯一不受控的排期）：**

1. 进 portal.azure.com → Microsoft Entra ID → App Registrations → New Registration。
   - Supported account types 选支持"任意组织目录 + 个人微软账号"的那档（玩家都是消费者账号）。
2. Authentication 页：Add Platform 选 Mobile and desktop applications；**Allow public client flows 切为 Yes**（device code flow 的前提）。
3. 抄下 Overview 页的 Client ID 与 Tenant ID。Client ID 非机密，可硬编码进仓库。
4. **先触发一次失败登录**：把认证链写到 `login_with_xbox` 返回 403 为止——微软要求应用有活动记录才受理白名单申请。
5. 填白名单申请表 https://aka.ms/mce-reviewappid ，用途如实写"开源第三方 Java 版启动器"。审批周期数周到一两个月，拖太久可发邮件跟进；批准后再等最多 24 小时生效。

**实现要点：**

- **Device code flow**：端点必须用 `consumers` tenant（`login.microsoftonline.com/consumers/oauth2/v2.0/devicecode`），用 `common` 会报无提示性的错误。scope 写 `XboxLive.signin offline_access`（后者用于 refresh token）。展示八位代码引导用户去 microsoft.com/link 输入，客户端按 interval 轮询 token 端点。
- **XSTS 错误码**要给出人话提示：
  - `2148916233`：微软账号没有 Xbox 账号 → 引导去 xbox.com 创建。
  - `2148916238`：未成年账户 → 需由成人加入家庭组。
- **令牌生命周期**：MSA refresh token 长期有效；MC access token 24 小时过期。启动前 `ensure_fresh` 静默刷新整条链，失败才弹登录界面。
- **存储**：refresh token 存系统 keychain（`keyring` crate），禁止明文落盘。
- **Game Pass 用户**：可能拿到空 profile（从未在官方启动器登录过），提示先在官方启动器完成一次初始化。

### 3.2 外置登录（authlib-injector）

面向 LittleSkin 等 Yggdrasil 兼容皮肤站，国内用户占比高，优先级与微软登录并列。

- 认证走皮肤站的 Yggdrasil API（authserver：`/authenticate`、`/refresh`、`/validate`）。
- 启动参数注入：
  - `-javaagent:{injector_jar_path}={api_root_url}`
  - 预取 API 元数据 base64 后塞进 `-Dauthlibinjector.yggdrasil.prefetched={b64}`，省一次启动时网络请求。
- injector jar 从官方 GitHub release 下载（BMCLAPI 有镜像），校验后缓存。

### 3.3 离线模式

- UUID 用 `md5("OfflinePlayer:" + name)` 生成 version 3 UUID，与原版服务器离线算法一致，保证进离线服时白名单与皮肤行为正确。
- access_token 填任意占位串；user_type 填 `legacy`（部分旧版参数模板需要）。
- **离线模式最先实现**：认证链可完全绕开，在 Azure 白名单批下来之前就能端到端跑通整条启动管线。

### 3.4 名册：账户是复数（`accounts`）

三种类型不是「三选一的模式」，是同一份名单里可以并存的条目。一个人同时有正版号、皮肤站号和几个测试用的离线号是常态。

拆成两半，判据是**这一条能不能给别人看**：

| | 内容 | 在哪 |
|---|---|---|
| 名册 | id、类型、名字、UUID、皮肤站地址 | `accounts.json` |
| 秘密 | 令牌 | 系统钥匙串，一账户一条，键 `session-<id>` |

- **id 一旦发出就不再改变**，钥匙串的键和界面的身份都指着它；名字随时可以改。
- **离线账户在钥匙串里没有条目**，它没有秘密。它的 UUID 由名字算出，所以**改名等于换人**——界面必须说清楚，不能让它看起来像改个标签。
- **同一个 UUID 再登录一次是「重新登录」**，换的是令牌，不是第二个账户。皮肤站不同则算不同的人，UUID 撞了也一样。
- **有记录没令牌是一种真实状态**（钥匙串没解锁、用户在系统里手删过），要说出来，不能表现成「没登录过」。
- 迁移只在 `accounts.json` 确实不存在时跑。钥匙串读不出来就什么都不动——读不到就不删，遗留的条目留在原地还有救。

---

## 4. Java 管理（`launch::java`）

分三层。

### 4.1 声明层：版本需要什么 Java

- version JSON 的 `javaVersion.majorVersion`（8 / 16 / 17 / 21）给出原版要求。
- Mod 加载器会收紧约束（旧 Forge 在新 Java 上直接崩），最终匹配逻辑是"版本要求 ∩ 加载器兼容区间"。维护一张小型兼容表：

| MC 版本段 | 原版要求 | 已知收紧 |
|---|---|---|
| ≤1.16.5 | 8 | Forge ≤36 建议 8，部分 8u321+ 有兼容问题 |
| 1.17.x | 16 | — |
| 1.18–1.20.4 | 17 | — |
| 1.20.5+ | 21 | — |

### 4.2 发现层：机器上有什么 Java

扫描候选路径的并集：

- Windows：注册表 `HKLM\SOFTWARE\JavaSoft\*`、`Program Files\Java`、`Program Files\Eclipse Adoptium` 等各发行版默认目录
- macOS：`/Library/Java/JavaVirtualMachines/*/Contents/Home`
- Linux：`/usr/lib/jvm/*`
- 通用：`JAVA_HOME`、`PATH`、启动器自有运行时目录

识别版本**直接读 JDK 根目录的 `release` 文件**（含 `JAVA_VERSION`、`OS_ARCH`），不要起 `java -version` 进程，快且可靠。

架构校验：Apple Silicon 上的 x64 Java 走 Rosetta 能跑但性能明显下降，标记为降级候选，仅在无 arm64 可用时选中并提示。

### 4.3 下载层：缺就自动下

- 首选 Mojang 官方运行时（`java-runtime-gamma` 等）：有 manifest、BMCLAPI 有镜像、与官方启动器行为一致，且清单结构与 assets 类似——下载器代码直接复用。
- Adoptium API（按 os/arch/major 拉 Temurin）作为兜底，用于 Mojang 清单未覆盖的平台组合。

**体验原则：这一层对用户完全隐形。** 点启动 → 没有合适的 Java → 静默下载，只在进度条上体现。"自定义 Java 路径"藏在实例设置的高级选项里。

### 4.4 管理层：设置里那一节

不做"关闭自动下载"的开关。这一层对用户隐形是设计目标，一个能关掉它的开关等于提供一个使游戏无法启动的按钮。

**按大版本分组，不是平铺安装路径。** 用户的问题是"我缺什么"，平铺列表只回答"我装了什么"。一台机器上同时需要 8、17、21 是常态（跨版本的实例各有各的要求），所以分组是唯一能同时回答这两个问题的形状。缺失的大版本也占一组，组内没有条目——那一行正是要让人看见的。

需求取自已落盘的版本元数据（`javaVersion.majorVersion`，权威下限），元数据缓存落地后离线也读得到；读不到时按版本号推，界面须说明那是估计值。

**JDK 与 JRE**：`release` 文件里的 `IMAGE_TYPE` 给出答案，没有该字段的按 `bin/javac` 是否存在判断。**它不参与选择**——运行游戏两者没有区别，在没有偏好的地方发明偏好只会让选择结果难以解释。它的唯一作用是让同一大版本下的两条记录不至于完全相同。

**架构降级**必须在界面上说明。Apple Silicon 上的 x64 Java 经 Rosetta 可以运行，但性能明显下降，而这一点在别处看不出来。

---

## 5. 启动流程（`launch`）

### 5.1 命令行拼装

```
java [JVM 参数] -cp [classpath] <mainClass> [游戏参数]
```

**变量替换**：实现一个简单模板器，替换 `${...}` 占位符；未知变量保留原样（部分加载器有私有变量）。核心变量：

```
auth_player_name  auth_uuid  auth_access_token  user_type(=msa/legacy)
game_directory  assets_root  assets_index_name  version_name  version_type
natives_directory  classpath  launcher_name  launcher_version
resolution_width  resolution_height（feature 控制）
```

**Classpath**：

- 分隔符 Windows 为 `;`，其余为 `:`。
- 顺序 = inheritsFrom 合并后的 libraries 顺序 + 末尾 client jar。顺序错误在某些版本会加载错 ASM。
- Windows 命令行长度上限问题：通过 CreateProcess 直接传参一般可行，极端整合包可退化为 `@argfile`（Java 9+）。

### 5.2 平台特例（必须处理）

- **macOS**：`-XstartOnFirstThread`（LWJGL 3 硬性要求；rules 里通常有，但老版本要自己补）。
- **Log4Shell**（1.7–1.18.1）：加 `-Dlog4j2.formatMsgNoLookups=true`，并使用 `logging.client` 指定的 Mojang 替换版 log4j 配置（`-Dlog4j.configurationFile=...`）。
- **Linux Wayland/X11**、高分屏缩放等问题按 issue 反馈再补，不预先堆参数。

### 5.3 版本隔离

`game_directory` 指向实例私有目录（存档、mods、config、resourcepacks、封面种子全部归属实例）；`assets_root` 与 `libraries` 全局共享。这与"每实例独立身份"的设计天然一致。

```
<data_root>/
  assets/            共享
  libraries/         共享
  runtimes/          共享（自动下载的 Java）
  instances/<id>/
    instance.json    实例元数据（版本、加载器、封面种子、设置覆盖）
    .minecraft/      game_directory
```

### 5.3.1 外部游戏目录

大多数人把启动器和 `.minecraft` 放在一起。实例描述里的 `external` 有值时，那个实例的游戏文件就在那个目录里，Fern 只持有一份指向它的描述——**不移动、不复制、不删除任何游戏文件**，删实例只删我们那份 `instance.json`。

两种布局都要认，判断错的后果是存档看起来消失了（游戏会在另一个目录新建一份空的）：

```
共用      <root>/saves                    所有版本共享（官方启动器）
版本隔离  <root>/versions/<id>/saves      每个版本一套（HMCL、PCL2）
```

判据是**哪一边真的有东西**，不是问用户——他多半不知道上一个启动器是怎么设的。加载器从库坐标认（`net.minecraftforge:forge:`），不从目录名认：目录名是启动器起的，用户可以随手改。

实现上只有一个入口：`DataPaths::scoped(external, version_id)` 返回一份指向那个目录的 `DataPaths`，每条链路在入口处换一次，下游三十来处路径拼接一个字都不用改。散在下游判断「这个实例是不是外部的」，漏掉的那一处会把文件写进错误的目录。

数据根本身也可以跟着可执行文件走（`DataPaths::resolve`）：旁边有 `.minecraft` 或 `fern-portable` 标记时即为便携模式。

### 5.4 进程管理

- spawn 后 **stdout/stderr 必须持续读取**——不读会因管道缓冲满而卡死游戏进程。
- 日志流解析 log4j XML 事件格式（`<log4j:Event>`），提取 level/logger/message 供日志查看器着色过滤。
- **启动成功判定**：日志出现窗口初始化标志（如 `Setting user:` 之后的 LWJGL/GL 初始化行），或简化为进程存活 15 秒。成功后启动器可按设置最小化。
- **异常退出**：
  1. 抓退出码 + stderr 尾部；
  2. 读 `crash-reports/` 最新文件；
  3. 模式匹配可识别原因（缺 mod 依赖、mod 版本冲突、Java 版本不符、显卡驱动、内存不足 `OutOfMemoryError`），输出人话 + 原始报告折叠展示。崩溃分析规则做成数据文件，便于持续补充。

---

## 6. 性能调优

原则：默认值做好比堆开关重要，克制。

### 6.1 内存自动策略

- 基线 = 物理内存的 1/4，下限 2 G。
- 检测到大型整合包（mods 目录体积 / mod 数量阈值）上调至 6–8 G。
- 上限不超过物理内存一半。
- UI 上一个滑杆允许覆盖，实例级设置。

### 6.2 GC

- Java 17+ 客户端场景：G1 + 温和参数即可：

```
-XX:+UseG1GC -XX:G1NewSizePercent=20 -XX:G1ReservePercent=20 -XX:MaxGCPauseMillis=50
```

- Aikar flags 是服务端调优方案，不照搬。
- ZGC 对大内存整合包做成实验选项（`-XX:+UseZGC`，Java 17+）。

### 6.3 其他

进程优先级、独显选择（Windows 混合显卡）等做成实例级高级设置，默认不动。

---

## 7. 开发路线

按依赖关系排序，每个里程碑可独立验收：

| # | 里程碑 | 内容 | 验收标准 |
|---|---|---|---|
| 1 | 基础契约 | 数据目录、实例模型、事件模型、真实 JSON fixtures | 核心 crate 建立，fixtures 可被测试读取 |
| 2 | 元数据模型 | version JSON 解析、inheritsFrom 合并、rules 求值 | 单测覆盖新旧两代格式 |
| 3 | 下载器基础设施 | 并发下载、sha1 校验、镜像切换、进度事件 | 压测下载 assets 全量无损坏 |
| 4 | 原版启动（离线） | 文件补全 + 命令拼装 + 进程管理 + 基础日志 | 离线模式启动 1.21 与 1.12.2 各一个版本进入主界面 |
| 5 | Java 管理 | 发现 + 自动下载 | 空环境下全自动完成 Java 获取并启动 |
| 6 | 认证 | 微软 device code flow + authlib-injector | 正版进入在线服务器；LittleSkin 皮肤正确显示 |
| 7 | Fabric | profile 合并安装 | 装 Fabric + 一个 mod 启动成功 |
| 8 | NeoForge | processors 执行 | 装一个中型整合包启动成功 |
| 9 | 崩溃分析与性能 | 日志解析、规则匹配、内存和 GC 策略 | 常见崩溃给出正确人话提示 |

**并行说明**：Azure 应用注册与白名单申请在里程碑 1 阶段就提交（先写认证链前半段触发 403）；审批等待期间 3–4 不受任何阻塞，5 的 authlib-injector 部分也可先行。

---

## 附：关键外部资源

- 版本总表：`https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
- BMCLAPI 镜像文档：`https://bmclapidoc.bangbang93.com`
- 微软认证流程参考：minecraft.wiki 的 Microsoft authentication 页面
- Azure 应用白名单申请表：`https://aka.ms/mce-reviewappid`
- authlib-injector：`https://github.com/yushijinhun/authlib-injector`
- Mojang Java 运行时清单：`https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json`
