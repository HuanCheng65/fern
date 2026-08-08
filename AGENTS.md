# 给协作者（含 AI 代理）的说明

仓库结构和常用命令在 [README](README.md)。这份文件只写**踩过才知道**的东西——
那些编译得过、测试全绿，但仍然是错的情况。

## 改完必须跑

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 .github/check-platform-deps.py
cd fern-ui && pnpm check && pnpm build
```

CI 跑的就是这五条，本地过了 CI 基本就过。

## 这个仓库的陷阱

**`fern-ui/src-tauri` 是独立的 Cargo workspace，`cargo test --workspace` 不包含它。**
它要 WebKitGTK 的 dev 包才编得动（Linux 上是 `libwebkit2gtk-4.1-dev`
`libsoup-3.0-dev` `librsvg2-dev` `libdbus-1-dev`）。装不了的机器上，验证 Tauri
命令的办法是写一个只依赖 `fern-core` 的临时 crate，把命令体原样搬过去编译，
并打印出实际的 JSON 确认字段名和前端的 TypeScript 对得上。这能验类型、serde
约束和字段名，**验不了宏展开和 `invoke_handler!` 列表**——那两样只有真编译才知道。

**Tauri 命令要在两个地方出现：** `#[tauri::command]` 标注，以及
`invoke_handler![]` 里的名字。漏掉后者不会有任何编译错误，只会在运行时报
「命令不存在」。改完用这个对一遍：

```bash
python3 -c "
import re; s=open('fern-ui/src-tauri/src/lib.rs').read()
c=set(re.findall(r'#\[tauri::command\]\s*(?:pub )?(?:async )?fn (\w+)', s))
r=set(re.findall(r'^\s{12}(\w+),?\$', s, re.M))
print('没注册:', sorted(c-r), '| 没定义:', sorted(r-c))"
```

**Tauri 插件最多要在四个地方出现：** `src-tauri/Cargo.toml` 的依赖、`run()` 里的
`.plugin(...)`、`capabilities/default.json` 的权限项、以及前端的 npm 包。少了
第三个不会有编译错误，只会在运行时报「not allowed」。

- `tauri-plugin-dialog`：四处都要，只授予 `dialog:allow-open`。
- `tauri-plugin-single-instance`：只要前两处——它不暴露命令，也没有前端包。
  但它**必须是第一个** `.plugin(...)`，否则第二个进程会先把窗口建出来再退出。

**`[target.'cfg(...)'.dependencies]` 段的位置。** 它会把**它后面所有**的依赖行
一起圈进去。往 `[dependencies]` 中间插一个这种段，后面的依赖就全变成平台专属
的——本平台照常编译，另一个平台上「unresolved import」刷屏。这事真发生过。
`check-platform-deps.py` 就是为它而写，新增平台专属依赖时要同步改脚本里的白名单。

**面向用户的新文案不要写在 Rust 里。** 崩溃诊断和启动前预检查的后端只发文案
id 和参数（`crash.<规则 id>` / `preflight.<类型>`），句子在
`fern-ui/src/lib/i18n/`。`fern-core` 的 `message_ids()` 是这条契约，`cargo test`
会照着它重写 `i18n/keys.ts`；文案表声明成 `Record<BackendMessage, Message>`，
少一条就是 `pnpm check` 的编译错误。界面里已有的中文不做一次性搬迁——改到哪一屏
顺手搬哪一屏。

**加一条崩溃规则 = 往 `fern-core/rules/crash.toml` 追加一段 + 放一份
`rules/fixtures/<id>.txt` + 在文案表里写两句话。** 没有第四步，也少不了任何一步：
三条测试分别查「每条规则有 fixture 且命中它」「没有孤儿 fixture」「干净日志不许
命中任何规则」。

**快照入库绝不能用硬链接。** 硬链接和源文件是同一个 inode，而 Minecraft 原地
重写 region 文件——对象仓库里那份内容会跟着变，哈希对不上，快照在没人察觉的
时候就坏了。只能 reflink（写时复制，任何一边写入都会断开共享）或者老实复制。
理由与其余取舍在 [docs/fern-backup-design.md](docs/fern-backup-design.md)。

