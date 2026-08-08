# 自更新

本文定义 Fern 的自更新机制。实现会落在 `fern-core/src/update/` 与 `fern-ui/src-tauri`。

写在最前面的一条：**Windows 上不做安装版。** 任何要求「先装再更新」的方案，无论多成熟，在这里都不成立。

「不做安装版」和「产物是不是单个文件」是两件事。后者单独决定过一次：**单个 `Fern.exe`**，
理由和放弃掉的那条路记在 §8.1——因为「不做安装版」并不自动推出「必须是一个文件」，
这一步的推理不能省。

---

## 1. 先钉死约束

| 约束 | 来源 | 后果 |
|---|---|---|
| Windows 坚决不做安装版 | 产品决定 | 排除 NSIS / MSI 一族 |
| Windows 产物是单个 `Fern.exe` | 单独决定过一次，见 §8.1 | 落盘要自己写（§3.1），也就没有增量更新 |
| macOS 发 `.app` / `.dmg`，Linux 发 `.deb` / `.AppImage` | 平台形态，不是我们的选择 | 这两个平台可以用现成的更新链路 |
| 便携模式下数据可能就在 exe 旁边（`fern-portable` 标记，见 `data/mod.rs`） | 已有设计 | 替换 exe 绝不能碰同目录的其它东西 |
| 用户主要在中国大陆 | 下载源已经默认 BMCLAPI（`fern-download`） | GitHub Release 直连不能当唯一下载源 |
| 「套壳 Fern 是玩家中木马的方式」（README 的许可附加条款） | 已经写进 LICENSE 立场 | 更新包必须验签，且这条不可关闭 |

第五条决定了这件事的性质：**自更新是一条把任意字节写进用户机器并执行的通道。**
它是整个应用里攻击面最大的一处，比下载模组更危险——模组进的是 JVM，更新包进的是我们自己的进程。

---

## 2. 为什么 `tauri-plugin-updater` 不能直接用

先说结论：**Windows 那一半用不了，另外两个平台正好可用。** 这是查了插件源码之后的判断，不是猜的。

