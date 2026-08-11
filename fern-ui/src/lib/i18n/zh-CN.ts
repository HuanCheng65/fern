/**
 * 简体中文。
 *
 * 措辞要求：正式、简明、清晰、自然，语气中性。不说废话，不用黑话，不替用户
 * 表达情绪。每一条都要说清**发生了什么**，能给建议就给一句能照做的。
 *
 * `backend` 里的键由 `keys.ts` 生成，那份清单来自后端的规则表与预检查。少写
 * 一条会是编译错误——`satisfies Record<BackendMessage, Message>` 在管这件事。
 */

import type { BackendMessage } from './keys'

export interface Message {
  title: string
  /** 一两句话。说清是什么，以及能做什么。 */
  detail: string
}

/** 加载器的显示名。后端传的是 `fabric` 这样的取值，显示成什么由这里决定。 */
const loader = {
  vanilla: '原版',
  fabric: 'Fabric',
  quilt: 'Quilt',
  forge: 'Forge',
  neoforge: 'NeoForge',
}

/**
 * 代码里出现的那几类调用叫什么。后端传的是 `run-program` 这样的取值。
 *
 * 说的是**代码里引用了什么**，不是「这个模组会做什么」——一段引用了
 * `Runtime.exec` 的代码不一定真的执行到那里。措辞照这个分寸来。
 */
const capability = {
  'run-program': '启动外部程序',
  'load-code': '在运行时加载代码',
  network: '建立网络连接',
  deserialize: '还原序列化对象',
  'public-address': '写死的公网地址',
}

