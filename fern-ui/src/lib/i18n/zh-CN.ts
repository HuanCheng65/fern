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
  'crash.fabric-missing-dependency': {
    title: '缺少前置模组：{need}',
    detail: '当前实例中没有安装 {need}。',
  },
  'crash.fabric-incompatible-mods': {
    title: '模组之间不兼容',
    detail:
      'Fabric 拒绝加载当前的模组组合。日志中「Incompatible mods found」之后的几行列出了具体是哪些。',
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
  'preflight.disabled-dependency': {
    title: '{dependency} 已被禁用',
    detail: '{mod} 需要它。在模组列表中重新启用即可。',
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
  'snapshot.skipped.too-large': {
    title: '文件过大',
    detail: '单个文件超过 512 MB，未纳入快照。',
  },
  'snapshot.skipped.transient': {
    title: '可重新生成',
    detail: '日志、崩溃报告和缓存不纳入快照，它们会自行生成。',
  },
  'snapshot.skipped.not-selected': {
    title: '不在备份范围内',
    detail: '快照只包含存档、配置、模组、资源包和截图，这一项不在其中。',
  },
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
    // 「构建」而不是「版本」：版本号相同的两次构建是常事，哈希才对得上。
    build: '构建',
    unknownBuild: '未知构建',
    diagnostics: '诊断信息',
    diagnosticsNote: '反馈问题时请附上这段内容，可以省去几轮来回确认。',
    copy: '复制',
    copied: '已复制',
    repository: '源码仓库',
    issues: '反馈问题',
    license: '本软件为自由软件，依 GNU 通用公共许可证第 3 版或更新版本发布。',
    licenseFork: '修改版不得沿用 Fern 的名称、字标与图标。',
    notOfficial:
      'Fern 不是 Minecraft 的官方产品，与 Mojang Studios 及 Microsoft 无关。',
    author: 'Astral Studio',
  },
}

export const zhCN = { loader, backend, ui }