看 [`plugins/updater/src/updater.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs) 的安装路径：

**Windows** 只认两种扩展名：

```rust
if ext == Some(OsStr::new("exe")) {
    return Ok(WindowsUpdaterType::nsis(path, None));
} else if ext == Some(OsStr::new("msi")) {
    return Ok(WindowsUpdaterType::msi(path, None));
}
```

拿到之后用 `ShellExecuteW` 把它当**安装器**拉起来（MSI 走 `msiexec.exe`），然后
[官方文档](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/updater.mdx)明说：
「The application is automatically exited when the install step is executed」。
`.zip` 也只是解出里面的安装器，不是「解压覆盖」。

也就是说，如果我们把便携版 `Fern.exe` 填进清单，插件会把**新版本当安装器**从临时目录里跑起来，
然后退掉当前进程——用户看到的是「更新完了，但版本号没变」，而磁盘上什么都没被替换。
这不是配置问题，是这条路径根本没有「替换二进制」这个动作。

**macOS** 收 `.app.tar.gz`，解到临时目录，`fs::rename` 换掉原 bundle，权限不够时
用 `osascript` 提权跑 `rm -rf && mv -f`。这套逻辑（尤其是提权那段）自己重写不划算。**可用。**

**Linux** AppImage 是就地替换（把当前 AppImage 改名备份 → 解包新的 → 改回来）；
deb/rpm 是写到临时目录再 `dpkg -i` / `rpm -U`，用 pkexec → 图形密码框 → sudo 逐级提权。
AppImage **可用**；deb 不该走这条路（见 §4）。

---

## 3. 结论：复用它的前半段，只自己写 Windows 的落盘

插件的 `Update` 把两件事分开了：

```rust
async fn download(...) -> Result<Vec<u8>>   // 下载 + 验签，返回字节
fn install(&self, bytes: impl AsRef<[u8]>) -> Result<()>
```

`download` 的文档原话是 "Downloads the updater package, **verifies it** then return it as bytes"——
**验签发生在返回之前**。所以我们能拿到「已经确认是我们签的」的字节，而不必自己实现 minisign 校验、
清单解析、`{{target}}`/`{{arch}}` 替换、版本比较这一整套。

于是分工是：

| 平台 | 检查 + 下载 + 验签 | 落盘 |
|---|---|---|
| Windows | `tauri-plugin-updater` | **我们自己**：`self_replace` |
| macOS | `tauri-plugin-updater` | `tauri-plugin-updater`（`.app.tar.gz`） |
| Linux AppImage | `tauri-plugin-updater` | `tauri-plugin-updater` |
| Linux deb | 只检查，不下载 | 不更新，见 §4 |

前端不能用插件的 JS `downloadAndInstall()`——它在 Windows 上会走错路径。
只暴露我们自己的 Tauri 命令（`update_check` / `update_apply`），Windows 分支在 Rust 侧岔开。

> 注意 `UpdaterBuilder` 在 Windows 上直接构造会因为 `current_exe_args` 为空而 panic
> （[plugins-workspace#2335](https://github.com/tauri-apps/plugins-workspace/issues/2335)），
> 用 `app.updater()` 走插件的正常入口。

### 3.1 Windows 上到底怎么换掉一个正在跑的 exe

Windows 不允许**删除或覆盖**正在运行的可执行文件，但**允许改名**。所有便携版自更新都建立在这一条上。

[`self-replace`](https://github.com/mitsuhiko/self-replace)（mitsuhiko，rye / uv 在用）把这件事封好了。
按它的[文档](https://docs.rs/self-replace/latest/self_replace/)，Windows 上的流程是：把当前 exe 挪开腾出文件名 →
新文件写到原路径 → 另外复制一份带 `FILE_FLAG_DELETE_ON_CLOSE` 的副本当清理进程，等父进程退出后删掉挪开的那个。
Unix 上就是一次 `rename()` 原子替换，正在运行的进程继续用已经打开的 inode。

自己重写这段没有意义：它的复杂度全在 Windows 的清理进程链上，而那正是最容易写出「每次更新在目录里
留一个 `.Fern.exe.old`」的地方。

`self_replace::self_replace(&new_exe)?` 之后调 `app.restart()`。

### 3.2 便携形态特有的坑

这些坑安装版没有，所以现成方案不会替我们挡：

**目录可能不可写。** 用户会把 exe 放进 `Program Files`、下载目录、U 盘、网络盘、只读挂载点。
**更新前先试写**：在 exe 同目录建一个临时文件再删掉。失败就不要进入下载流程，直接退化成
「打开下载页」——下了一半才发现写不进去，比一开始就说清楚糟得多。

**杀软。** 一个未签名的 exe 在运行时改写自己所在目录里的 exe，是行为检测的教科书特征。
这不是能靠代码绕开的，只能靠签名（§7）。至少要保证失败路径是干净的：替换失败时原文件必须还在原地。

**同一台机器上有好几份。** 便携版会被复制来复制去。更新只作用于 `current_exe()`，别去猜别的副本在哪。

**游戏正在跑。** Fern 退出时会不会带走游戏进程，取决于 `process.rs` 的实现——
**有游戏在跑时不提示重启**，等它退了再说。这条比更新本身重要。

**路径里有中文、空格、或在 OneDrive 同步目录下。** 前两个 `self-replace` 处理得了；
OneDrive 会在替换的瞬间锁文件，属于「试写通过但替换失败」，走失败路径。

---

## 4. Linux 的 deb 不自更新

`.deb` 是包管理器装的，进程对 `/usr/bin` 没有写权限，Tauri 的做法是 `pkexec dpkg -i` 弹密码框。
一个启动器在用户玩游戏的时候弹系统提权密码框，是**比不更新更坏的体验**，而且教会用户对这种弹窗点确定。

deb 用户只提示「有新版本」并给下载链接。想做得更好就以后开一个 apt 源，那是发行工作不是客户端工作。

判断当前是哪种形态：AppImage 下 `APPIMAGE` 环境变量存在，deb 下不存在。

---

## 5. 清单与分发

分发放 **Cloudflare R2**。选它的理由只有一个，但很硬：**出网流量 $0/GB**。
一个启动器的更新包是纯下载负载，在按流量计费的对象存储上这是最贵的一项。
免费额度是 10 GB 存储 / 100 万次 Class A / 1000 万次 Class B 每月，且不过期。

**必须绑自定义域名。** `r2.dev` 那个托管域名官方明说是给测试用的，
有可变速率限制（超了返回 429）、带宽也可能被限流，而且没有缓存和 Workers 的能力。

### 5.1 一条贯穿全篇的原则：客户端只知道一个 URL

R2 是对象存储，没有计算。所以「动态那部分挂哪」看起来是个必须先回答的问题——
但它其实可以往后推很久，**只要客户端从第一天起就把那个地址当成「一个端点」而不是「一个文件」**。

同一个路径，今天是 R2 上的一个静态 JSON，明天可以是一个 Worker，客户端一个字节都不用改。
这是整个分发设计里唯一一个**必须现在就做对**的决定，因为做错了要发新版客户端才能纠正——
而更新机制本身出问题时，正是最发不出新版客户端的时候。

### 5.2 布局

```
dl.fern.huanchengfly.top/                              ← R2 + 自定义域名
├── stable/manifest.json                  ← Cache-Control: max-age=60
├── beta/manifest.json                    ← 同上
└── release/0.2.0/                        ← Cache-Control: max-age=31536000, immutable
    ├── Fern-windows-x86_64.exe
    ├── Fern-windows-x86_64.exe.sig
    ├── Fern-darwin-universal.app.tar.gz
    ├── Fern-darwin-universal.app.tar.gz.sig
    ├── Fern-linux-x86_64.AppImage
    └── Fern-linux-x86_64.AppImage.sig
```

**版本号进路径，发布过的文件永不覆盖。** 于是二进制可以设成永久缓存，
也就永远不会出现「传了个同名的新文件，但一部分用户还在 CDN 上拿旧的」——
那是这类系统里最难查的一种问题，而它完全可以用命名规避掉。

只有 `manifest.json` 是可变的，缓存 60 秒。

平台 key：`windows-x86_64`、`darwin-aarch64`、`darwin-x86_64`、`linux-x86_64`。
macOS 是 universal 包，两个 key 指向同一个文件。

### 5.3 静态先行：大部分「需要动态」的东西其实不需要

清单是 R2 上的静态 JSON 就够了，一直够到很后面。逐条算过：

| 想要的能力 | 真的要服务端算吗 |
|---|---|
| 分平台分架构 | 不要。静态清单的 `platforms` 表就是干这个的 |
| 灰度放量 | **不要。** 见 §5.4，客户端就能做，而且更好 |
| 紧急停止推送 | 不要。改一个静态文件，60 秒后全网生效 |
| 强制最低版本 | 不要。清单里加一个字段 |
| 测试版通道 | 不要。见 §5.5，是另一个路径的另一个文件 |
| 按地区给不同下载源 | **要**，但有更好的办法（§5.6） |
| 知道多少人停在旧版本 | 要——**但这是遥测，不该混进更新检查**（见 `fern-telemetry-design.md`） |

所以顺序是：**先纯静态，等真的撞上后两行再加 Worker。** Workers 免费额度是 10 万请求/天，
按「每 6 小时检查一次」算能撑到两万五千个活跃安装，到那时候这个项目已经养得起 $5/月了。

### 5.4 灰度：完全在客户端做

已经验证过：Tauri 的 `RemoteRelease` **没有** `deny_unknown_fields`，
清单里多出来的字段会被静默忽略。所以自定义字段可以和 Tauri 认的字段放在同一个文件里。

于是灰度是这样的——清单里写：

```json
{
  "version": "0.2.0",
  "rollout": 30,
  "critical": false,
  "minVersion": "0.1.0",
  "platforms": { "...": { "url": "...", "signature": "..." } }
}
```

每个安装在本地存一个**随机的 0–99**（装的时候生成一次，此后不变）。
`bucket < rollout` 才接受这个版本。推进放量就是改静态文件里的一个数字：30 → 60 → 100。

这比服务端分桶好，不只是省一个 Worker：

- **客户端什么都不用发。** 不需要把安装 id 塞进请求头，于是更新检查不会偷偷变成一条遥测通道——
  这跟「遥测默认关」的立场是一致的（原来的方案里我打算复用遥测 id，那是错的）。
- 灰度桶和遥测 id **无关**，各自独立，互相不泄露。
- 出问题时把 `rollout` 改回 0，60 秒生效，不用碰任何代码。

实现上：先自己 `GET` 一次 manifest 读 `rollout` / `critical` / `minVersion`，
决定要不要继续；要继续再把**同一个 URL** 交给 `tauri-plugin-updater`。
第二次请求命中 CDN 缓存，代价是一次 HTTP 往返。
（`UpdaterBuilder` 也有 `version_comparator()`，能把判断塞进插件内部，但那样读不到自定义字段，
不如自己先读一遍直白。）

出了问题就把清单改回去，**客户端不做降级**。已经更新的人靠下一个 hotfix 救，不靠回滚——
自动降级本身就是一条攻击路径（见 §7）。

### 5.5 通道

`endpoints` **在运行时给**，不写死在 `tauri.conf.json` 里——`UpdaterBuilder::endpoints(Vec<Url>)`
接受运行时传入。因为通道是用户设置：

```
https://dl.fern.huanchengfly.top/{channel}/manifest.json
```

`channel` 是 `stable` / `beta`，存在设置里。

**没选过时跟随当前构建**：版本号带预发布段就走测试通道。设置里存的是
`Option<Channel>`，而不是一个默认为 `Stable` 的枚举——「没选过」和「选了稳定版」
必须区别对待。装了测试版构建却默认查稳定通道的人，只会一直看到
「当前版本高于该通道」，而他手上那一份本来就来自测试通道。选定之后以选定的为准，
包括从测试版切回稳定版。

**版本号用 SemVer 预发布段**：`0.2.0-beta.1 < 0.2.0`。
这一条让通道几乎不需要额外逻辑——beta 用户在 `0.2.0` 正式发布时会自然收到它，
因为它比 `0.2.0-beta.3` 大。不用写「毕业」这种特殊处理。

**从 beta 切回 stable 是降级，不做。** §7 的「拒绝版本号 ≤ 当前」在这里同样生效：
切回去的用户停在原地，等 stable 追上来。界面要照实说这件事，别让用户以为切换没生效。

**真正的约束不是下载管道，是磁盘上的格式。** 这是做通道最容易翻车的地方，
而且翻车的时候数据已经没了：

> **beta 和 stable 共用同一个数据目录，所以 beta 绝不允许做不向后兼容的数据格式变更。**

一个 beta 把设置或快照索引迁移成新格式，用户切回 stable，旧版本读不懂——
这不是「体验不好」，是数据损坏。要做破坏性迁移只有两条路：让 beta 用独立的数据目录
（那基本等于两个应用，也就失去了 beta 测试的意义），或者**把迁移拆成两步**——
先发一个「读得懂新旧两种格式」的 stable，等它铺开，再发写新格式的 beta。后者是对的做法。

**CI 从 tag 推导通道，不开第二条流水线。** `v0.2.0-beta.1` → 传到 `beta/`，
`v0.2.0` → 传到 `stable/`。发布动作就两个：往 `release/<version>/` 传文件，
改一个 `manifest.json`。

再往后如果要 `nightly`，它是第三个路径，但**nightly 不该有自更新**——
每天变的东西自动往用户机器上装，风险和收益不成比例。nightly 只发链接。

### 5.6 中国大陆

先说清楚现状，别抱幻想：**Cloudflare 的中国网络要 Enterprise 套餐 + 单独订阅 + 每个域名的
ICP 备案 + 合作方 JD Cloud 的内容审核。** 对我们不成立。免费/Pro 计划下，
大陆用户的请求走境外 PoP（香港 / 日本 / 美西），能通，但速度和稳定性看运气。

但这件事有个很干净的解法，而且它是**签名机制白送的**：

> **因为每个更新包都经过 minisign 验签，镜像完全不需要被信任。**

所以做法是给 `endpoints` 一个数组，Tauri 会依次尝试：

```
https://mirror.fern.cn/stable/manifest.json     ← 国内镜像，清单里的 url 指向国内
https://dl.fern.huanchengfly.top/stable/manifest.json        ← R2，canonical
```

**顺序按界面语言决定**（中文在前，其它语言 R2 在前）。这一招同时解决了两件事：
一个源挂了自动走下一个，而下一个源的清单里的下载地址也是就近的——
因为每个镜像有自己的一份清单，指向自己的文件。两边的内容由签名保证一致。

镜像本身可以是阿里云 OSS / 腾讯云 COS（要备案），也可以什么都不做——
**P0 阶段只有 R2**，等真的收到速度投诉再加。加镜像不需要改客户端，只需要改
运行时拼 endpoints 的那几行。

不要复用 `fern-download` 的镜像重写逻辑：那份逻辑是给 Mojang 的域名写的，
更新包的来源是我们自己，两件事只是看起来像。

### 5.7 失败要安静

端点挂了、DNS 污染了、清单是坏的 JSON——**检查更新失败绝不打断任何事**，连提示都不要，
静默等下一次。一个因为更新服务器挂了而弹错误框的启动器，比一个不会自更新的启动器差。

### 5.8 签名

minisign，Ed25519。私钥只存在 GitHub Actions 的 secret 里（`TAURI_SIGNING_PRIVATE_KEY`，
注意它必须是导出的环境变量，`.env` 文件不生效），公钥编进二进制。

`bundle.createUpdaterArtifacts: true` 让 bundler 生成 `.app.tar.gz` 和各自的 `.sig`。
Windows 的便携 exe 不经过 bundler，用 `pnpm tauri signer sign` 单独签。

**私钥泄漏是这个项目唯一的灾难级事故**——它等于可以给所有 Fern 用户推送任意代码。
所以：私钥不进仓库、不进本地开发环境、只在打 tag 的 CI job 里出现；轮换方案要提前想好
（新版本内置新旧两个公钥，一个大版本之后去掉旧的）。

**R2 的写入凭据同理**：只给一个作用域限定在这一个 bucket 的 API token，
而且它的权限不该包含「删除」——发布只需要写新对象和覆盖两个 manifest。

公钥（key ID `15FB285F4CA661B8`）已经写在 `tauri.conf.json` 的 `plugins.updater` 里。
JSON 不能写注释，所以那一段的三件事记在这里：

- **公钥进仓库是对的。** 它本来就要编进每一个二进制，藏起来没有意义，
  而放在这里意味着 P1 只剩「加依赖 + `.plugin()` + capability」三行。
- **插件还没加，这段配置现在没有读者。** `PluginConfig` 是
  `HashMap<String, JsonValue>`，多一个键不会让配置解析失败——已经对着
  tauri-utils 的源码确认过，不是猜的。
- **那里的 `endpoints` 只是个默认值。** 真正生效的是运行时按通道拼出来的那一个
  （§5.5），因为通道是用户设置，而配置文件是编译期的。

**私钥一旦编出带公钥的版本就不能再丢了**：装出去的客户端只认这一把钥匙的签名。
在那之前丢了还能重新生成，之后就是「所有已安装的客户端永久收不到更新」。

### 5.9 版本号体系

自更新把版本号从「给人看的字符串」变成了**一个会被机器比大小的值**，所以它得先定死。

#### 一处真相

**产品版本 = `fern-ui/src-tauri/Cargo.toml` 的 `version`。** 别处的 `version` 都不是它：

| 位置 | 是什么 |
|---|---|
| `fern-ui/src-tauri/Cargo.toml` | **产品版本。改这里。** |
| `fern-ui/src-tauri/tauri.conf.json` | 必须和上面**一字不差**，由 `build.rs` 在编译期强制 |
| 根 `Cargo.toml` 的 `workspace.package.version` | 钉在 `0.0.0`。那几个库不发布，版本号没有读者 |
| `fern-ui/package.json` | 钉在 `0.0.0`。npm 要求有这个字段，仅此而已 |

第二行是这一节存在的理由。那两个数字**来源不同、读者也不同**：关于页显示的是
`CARGO_PKG_VERSION`，而自更新比较的是 `PackageInfo::version`——按 tauri-codegen 的逻辑，
`config.version` 有值就用它，没值才回落到 `CARGO_PKG_VERSION`。

于是发版时漏改一处的症状是：**关于页显示 `0.2.0`，更新器却以为自己还是 `0.1.0`，
每次检查都提示同一个更新，装完再提示。** 没有任何测试会失败——两个文件各自都合法，只是不相等。
所以 `build.rs` 里有一个 `the_two_version_numbers_must_agree()`，对不上直接编译失败。

问过一次「那把 `version` 从 `tauri.conf.json` 删掉不就只剩一个了」：不行。`tauri-build`
只在 `config.version` 有值时才写 Windows 的版本资源，删掉之后 exe 的「属性 → 详细信息」
一片空白，而一个没有版本资源的未签名 exe 在杀软眼里更可疑。

#### 号段的含义

SemVer，`0.MINOR.PATCH`：

- **MINOR** —— 有新功能，或有用户能察觉的行为变化，或**任何磁盘格式的变化**。
- **PATCH** —— 只修 bug，磁盘上的东西一个字节都不变。

第三条是给 §5.5 那条通道纪律用的：**「数据格式变了」和「MINOR 涨了」是同一件事**，
于是「beta 不许做不兼容的格式变更」有了一个能在 review 里指出来的判据。

`1.0.0` 的条件现在就写下来，否则它永远发不出去：**三平台的自更新都真跑通过（§10），
且数据目录的格式在一个完整的 MINOR 周期里没有再变过。** 在那之前一直是 `0.x`。

#### 预发布

`0.2.0-beta.1`、`0.2.0-beta.2`……SemVer 规定预发布版小于同号正式版，
所以 `0.2.0-beta.3 < 0.2.0` 是天然成立的，通道之间不需要任何额外逻辑（§5.5）。

只用 `beta` 一个词，不引入 `alpha` / `rc`——通道只有两条，第三个词没有对应的去处。

**不用 build metadata（`0.2.0+a1b2c3d`）。** SemVer 明确规定比较时忽略 `+` 之后的部分，
放进去会造出「看起来不同但比较相等」的版本号，而这正是更新器最不该遇到的东西。
构建标识另有其人：`FERN_COMMIT` 和 `FERN_BUILD_DATE`，关于页已经在显示了。

#### tag

tag 是 `v` + 版本号，一字不差：`v0.2.0`、`v0.2.0-beta.1`。CI 从 tag 名推导通道
（带 `-` 的进 beta，否则进 stable），并且**先校验 tag 与 crate 版本一致**再干别的——
打错 tag 要在第一步就停下，而不是把一个版本号错误的包传上 R2。

---

## 6. 什么时候更新，怎么打断用户

原则：**更新是启动器的事，玩家在意的是游戏。**

- 启动后延迟 30 秒检查一次，之后每 6 小时一次。启动的那一刻网络要留给别的事。
- 检查到新版本**不弹窗**，只在设置入口和标题栏放一个不打扰的标记。
- 下载在后台静默进行，下完了才提示，且提示语是「下次启动生效」而不是「立即重启」。
- 有游戏在运行时，连提示都不出。
- 用户能关掉自动更新（设置里 `about` 段），关掉之后仍然检查、仍然显示标记，只是不下载。
- **验签不提供开关。**

安全更新需要例外通道：清单里带一个 `critical: true`，这种版本允许提示得更强一点。
但仍然不强制——强制更新是自己给自己留的后门。

---

## 7. 安全

| 威胁 | 对策 |
|---|---|
| 中间人替换更新包 | minisign 验签（插件在 `download` 里做），不可关闭 |
| 回滚攻击：重放一个旧的、已签名的、有漏洞的版本 | 签名只证明来源，不证明新鲜度。**客户端拒绝版本号 ≤ 当前的更新包**，这一条要自己加 |
| 清单被替换成指向旧版本 | 同上；另外清单里带 `sha256`，下载后二次校验 |
| 私钥泄漏 | §5.8；轮换方案提前设计 |
| 更新中断导致 exe 损坏 | `self-replace` 的替换是先写新文件再换名，中断的最坏结果是目录里多一个临时文件 |
| 下载源被投毒 | 验签兜底，所以 CDN 不需要被信任 |

### 7.1 签名证书的现状（这是个真问题）

**Windows：** Azure Artifact Signing（原 Trusted Signing）Basic 档 $9.99/月，
能直接集成进 GitHub Actions，不需要硬件令牌。但**个人开发者目前只对美国和加拿大开放**，
组织身份也只覆盖美加欧英。对国内主体基本不可用。
退路是传统 OV 证书（需要公司实体 + 硬件令牌或云 HSM，年费几百到上千）。
另外要有预期：[即使签了名，SmartScreen 的信誉仍然要靠下载量累积](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)，
签名不等于立刻没有警告。

**macOS：** Apple Developer Program $99/年。不公证的话用户要右键打开，
Sequoia 之后这条路还更绕了。**但对自更新有个有利的细节**：quarantine 属性是下载器（浏览器）打的，
我们自己 `reqwest` 下载再写盘的文件不带这个属性——所以**第一次安装的门槛远高于后续更新**。
这反过来说明自更新对 macOS 用户的价值最大。

结论：签名是发行成本不是技术债，但它决定了自更新的体验上限。**在没有签名之前也要把自更新做出来**，
因为不做的话用户的替代方案是「去某个网盘下一个 Fern.exe」——那才是真正的风险。

---

## 8. 选型的分叉，与评估过不做的

### 8.1 为什么是单个 exe，而不是一个解压即用的文件夹

先说清楚这一步的性质：**「不做安装版」不等于「必须是单个文件」。** 一个解压即用、
不写注册表、不建快捷方式的文件夹，同样不是安装版。所以这是一次独立的选择，不是前一条的推论——
把它当推论会顺手把 Velopack 这条路排除掉，而它是这个领域里最成熟的方案。

结论是 **A（单个 exe）**，但下面这张表要留着，因为本节末尾那个条件成立时结论会翻。

| | A. 单个 `Fern.exe` | B. 解压即用的文件夹（Velopack） |
|---|---|---|
| 用户拿到的 | 一个文件 | 一个 zip，解开是三个 exe 加一个 `current/` |
| 落盘方式 | `self-replace`（§3.1），我们自己写 | `Update.exe` 换掉整个 `current/` |
| 要写的代码 | 试写检测 + 自替换 + 重启，约百来行加一堆平台坑 | 调 `UpdateManager`，几十行 |
| 增量更新 | 无 | 有 |
| 灰度、回滚 | 自己做（§5.4） | 框架自带 |
| macOS / Linux | 靠 `tauri-plugin-updater`（§3） | Velopack 也管，但要换掉 Tauri 的整条打包链 |
| 多一个要签名的 exe | 否 | 是（`Update.exe` 也会被 SmartScreen 和杀软看） |
| 和 `fern-portable` 标记的关系 | 无风险 | **要小心**：`current/` 每次更新整个被替换，数据和标记文件必须在它外面 |
| 现在的 CI 改动 | 小（已经是 `--no-bundle` 出单 exe） | 大（引入 `vpk`，三平台的产物形态都要重排） |

选 **A** 的理由不是「便携就该是一个文件」这种口号，而是三条具体的：

1. **现状离 A 更近。** CI 已经在出单个 `fern-ui.exe`，A 只加客户端代码，B 要重排整条发布链。
2. **B 的「整个 `current/` 被替换」和已有的便携模式设计有交互。** `data/mod.rs` 的
   `fern-portable` 标记是「放在可执行文件旁边」，而 B 里「可执行文件旁边」有两层含义
   （stub 那层和 `current/` 那层），选错一层就是更新一次数据没了。这类 bug 单测抓不到。
3. **多一个 `Update.exe` 就多一个签名和信誉问题**，而签名恰好是我们最缺的东西（§7.1）。

**放弃掉的东西要记清楚**，不然以后会有人以为 A 是免费的：没有增量更新、没有框架级的灰度和回滚
（灰度得自己做，见 §5.4）、§3.2 那一堆 Windows 便携坑全部由我们自己承担。

**结论翻转的条件只有一个：产物本来就不止一个文件。** 比如哪天要带固定版本的 WebView2 运行时——
那时 A 省下的那点简洁已经不存在了，而 B 白送的增量、灰度、回滚还在，应该立刻改用 B。
真要换，换的只是 §3 那一段：P0（§9）两条路完全共用。

### 8.2 不做的

**Sparkle。** macOS 上的标准答案，但要和 Tauri 的进程模型、XPC 服务的签名顺序缠在一起，
而 Tauri 自己的 macOS 路径已经够用。收益不抵成本。

**增量/差分更新。** Tauri 不支持。Tauri 的产物本来就只有十几 MB，配 CDN 之后省下的那几 MB
不值得引入一套 patch 机制——而 patch 应用失败是最难查的一类问题。

**强制更新。** 见 §6。

---

## 9. 实施顺序

**P0 —— 先修发布链，客户端只提示。** 代码已经写完，落在：

| 东西 | 在哪 |
|---|---|
| 版本号一处真相 + 编译期比对 | `fern-ui/src-tauri/build.rs` |
| 通道、清单、灰度、`decide` | `fern-core/src/update/mod.rs` |
| 通道与自动检查的设置项 | `fern-core` 的 `UpdateSettings`、`fern-ui/src/lib/persist.ts` |
| `check_update` 命令 | `fern-ui/src-tauri/src/lib.rs` |
| 界面与定时检查 | `fern-ui/src/lib/update.svelte.ts`、设置的「关于」一节 |
| 发布流水线 | `.github/workflows/release.yml` + `.github/build-manifest.py` |

**还差四件只有人能做的事**，做完这条链才通：

1. `pnpm tauri signer generate` 生成密钥对。私钥进 `TAURI_SIGNING_PRIVATE_KEY`，
   公钥留着——P1 加更新器插件时要写进 `tauri.conf.json`。
2. 定下真正的域名，同步改 `update/mod.rs` 的 `DEFAULT_ENDPOINT` 和仓库变量
   `UPDATE_BASE_URL`（流水线的 `plan` 会对一遍，不一致就停）。
3. 开 R2 bucket，绑自定义域名（不能用 `r2.dev`，§5），配好那几个 secret。
4. 用一个 `v0.1.1-beta.1` 走一遍全流程，确认 beta 通道的清单真的出现在 R2 上。

做完之后「发版」就从手动下载 Actions artifact 变成了打一个 tag。

> **一个已知的 CI 缺口**：`package.yml` 的 `cargo fmt --all` 和
> `cargo clippy --workspace` **都不覆盖 `fern-ui/src-tauri`**——它是独立的
> Cargo workspace，AGENTS.md 只提了 `cargo test` 不包含它，其实 fmt 和 clippy
> 也一样。那边现在有没排过版的代码。修它要在 CI 里单独加一条，
> 而那条需要 WebView 的开发库，所以只能挂在 package 任务上。

**P1 —— 真的自更新。** 已完成：`update_apply` 用插件下载验签，Windows 走
`self-replace` 自己落盘并先试写，macOS / AppImage 交给插件，deb 只给下载地址。
装完**不自动重启**，重启是单独一个命令，而且有游戏在跑时界面不给那个按钮。

**更新日志**跟着这一步一起做了：`CHANGELOG.md` 按版本分节，发布时
`build-manifest.py` 把对应那一节取出来写进清单的 `notes`，界面在更新那一行显示它。
没写那一节的版本照常发得出去——一次疏漏不该挡住发版，而界面在没有 `notes`
时什么都不显示，不占位置。

**P2 —— beta 通道对外开放。** 技术上 P0 就已经支持了，P2 做的是设置项、
关于页的通道标记、以及 §5.5 那条数据格式的纪律怎么落到 review 流程里。

**P3 —— 国内镜像。** 加一个 endpoint，按语言排序。等真的收到速度投诉再做。

**P4（可能永远不做）—— Worker。** 只有撞上 §5.3 表里最后两行才需要。

---

## 10. 怎么验证

**这个功能的 bug 单元测试一个都抓不到**，理由和 AGENTS.md 里说的启动链路完全一样：问题全在接缝上。

必须真的做的验证，每个平台各一遍：

1. 打两个版本（`0.1.0` → `0.1.1`），架一个本地 HTTP 端点当更新源。
2. 真的跑 `0.1.0`，让它更新，重启，**看关于页的版本号和提交哈希是不是都变了**。
3. Windows 上额外验：把 exe 放进只读目录、放进带中文的路径、更新过程中拔网线、
   更新后检查目录里有没有留下临时文件。
4. macOS 上额外验：从 `.dmg` 装进 `/Applications` 之后再更新（权限路径和从下载目录跑不一样）。
5. Linux 上验 AppImage 的就地替换，以及 deb 形态下**没有**触发下载。

签名要单独验一次：**故意用错误的私钥签一个包，确认客户端拒绝它**。
这条永远不能只靠读代码确认。