const backend = {
  // ── 崩溃分析 ────────────────────────────────────────────────────────────
  'crash.out-of-memory': {
    title: '内存不足',
    detail:
      '游戏需要的内存超过了当前上限（{kind}）。可以在实例设置中提高内存上限，或减少模组数量。',
  },
  'crash.java-too-old': {
    title: 'Java 版本过低',
    detail:
      '{class} 需要更新的 Java：它按 class 文件版本 {needed} 编译，当前运行时最高支持 {current}。可以在实例设置中更换 Java。',
  },
  'crash.unrecognized-jvm-option': {
    title: 'JVM 参数无法识别：{option}',
    detail:
      'Java 不接受这个参数，进程没有启动。它来自设置中的「额外 JVM 参数」，删除或更正后即可启动。',
  },
  'crash.fabric-unresolved-dependency': {
    title: '缺少前置模组：{need}',
    detail: '{mod} {version} 需要 {need}（{range}），当前实例中没有能满足它的版本。',
  },
  'crash.fabric-suggested-fix': {
    title: 'Fabric 给出的解决办法：安装 {need}',
    detail: '按 Fabric 的计算，安装 {need} {version} 或更高版本可以让这套模组组合成立。',
  },
  'crash.fabric-incompatible-mods': {
    title: '模组之间不兼容',
    detail:
      'Fabric 拒绝加载当前的模组组合。日志中「Mod resolution failed」之后的几行列出了具体是哪些。',
  },
  'crash.forge-missing-dependency': {
    title: '缺少前置模组：{need}',
    detail: '{by} 需要 {need} {range}，当前实例中没有安装。',
  },
  'crash.forge-mandatory-dependencies': {
    title: '缺少前置模组',
    detail: '有模组要求的前置没有安装，或者版本不符。日志中该行之后列出了具体条目。',
  },
  'crash.forge-dependency-missing': {
    title: '缺少前置模组：{dependency}',
    detail: '{mod} 需要 {dependency}（{requirement}），当前实例中没有安装。',
  },
  'crash.forge-dependency-version': {
    title: '{dependency} 版本不符',
    detail: '{mod} 需要 {dependency} {requirement}，当前安装的是 {current}。',
  },
  'crash.forge-mod-failed-to-load': {
    title: '{mod} 加载失败',
    detail:
      '{mod} 在加载过程中出错，游戏因此没能启动。日志中该模组的段落里有具体的异常。移除它或更换版本可以继续。',
  },
  'crash.forge-mods-incompatible': {
    title: '模组之间不兼容',
    detail: '有模组与游戏版本或彼此不兼容。日志中列出了具体是哪些。',
  },
  'crash.duplicate-mod': {
    title: '{name} 安装了多份',
    detail: '同一个模组存在多个文件，加载器无法确定使用哪一个。删除多余的一份：{path}',
  },
  'crash.forge-duplicate-mods': {
    title: '有模组安装了多份',
    detail: '同一个模组存在多个版本。日志中该行之后列出了具体条目。',
  },
  'crash.mixin-failure-named': {
    title: '{config} 的修改没能应用',
    detail:
      '{config} 要修改的游戏代码与它预期的不一致。通常是与另一个模组冲突，或者它不适配当前游戏版本。',
  },
  'crash.mixin-failure': {
    title: '模组之间冲突',
    detail:
      '有模组要修改的游戏代码与它预期的不一致。常见于两个模组修改了同一处，或某个模组不适配当前游戏版本。',
  },
  'crash.graphics-driver-crash': {
    title: '显卡驱动崩溃',
    detail:
      '崩溃发生在 {library} 中，这是显卡驱动本身，与游戏和模组无关。更新显卡驱动通常可以解决。',
  },
  'crash.graphics-unavailable': {
    title: '无法建立图形环境',
    detail:
      '游戏没能初始化 OpenGL。常见原因是显卡驱动过旧、通过远程桌面运行，或显卡不支持这个版本所需的 OpenGL。',
  },
  'crash.corrupt-archive': {
    title: '有文件损坏',
    detail:
      '某个 jar 文件无法读取，通常是下载中断留下的不完整文件。可以校验一次游戏文件；如果是模组，删除后重新安装。',
  },
  'crash.broken-config': {
    title: '配置文件有误',
    detail: 'config 目录下的某个配置文件无法解析。删除它之后，模组会重新生成一份默认配置。',
  },
  'crash.port-in-use': {
    title: '端口被占用',
    detail: '游戏需要的端口已被其他程序占用，通常是上一次的游戏进程尚未退出。',
  },
  'crash.duplicate-asm-classes': {
    title: 'classpath 上有两份 ASM',
    detail:
      '{first} 与 {second} 同时在 classpath 上，加载器无法确定用哪一份，因此拒绝启动。这来自版本描述文件里重复列出的库，重新安装一次加载器通常可以解决。',
  },
  'crash.forge-launchwrapper-java8': {
    title: '这个 Forge 版本在启动时中断',
    detail:
      '1.7.10 之前的 Forge 存在一处缺陷，在现有的 Java 8 上启动时必然中断，与安装了哪些模组无关。Fern 会在安装时修改对应的类文件来绕开它；仍然出现这条信息，说明这一步没有生效，可尝试重新检查游戏文件。',
  },
  'crash.java-without-a-jvm': {
    title: 'Java 安装不完整',
    detail:
      '这份 Java 缺少虚拟机本身（{path}），只有一个启动程序，通常是下载中途被打断。可以在实例设置中改用另一份 Java，或删除这份后重新下载。',
  },

  // ── 启动前预检查 ────────────────────────────────────────────────────────
  'preflight.no-loader': {
    title: '当前实例没有模组加载器',
    detail: 'mods 目录中有 {count} 个模组，但原版不会加载它们。为这个实例安装加载器，或移除这些模组。',
  },
  'preflight.duplicate': {
    title: '{mod} 安装了 {count} 份',
    detail: '同一个模组存在多个文件：{files}。保留其中一份即可。',
  },
  'preflight.wrong-loader': {
    title: '{mod} 不适用于{instanceLoader}',
    detail: '它是 {modLoader} 的模组，而这个实例使用{instanceLoader}。',
  },
  'preflight.wrong-game-version': {
    title: '{mod} 可能不适配 {minecraft}',
    detail: '它声明支持的版本是 {range}。',
  },
  'preflight.missing-dependency': {
    title: '缺少前置模组：{dependency}',
    detail: '{mod} 需要 {dependency}，当前实例中没有安装。',
  },
  'preflight.incompatible': {
    title: '{mod} 与 {other} 不兼容',
    detail:
      '{mod} 声明不兼容 {range} 的 {other}，当前安装的是 {version}。升级 {other}，或移除其中一个。',
  },
  'preflight.disabled-dependency': {
    title: '{dependency} 已被禁用',
    detail: '{mod} 需要它。在模组列表中重新启用即可。',
  },
  'preflight.wrong-java': {
    title: '{mod} 需要另一个版本的 Java',
    detail:
      '它声明的 Java 版本是 {range}，而这个实例会使用 Java {java}。加载器会因此拒绝启动。可以在实例设置中更换 Java。',
  },

  'preflight.stale-jvm-argument': {
    title: '{argument} 在 Java {java} 上已不可用',
    detail:
      '这个参数从 Java {removedIn} 起被移除，带着它启动虚拟机会直接失败。启动时会自动忽略它；可以在实例设置的额外 JVM 参数中删除。',
  },

  // 事前兼容规则拦下来的。能自己处理掉的那些不出现在界面上，只有确实无法
  // 启动、需要用户做决定的才在这里说话。
  'preflight.compat.a-headless-java-cannot-open-a-window': {
    title: '这份 Java 无法显示游戏窗口',
    detail:
      '当前选中的 Java 不含图形组件，多为系统软件源中的精简版本。游戏会在创建窗口时中断。请安装完整版本的 Java，或在实例设置中改用另一份。',
  },
  'preflight.compat.a-32-bit-java-cannot-hold-a-large-heap': {
    title: '这份 Java 是 32 位的，可用内存有限',
    detail:
      '32 位的 Java 最多只能使用约 1 GB 内存，超出后虚拟机无法启动。内存上限已自动限制在这个范围内。改用 64 位的 Java 可以解除限制。',
  },
  'preflight.compat.modlauncher-8-breaks-on-a-new-java-8': {
    title: '这个 Forge 版本无法在当前 Java 上启动',
    detail:
      'Forge 36.2.25 之前的版本依赖一个在较新的 Java 8 中已被修改的内部接口，启动时会中断。本机没有可用的旧版 Java 8，请将 Forge 更新到 {loaderVersion} 或更高版本。',
  },
  'preflight.compat.old-fml-sorts-while-iterating': {
    title: '这个 Forge 版本需要修改后才能启动',
    detail:
      '1.7.10 之前的 Forge 存在一处缺陷，在现有的 Java 8 上启动时必然中断。Fern 会在安装时修改对应的类文件，而这一步没有完成。',
  },
  'preflight.compat.a-jar-mod-fails-the-tamper-check': {
    title: '这个实例的游戏文件被模组修改过',
    detail:
      '1.6 之前那种模组会直接改写游戏本体，Forge 检查到之后会拒绝启动。Fern 会在启动时关闭这项检查，而这一步没有完成。',
  },
  'preflight.compat.old-fml-refuses-an-unsigned-client': {
    title: '这个 Forge 版本会拒绝启动游戏',
    detail:
      '这些版本会检查游戏文件的数字签名，而当年的签名方式已不被现在的 Java 认可。Fern 会在启动时关闭这项检查，而这一步没有完成。',
  },

  // ── 文件对账 ────────────────────────────────────────────────────────────
  // 只陈述事实：什么变了、和上次记录的差别在哪。不做判断，不猜测原因——改动
  // 出自用户自己、另一个启动器还是别的程序，我们分辨不了，也不该暗示。
  'integrity.rewritten-together': {
    title: '{count} 个模组文件在同一时段被改写',
    detail:
      '它们的内容发生了变化，但模组声明的版本号没有改变。安装新版本会同时改变版本号。',
  },
  'integrity.ledger-broken': {
    title: '变更记录不完整',
    detail:
      '这个实例的变更记录从第 {line} 条起无法校验，其后的内容不足以采信。此前的记录仍然有效。',
  },
  'integrity.left-upstream': {
    title: '{count} 个文件不再对应 Modrinth 上的版本',
    detail:
      '改动之前，这些文件可以在 Modrinth 上查到对应的发布版本，改动之后查不到。其中包括 {file}。',
  },
  'integrity.silent-rewrite': {
    title: '{file} 的内容已改变',
    detail: '文件内容发生了变化，但模组声明的版本号没有改变。安装新版本会同时改变版本号。',
  },
  'integrity.gained-capability': {
    title: '{count} 个文件出现了此前没有的调用',
    detail:
      '这些文件的内容变了，模组声明的版本号没有改变，而其中的代码新增了以下调用：{capability}。其中包括 {file}。',
  },

  // ── 快照 ────────────────────────────────────────────────────────────────
  // 标题是快照列表里那一行的名字，说明回答「它为什么在这里」。
  'snapshot.manual': {
    title: '手动',
    detail: '在快照列表中手动拍下的。这类快照不会被自动清理。',
  },
  'snapshot.before-mod-change': {
    title: '改动模组之前',
    detail: '安装、删除、启用或禁用模组前自动拍下，记录的是改动前的状态。',
  },
  'snapshot.after-session': {
    title: '游戏结束之后',
    detail: '游戏正常退出后自动拍下，记录这次游玩的进度。',
  },
  'snapshot.before-launch': {
    title: '启动之前',
    detail: '距上一张快照已超过六小时，启动前补拍一张。',
  },
  'snapshot.before-restore': {
    title: '恢复之前',
    detail: '恢复其他快照前自动拍下，用来撤销那次恢复。',
  },
  // 触发快照的那件事。类别（上面五条）回答「哪一类时刻」，这里回答「那一次
  // 是什么」——列表行的标题优先用这几句，人找快照找的是事件，不是类别。
  'snapshot.about.install': { title: '安装 {name} 之前', detail: '' },
  'snapshot.about.remove': { title: '删除 {name} 之前', detail: '' },
  'snapshot.about.enable': { title: '启用 {name} 之前', detail: '' },
  'snapshot.about.disable': { title: '停用 {name} 之前', detail: '' },
  'snapshot.about.session': { title: '游玩 {duration}之后', detail: '' },

  'snapshot.skipped.too-large': {
    title: '文件过大',
    detail: '单个文件超过 512 MB，未纳入快照。',
  },
  'snapshot.skipped.transient': {
    title: '可重新生成',
    detail: '日志、崩溃报告和缓存不纳入快照，它们会自行生成。',
  },
  'snapshot.skipped.not-selected': {
    title: '不在快照范围内',
    detail: '快照只包含存档、配置、模组、资源包和截图，这一项不在其中。',
  },

  // ── 启动进度 ────────────────────────────────────────────────────────────
  // 标题是进度条上那一行字。stage 是阶段名，track 是阶段里并排跑的支线，
  // note 是随做随换的注脚——都不需要 detail：进度不解释自己，说完这一刻
  // 就翻篇。
  'job.stage.resolve-version': { title: '读取版本信息', detail: '' },
  'job.stage.install-loader': { title: '安装 {loader} {version}', detail: '' },
  'job.stage.download-files': { title: '补全游戏文件', detail: '' },
  'job.stage.prepare-launch': { title: '准备启动', detail: '' },
  'job.track.download': { title: '下载游戏文件', detail: '' },
  'job.track.java-runtime': { title: '准备 Java', detail: '' },
  'job.track.snapshot': { title: '拍摄快照', detail: '' },
  'job.track.natives': { title: '解压平台组件', detail: '' },
  'job.track.account': { title: '刷新账户凭据', detail: '' },
  'job.track.mods': { title: '检查模组', detail: '' },
  'job.note.downloading': { title: '检查并下载 {count} 个文件', detail: '' },
  'job.note.retry': { title: '重试 {count} 个文件', detail: '' },
  'job.note.asset-index': { title: '读取资源索引', detail: '' },
  'job.note.legacy-assets': { title: '整理旧版资源', detail: '' },
  'job.note.loader-inspect': { title: '读取 {loader} {version} 的安装信息', detail: '' },
  'job.note.loader-profile': { title: '安装 {loader} {version}', detail: '' },
  'job.note.forge-core': { title: '摆放 Forge 的核心库', detail: '' },
  'job.note.forge-libraries': { title: '下载安装期需要的库', detail: '' },
  'job.note.forge-processor': { title: '安装 {index}/{count}：{name}', detail: '' },
  'job.note.java-prepare': { title: '准备 Java 运行时（{component}）', detail: '' },
  'job.note.java-download': { title: '下载 Java {version}', detail: '' },
  'job.note.java-adoptium-query': { title: '向 Adoptium 查询 Java {major}', detail: '' },
  'job.note.java-adoptium-download': { title: '下载 Temurin {name}（{size} MB）', detail: '' },
  'job.note.java-extract': { title: '解压 Java 运行时', detail: '' },
  'job.note.authlib': { title: '下载 authlib-injector {version}', detail: '' },
} satisfies Record<BackendMessage, Message>

