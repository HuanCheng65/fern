# 给协作者（含 AI 代理）的说明

仓库结构和常用命令在 [README](README.md)。这里只记**踩过才知道**的事——
编译得过、测试全绿，但仍然是错的那些。

## 改完必须跑

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 .github/check-platform-deps.py
cd fern-ui && pnpm check && pnpm build
```

CI 跑的就是这五条。动过 `fern-site` 或 `fern-kit` 的话再加一条——官网那条流水线
构建不过就发不出去，而它没有别的检查环节：

```bash
cd fern-site && pnpm build
```

## 陷阱

- **`fern-ui/src-tauri` 是独立的 Cargo workspace**，`cargo test --workspace` 不含它，
  且要 WebKitGTK dev 包（`libwebkit2gtk-4.1-dev` `libsoup-3.0-dev` `librsvg2-dev`
  `libdbus-1-dev`）。装不了的机器只能写个临时 crate 把命令体搬过去编译，验类型和
  serde 约束——验不了宏展开和 `invoke_handler!` 列表。
- **Tauri 命令要出现两次**：`#[tauri::command]` 标注和 `invoke_handler![]` 里的名字。
  漏后者不报编译错，运行时才说「命令不存在」。改完对一遍：

```bash
python3 -c "
import re; s=open('fern-ui/src-tauri/src/lib.rs').read()
c=set(re.findall(r'#\[tauri::command\]\s*(?:pub )?(?:async )?fn (\w+)', s))
r=set(re.findall(r'^\s{12}(\w+),?\$', s, re.M))
print('没注册:', sorted(c-r), '| 没定义:', sorted(r-c))"
```

- **Tauri 插件最多要出现四处**：`src-tauri/Cargo.toml` 依赖、`run()` 里的
  `.plugin(...)`、`capabilities/default.json` 权限项、前端 npm 包。少了第三处只在
  运行时报 not allowed。`tauri-plugin-dialog` 四处都要（只给 `dialog:allow-open`）；
  `tauri-plugin-single-instance` 只要前两处，且**必须是第一个** `.plugin(...)`。
- **`[target.'cfg(...)'.dependencies]` 会圈走它后面所有的依赖行。** 插到
  `[dependencies]` 中间，后面的依赖就全成了平台专属，另一个平台上 unresolved import
  刷屏。`check-platform-deps.py` 为此而写，新增平台专属依赖要同步改白名单。
- **面向用户的新文案不写在 Rust 里。** 崩溃诊断和启动预检查的后端只发文案 id 和参数
  （`crash.<规则 id>` / `preflight.<类型>`），句子在 `fern-ui/src/lib/i18n/`；
  `message_ids()` 是契约，`cargo test` 会据此重写 `keys.ts`。已有的中文不做一次性
  搬迁，改到哪一屏顺手搬哪一屏。
- **加一条崩溃规则 = `fern-core/rules/crash.toml` 追加一段 + 一份
  `rules/fixtures/<id>.txt` + 文案表两句话。** 三步都不能少，三条测试分别卡这三样。
- **快照入库绝不用硬链接**——同一个 inode，而 Minecraft 原地重写 region 文件，仓库里
  那份会跟着变。只能 reflink 或老实复制，理由见
  [docs/fern-backup-design.md](docs/fern-backup-design.md)。
- **跨语言结构体改字段名不报编译错**（`Snapshot` `RestoreScope` `RestoreMode`
  `Restored` `Usage` `Exported` 直接过给 TypeScript），只会在运行时变 `undefined`。
  `the_interface_sees_the_field_names_it_expects` 钉住了 JSON 形状，改了要同步
  `fern-ui/src/lib/backup.ts`。
- **JSON 命名**：类型标签用 snake_case（`launch_stage`），数据字段用 camelCase
  （`instanceId`）。改了 Rust 侧要同步 `fern-ui/src/lib/` 里的类型。
- **补全与启动必须用同一份合并后的元数据**（`version::resolve`），否则会出现
  「文件明明下好了却说缺」这种最难查的问题。

## 验证

**这个项目最严重的 bug，单元测试一个都抓不到**——GC 参数少了
`-XX:+UnlockExperimentalVMOptions`、NeoForge processors 与补全的顺序、启动变量少了
`library_directory`。问题都在接缝上，而单测覆盖的是纯函数。