**跨语言的结构体改字段名不会有编译错误。** `Snapshot`、`RestoreScope`、
`RestoreMode`、`Restored`、`Usage`、`Exported` 这几个直接过给 TypeScript，改错
只会在运行时变成 `undefined`。`fern-core` 里的
`the_interface_sees_the_field_names_it_expects` 把 JSON 形状钉死了，改了要同步
改 `fern-ui/src/lib/backup.ts`。

**事件与命令的 JSON 命名规则：** 类型标签用 snake_case（`launch_stage`、
`preparing_java`），数据字段用 camelCase（`instanceId`）。前者是判别用的常量，
后者在 JS 里当属性读。改了 Rust 侧要同步改 `fern-ui/src/lib/` 里对应的类型。

**补全与启动必须用同一份合并后的元数据**（`version::resolve`）。两边各算各的，
就会出现「文件明明下好了却说缺」这种最难查的问题。

## 验证策略

**这个项目最严重的 bug，单元测试一个都抓不到。** 已经发生过的：

- GC 参数少了 `-XX:+UnlockExperimentalVMOptions`，每一次启动都失败——单测只检查
  参数列表里有什么，而这个错误只有真把 java 跑起来才看得见
- NeoForge 的 processors 要拆原版 client jar，而补全先装加载器后下 jar
- 启动变量少了 `library_directory`，NeoForge 的模块路径成了字面量，报出来是
  `InaccessibleObjectException`，和真正的原因隔了四层

规律是：问题都在**接缝**上，而单测覆盖的是纯函数。所以——

**改了启动链路（`prepare` / `launch` / `loader` / `forge` / `java`），就真的跑一次。**
写一个临时的 `fern-core/examples/*.rs`，建实例、补全、启动，看事件流和
`logs/instances/<id>/launch.log`。验完删掉。这比再写十个单测有用。

## 设计与文案

设计约束在 [docs/UI_DESIGN.md](docs/UI_DESIGN.md)，核心链路在
[docs/launcher-core-dev.md](docs/launcher-core-dev.md)。几条容易违反的：

- **不编造数据。** 没有数据源的字段就不要在界面上留位置——一个永远显示 0 的格子
  比没有这个格子更糟。功能没做就说没做，不写「敬请期待」。
- **`src/styles/tokens.css` 是设计系统的唯一来源。** 间距、字号、圆角、动效
  全走 CSS 变量，不要在组件里写魔数。浮层统一用 `Overlay.svelte`，玻璃、影子、
  进出动画只在那里定义一次。
- **图标是算出来的，不是画出来的。** 标志的全部几何是 7×9 网格上的八段走线
  （`fern-ui/src/lib/mark.ts` 与 `docs/fern-brand-system.html`）。改了走线要跑
  `python3 .github/make-icons.py` 重新生成应用图标和 favicon，不要手工导出
  PNG——那样迟早和规范对不上。**macOS 那份是例外**：`icons/macos/compiled/` 下的
  `Assets.car` 和 `Fern.icns` 是 Icon Composer 的产物，脚本只生成喂给它的
  `Fern.icon/Assets/fern-mark.svg`，剩下一步要在 macOS 上手工重新导出。走线一变
  脚本就会提醒——不导出的话，Linux 和 Windows 换了新标志，macOS 还是旧的。
- **平台专属的东西不要写在 `tauri.conf.json` 里。** `bundle.icon` 是安全的，
  打包器按平台挑；`bundle.resources` 不是，写在公共 conf 里三个平台无条件全带。
  那 1.7MB 的 `Assets.car` 曾经就这么跟着 deb 和 Windows 一起发。
- **品牌色不是界面色板。** `--pine` `--paper` `--fern` `--sprout` 只用在身份该
  出现的地方（图标、字标、还没有背景可学时的那一帧）。界面的颜色由背景层生成
  并注入，把 `--accent` 写死成蕨绿等于把「UI 向背景学色彩」这条设计原则关掉。