/**
 * 界面自己的文案。
 *
 * 按属性访问（`ui.about.notOfficial`）而不是按字符串键查表：写错的名字是编译
 * 错误，读代码时也仍然看得见那句话在哪。字符串键那一套留给后端发过来的 id，
 * 因为那些只有运行时才知道。
 *
 * 关于页是第一屏搬过来的。其余屏改到哪一屏顺手搬哪一屏，不做一次性全量搬迁。
 */
const ui = {
  about: {
    tagline: '一个 Minecraft 启动器。',
    unknownBuild: '未知构建',
    copy: '复制',
    copied: '已复制',
    repository: '源码仓库',
    issues: '反馈问题',
    update: {
      check: '检查更新',
      checking: '正在检查',
      // 只有用户自己点过之后才会看到这句。自动检查失败时不提示。
      failed: '无法连接更新服务器，稍后将自动重试。',
      upToDate: '已是最新版本。',
      // 一条尚未发布过版本的通道。这不是故障，措辞不能像故障——
      // 稳定通道在首个正式版发布之前，所有人看到的都是这一句。
      noRelease: '该通道尚未发布版本。',
      available: '可更新至',
      critical: '安全更新，建议尽快安装。',
      apply: '更新',
      updating: '正在更新',
      download: '下载',
      // 更新已装好，但要重启才生效。
      installed: '更新已安装，重启后生效。',
      restart: '立即重启',
      // 有游戏正在运行时不给重启按钮，并说明原因。
      restartBlocked: '有游戏正在运行，请在游戏退出后重启 Fern。',
      // 由包管理器安装的版本不自更新。
      managed: '该版本由系统包管理器安装，请通过包管理器更新。',
      // 从测试版切回稳定版时出现。不写清楚，用户会以为切换未生效。
      aheadOfChannel: '当前版本高于该通道的最新版本，将保持不变，直至该通道发布更高版本。',
      needsFullDownload: '该版本无法从当前版本直接更新，请下载完整安装包。',
      noBuild: '当前平台暂无可用版本。',
      automatic: '自动检查',
      automaticOn: '开启',
      automaticOff: '关闭',
      channel: '更新通道',
      channelStable: '稳定版',
      channelBeta: '测试版',
    },
    license: '本软件是自由软件，依 GNU 通用公共许可证第 3 版或更新版本发布。',
    licenseFork: '修改版不得使用 Fern 的名称、字标与图标。',
    notOfficial:
      'Fern 不是 Minecraft 的官方产品，与 Mojang Studios 及 Microsoft 无关。',
    author: 'Astral Studio',
  },

  // 实例详情的「快照」一页。列表回答「什么时候、为什么在这里、里面有什么」，
  // 决定都发生在浮层里。
  snapshots: {
    head: '快照',
    count: '{count} 张',
    take: '拍一张',
    taking: '正在拍摄',
    taken: '已拍下快照',
    takenFiles: '{count} 个文件',
    loading: '读取快照',
    noteRunning: '游戏运行时存档正在写入，此时拍下的内容不完整。请先退出游戏。',
    noteAuto: '改动模组前和游戏结束后会自动拍下，相同的文件只存一份。',
    noteRetention: '旧快照按近密远疏自动清理，起了名字的和手动拍下的永久保留。',
    emptyLead: '还没有快照。',
    emptyDetail: '第一张会在下次改动模组或结束游戏时自动拍下，也可以现在手动拍一张。',
    worlds: '{count} 个世界',
    mods: '{count} 个模组',
    files: '{count} 个文件',
  },

  // 导出实例的弹窗。两种格式回答两个问题：整合包给别人，搬迁包给自己的
  // 另一台机器。带什么由内容清单说了算，空分区不出现。
  export: {
    dialog: '导出实例',
    title: '导出「{name}」',
    formatAria: '导出格式',
    mrpack: '整合包',
    fernpack: '搬迁包',
    mrpackTitle: 'Modrinth 整合包（.mrpack）',
    mrpackAbout: 'Prism、HMCL、PCL 等启动器都能导入。整合包不含存档。',
    mrpackMods:
      '模组只记下载地址，包内不含 jar 文件。地址要联网按文件哈希从 Modrinth 查得，查不到的模组会直接打进包里。',
    fernpackTitle: 'Fern 搬迁包（.fernpack）',
    fernpackAbout: '包含模组文件本身，换机器时用。只有 Fern 能打开。',
    carry: '带上哪些内容',
    world: '世界「{name}」',
    mods: '模组文件（{count} 个）',
    modsOff: '不含 jar 的包在另一台机器上需要重新下载模组。',
    config: '配置（{count} 个文件）',
    configHint: '配置里可能有服务器地址和坐标点，分享给别人前想一想。',
    resourcepacks: '资源包（{count} 个文件）',
    shaderpacks: '光影包（{count} 个文件）',
    schematics: '投影原理图（{count} 个文件）',
    screenshots: '截图（{count} 张）',
    cancel: '取消',
    run: '选择位置并导出',
    running: '正在导出',
    done: '已导出',
    doneDetail: '{count} 个文件 · {size}',
    doneLinked: '，其中 {count} 个模组以下载地址记录',
  },

  // 单张快照的浮层：命名、恢复、删除。
  snapshot: {
    dialog: '快照',
    rename: '命名',
    renameHint: '起了名字的快照会永久保留，不被自动清理。',
    renamePlaceholder: '例如：装 Create 之前',
    nameAria: '快照名称',
    renameSave: '保存名称',
    cancel: '取消',
    inconsistent: '拍摄时文件仍在变动，这张快照的内容可能不一致。',
    diffSame: '拍摄以来没有变化。',
    diffLead: '拍摄之后：{parts}。',
    diffModsAdded: '新装了 {count} 个模组',
    diffModsRemoved: '移除了 {count} 个模组',
    diffSavesAdded: '新建了世界 {names}',
    diffSavesRemoved: '删除了世界 {names}',
    diffSavesChanged: '世界 {names} 有改动',
    diffConfigChanged: '{count} 个配置文件有改动',
    contentWorlds: '世界（{count}）',
    contentMods: '模组（{count}）',
    contentModsLoading: '读取名单',
    enterRestore: '恢复…',
    back: '返回',
    scope: '恢复哪一部分',
    scopeAll: '整个实例',
    scopeSave: '一个世界',
    scopeConfig: '配置',
    scopeMods: '模组',
    world: '世界',
    mode: '写回方式',
    modeReplace: '覆盖原世界',
    modeCopy: '另存为新世界',
    copyName: '新世界的名称',
    consequenceCopy: '会新建一个名为「{name}」的世界，原来的世界不受影响。',
    consequenceSave: '会把「{save}」还原到这一刻，之后新生成的区块和数据会被删除。',
    consequenceConfig: '会还原 config 目录和游戏目录下的设置文件，存档与模组不受影响。',
    consequenceMods: '会把模组还原成这一刻的 {count} 个，之后新装的会被删除。',
    consequenceModsBase: '会把模组还原成这一刻的 {count} 个。',
    consequenceModsDrop: '将删除此后新装的 {count} 个：{names}。',
    consequenceModsReturn: '将带回此后移除的 {count} 个：{names}。',
    consequenceSaveReturn: '「{save}」现已不存在，恢复会把它带回来。',
    consequenceSaveSame: '「{save}」自拍摄以来没有变化，恢复它不会造成损失。',
    consequenceAll: '会还原存档、配置和模组，之后新增的文件会被删除。',
    consequenceAllDrop: '此后新建的世界也会被删除：{names}。',
    safety: '恢复前会自动拍一张，可以用它撤销这次恢复。',
    skipped: '{count} 项未纳入快照',
    missingLead: '以下文件的内容已无法从快照中读出，未被写回，原文件保持不变：',
    missingDone: '除上列文件外，恢复已完成。',
    close: '关闭',
    running: '游戏正在运行，写回的文件会被覆盖。请先退出游戏。',
    deleteWarn: '删除后这张快照无法找回。',
    delete: '删除',
    deleteConfirm: '确认删除',
    restore: '恢复',
    restoring: '正在恢复',
    restored: '已恢复',
    restoredWritten: '写回 {count} 个文件',
    restoredRemoved: '，删除 {count} 个',
  },
}

export const zhCN = { loader, capability, backend, ui }