所以改了启动链路（`prepare` / `launch` / `loader` / `forge` / `java`）就真跑一次：写个
临时的 `fern-core/examples/*.rs`，建实例、补全、启动，看事件流和
`logs/instances/<id>/launch.log`，验完删掉。

## 设计与文案

约束在 [docs/frond-design-system.md](docs/frond-design-system.md)，核心链路在
[docs/launcher-core-dev.md](docs/launcher-core-dev.md)。几条容易违反的：

- **不编造数据。** 没有数据源就不要在界面上留位置，功能没做就说没做。
- **`fern-kit/src/styles/` 是设计系统的唯一来源**（尺度在 `scale.css`，配色在
  `surface-dark.css` 与 `brand.css`），组件里不写魔数；浮层统一用 `ui/Dialog.svelte`。
- **标志几何是算出来的**（`fern-kit/src/ui/mark.ts`），改了走线跑
  `python3 .github/make-icons.py`，不手工导 PNG。**macOS 是例外**：
  `icons/macos/compiled/` 下的产物要在 macOS 上用 Icon Composer 重新导出。
- **平台专属的东西不进 `tauri.conf.json`。** `bundle.icon` 安全，`bundle.resources`
  不是——写在公共 conf 里三个平台无条件全带。
- **品牌色不是界面色板。** `--pine` `--paper` `--fern` `--sprout` 只用在身份该出现的
  地方；界面颜色由背景层生成并注入，写死 `--accent` 等于关掉「UI 向背景学色彩」。
- **`components/Backdrop.svelte` 不要动。** 它每隔几秒重写 `:root` 上的色板变量，
  主题变量必须写在 `document.body.style` 上。
- **文案用中性书面语**，一句话说完一件事（「显卡驱动起不来」→「图形环境不可用」）。
  错误信息也算文案——`anyhow!` 会原样冒到界面上。

## 安全

- **令牌不落盘、不进 webview**，存系统钥匙串（`credentials.rs`）；钥匙串用不了就直说，
  **不要**退回明文文件。交给界面的 `AccountView` 里一个令牌都没有。
- **所有来自网络的字符串都可能被拼进路径**（版本 id、加载器 profile id、Maven 坐标、
  资源索引名、压缩包条目名）。关口是 `version::is_safe_id`、`fern_meta::maven_path`、
  `fern_download::safe_join`，新增路径拼接照样要过。

## 提交

- 用 `git commit --no-gpg-sign`（避免触发 SSH Agent 授权）。
- 提交信息用英文，遵循 Conventional Commits。标题说清改了什么，正文只写为什么，
  能不写就不写。
- 报告结论要诚实：没验过的说没验过，交叉编译检查不等于跑过。

用户看得见的改动在提交信息末尾加一条尾注，**整个提交信息里只有这一行是中文**，
一个提交最多一条（需要两条就说明该拆开）：

```
feat(update): check for updates on a channel

Release-Note: 可在设置中选择更新通道，测试版可更早获得新功能。
```

一句话，**不带句末标点**（更新日志是一列变化，不是一段文章），60 字以内，从用户
能观察到的变化写起，不出现内部名词和版本号，不写「优化了使用体验」这种空话。
只有 `feat` / `fix` / `perf` 该有尾注——不是说重构一定不改变界面，而是那条变化
值得写进更新日志的话，它就该有自己的提交。

`.github/draft-changelog.py` 把尾注汇成 `CHANGELOG.md` 的「未发布」小节。写法由
`.github/check-release-notes.py` 查，分三处，硬的只有两头：

| 时机 | 怎么处理 | 为什么 |
| --- | --- | --- |
| `git commit` | 挡（`.githooks/commit-msg`） | 信息还没定型，改一条是免费的 |
| CI 推送 | 只提醒 | 推出去的提交信息是只读的，在这儿挡等于要求改写历史 |
| `release.py` 发版 | 挡 | 查的是 `CHANGELOG.md` 里的条目，改它只是改一个文件 |

钩子由 `pnpm install` 装上（根目录的 `prepare` 脚本设 `core.hooksPath`）。
