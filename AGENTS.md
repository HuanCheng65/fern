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

**`[target.'cfg(...)'.dependencies]` 段的位置。** 它会把**它后面所有**的依赖行
一起圈进去。往 `[dependencies]` 中间插一个这种段，后面的依赖就全变成平台专属
的——本平台照常编译，另一个平台上「unresolved import」刷屏。这事真发生过。
`check-platform-deps.py` 就是为它而写，新增平台专属依赖时要同步改脚本里的白名单。

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
  PNG——那样迟早和规范对不上。
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
- 报告结论要诚实：没验过的说没验过，交叉编译检查不等于跑过。