- **`components/Backdrop.svelte` 不要动。** 它每隔几秒会重写 `:root` 上的色板
  变量，所以主题变量写在 `document.body.style` 上才不会被盖掉。
- **文案用中性书面语。** 不口语、不劝导、一句话说完一件事。「显卡驱动起不来」
  要写成「图形环境不可用」。错误信息也算文案——核心里那些 `anyhow!` 会原样冒到
  界面上。

## 安全

- **令牌不落盘、不进 webview。** 访问令牌和 refresh token 存系统钥匙串
  （`credentials.rs`）。钥匙串用不了就直接说用不了，**不要**退回明文文件。
  交给界面的是 `AccountView`，里面一个令牌都没有。
- **所有来自网络的字符串都可能被拼进路径**：版本 id、加载器 profile 的 id、
  Maven 坐标、资源索引里的名字、压缩包里的条目名。已有的关口是
  `version::is_safe_id`、`fern_meta::maven_path` 和 `fern_download::safe_join`，
  新增路径拼接时照样要过。

## 提交

- 用 `git commit --no-gpg-sign`（避免触发 SSH Agent 授权）。
- **提交信息用英文，遵循 Conventional Commits。** 标题一行说清改了什么，正文
  只写**为什么**——diff 已经说了怎么改的。不复述代码，不写过程流水账。
- **正文能不写就不写，要写就用最短的篇幅说清楚。禁止小作文。** 语言简明、
  清晰、专业，不讲黑话，不啰嗦，不口语化不情绪化。
  仓库里 2026-08 之前有一批长篇散文风格的提交，那是旧习惯，不要照着模仿。
- 报告结论要诚实：没验过的说没验过，交叉编译检查不等于跑过。

## 更新日志

**更新日志是界面的一部分**，不是开发记录：它进更新清单的 `notes`，显示在设置的
关于页里。所以它不能从提交标题生成——`fix(ci): install from the workspace root`
不该出现在用户眼前。

改动如果值得让用户知道，就在提交信息末尾加一条尾注。**整个提交信息里只有这一行
是中文**（多语言以后再说）：

```
feat(update): check for updates on a channel

Release-Note: 可在设置中选择更新通道，测试版可更早获得新功能。
```

**一个提交最多一条尾注。** 需要两条就说明这个提交该拆开。

`.github/check-release-notes.py` 在 CI 里校验格式，`.github/draft-changelog.py`
把尾注汇成 `CHANGELOG.md` 的「未发布」小节。

### 这一句话怎么写

分类不用自己写，由提交类型决定：`feat` → 新增，`fix` → 修复，`perf` → 改进。
其余类型（`chore` / `refactor` / `docs` / `build` / `ci` / `test` / `style`）
**不该有**尾注——用户看不见的改动不进更新日志。

语法：

1. **一句话，句号结尾，不换行，不用列表。** 长度不超过 60 个字符。
2. **以用户能观察到的变化开头**，不是以模块名或实现开头。
   - ✅ 启动失败时会指出是哪个模组导致的。
   - ❌ 重构了崩溃分析的规则匹配逻辑。
3. **主语省略，或者是 Fern。** 不写「我们」，不写「本次更新」。
4. **不出现内部名词。** mixin、classpath、清单、灰度、self-replace 这些不能
   出现；界面上本来就有的词可以（实例、模组、快照、Java）。
5. **不写版本号、issue 号、贡献者。** 那些在别处。
6. 能给一句可照做的建议就给，但**不超过一句**。
7. **禁止空话。** 「优化了使用体验」「修复了一些已知问题」「提升了稳定性」
   这类句子没有信息量。**写不出具体是什么，就说明这条不该进更新日志。**

措辞标准和界面文案同一套（见「设计与文案」一节）：正式、清晰、简明、中性，
用户听得懂的人话，不讲黑话，不口语化不情绪化。
