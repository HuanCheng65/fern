/**
 * 设置的目录。
 *
 * 这里是每一项设置的**标题、说明和位置**的唯一出处：设置页从这张表渲染那一
 * 行的标签，命令面板从同一张表把它变成一个可以被搜到的位置。
 *
 * 之所以不是「一张给面板用的清单加一份写在标记里的正文」——那需要每加一项
 * 设置就在两个地方各写一遍同样的话，而两份说明迟早会说得不一样。让标记不再
 * 持有这份信息，也就没有副本可以漂移。
 *
 * 加一项设置：这里加一条，标记里 `<SettingRow id="…">` 用一次那个 id。id
 * 写错会当场报错，不会变成一条搜不到却没人发现的设置。
 *
 * 自动发现走不通：面板要在设置页没打开的时候就搜得到，而 DOM 里的东西只有
 * 挂载了才存在，恰恰是不需要搜的时候才有。
 */

import { nav } from './nav.svelte'
import { provides, type Subject } from 'fern-kit/parts/palette'

export interface SettingsSection {
  id: string
  label: string
}

export interface SettingsRow {
  /** `分区/行`。既是地址，也是标记里那个 `data-setting`。 */
  id: string
  label: string
  /** 说明。只在没有它就会用错的地方写。 */
  note?: string
  /**
   * 界面上没写、但人会拿来搜的词。
   *
   * 「GC」「RAM」这类：标题里写的是中文全称，而记得它的人多半用缩写去找。
   */
  keywords?: string
  /** 标题在上、控件在下。控件比一行放得下的宽时用。 */
  stack?: boolean
}

/**
 * 分区按「你为了什么来这一页」切，不按功能模块切（见 docs/frond-design-system.md）。
 */
export const SETTINGS_SECTIONS: SettingsSection[] = [
  { id: 'appearance', label: '外观' },
  { id: 'account', label: '账户' },
  { id: 'game', label: '游戏' },
  { id: 'snapshots', label: '快照' },
  { id: 'java', label: 'Java' },
  { id: 'download', label: '下载' },
  { id: 'data', label: '数据' },
  { id: 'about', label: '关于' },
]

export const SETTINGS_ROWS: SettingsRow[] = [
  { id: 'appearance/accent', label: '强调色', keywords: 'accent color 颜色 主题色' },
  { id: 'appearance/swatch', label: '颜色', keywords: 'color 色板' },
  { id: 'appearance/density', label: '界面密度', keywords: 'density 紧凑 宽松' },
  { id: 'appearance/radius', label: '圆角', keywords: 'radius corner 直角 圆润' },
  {
    id: 'appearance/motion',
    label: '动效',
    note: '关闭后同时停用背景粒子与指针视差。窗口失焦时始终暂停。',
    keywords: 'motion animation 动画 粒子',
  },
  {
    id: 'appearance/code',
    label: '主题码',
    note: '包含以上全部外观选择。他人粘贴后点击应用即可复现。',
    keywords: 'theme code 分享 导入 导出',
    stack: true,
  },
  { id: 'appearance/reset', label: '恢复默认外观', keywords: 'reset default' },

  {
    id: 'account/list',
    label: '账户',
    note: '可保存多个身份，点击名称切换。令牌存储于系统钥匙串，不写入任何文件。',
    keywords: 'account login 登录 微软 离线 外置',
    stack: true,
  },

  {
    id: 'game/memory',
    label: '游戏内存上限',
    keywords: 'memory ram xmx heap 堆 内存',
    stack: true,
  },
  {
    id: 'game/gc',
    label: '垃圾回收器',
    note: '自动会按 Java 版本挑：21 以上给分代 ZGC，更老的给 G1。实例可单独覆盖。',
    keywords: 'gc g1 zgc garbage collector 自动',
  },
  {
    id: 'game/window',
    label: '游戏窗口',
    note: '未指定时沿用游戏自身记录的尺寸。',
    keywords: 'window resolution 分辨率 尺寸 全屏',
  },
  {
    id: 'game/jvm',
    label: '额外 JVM 参数',
    note: '置于 Fern 内置参数之后，同名参数以此处为准。以空格分隔，不解析引号。',
    keywords: 'jvm arguments flags 参数',
    stack: true,
  },
  {
    id: 'game/minimize',
    label: '启动后最小化',
    note: '在游戏窗口出现后最小化 Fern，而非点击启动时。',
    keywords: 'minimize 最小化 隐藏',
  },

  {
    id: 'snapshots/automatic',
    label: '自动拍摄',
    note: '在改动模组前、启动前与游戏结束后自动拍摄。关闭后仅保留手动拍摄，改动模组导致存档损坏时将无法回滚。',
    keywords: 'snapshot backup automatic 快照 备份 自动 回滚',
  },
  {
    id: 'snapshots/limit',
    label: '占用上限',
    note: '快照总占用超过此值时，从最旧的自动快照开始删除。手动拍摄与已加标签的快照不会被删除。留空表示不限。',
    keywords: 'snapshot backup limit size disk 快照 备份 上限 占用 磁盘 清理',
    stack: true,
  },
  {
    id: 'java/runtimes',
    label: '运行时',
    note: '缺失的版本会在首次启动相应实例时自动下载，也可在此提前安装。',
    keywords: 'java jdk jre runtime 运行时 删除 占用',
    stack: true,
  },
  {
    id: 'java/add',
    label: '手动添加',
    note: '扫描不到的安装位置。填写 JDK/JRE 根目录或其中的 java 可执行文件。',
    keywords: 'java path 路径 添加',
    stack: true,
  },
  { id: 'java/rescan', label: '重新扫描', keywords: 'rescan refresh 刷新' },

  { id: 'download/source', label: '下载源', keywords: 'download source mirror bmclapi 镜像 官方源' },
  {
    id: 'download/concurrency',
    label: '同时下载数',
    note: '同时下载的文件数量上限。网络设备承受不住大量连接时可调低。',
    keywords: 'concurrency parallel connections 并发 线程 连接数 同时',
    stack: true,
  },
  {
    id: 'download/rate-limit',
    label: '下载限速',
    note: '每秒下载量的上限。留空表示不限速。',
    keywords: 'rate limit bandwidth speed 限速 带宽 网速',
  },
  {
    id: 'download/proxy',
    label: '代理',
    note: '跟随系统时使用系统代理与 HTTP_PROXY 等环境变量。',
    keywords: 'proxy http socks 代理 翻墙',
    stack: true,
  },

  {
    id: 'data/root',
    label: '目录',
    keywords: 'data directory path migrate 数据目录 游戏目录 日志目录 路径 位置 便携 迁移 移动 搬家',
    stack: true,
  },
  {
    id: 'data/usage',
    label: '占用',
    note: '各部分占用的磁盘空间。标注可清除的会在需要时重新生成或重新下载，不影响存档、模组或任何设置。',
    keywords:
      'storage disk usage space clean slim 磁盘 占用 空间 大小 容量 清理 清除 缓存 日志 瘦身 释放 版本 依赖库 资源 运行时 快照 实例',
    stack: true,
  },
  {
    id: 'data/existing',
    label: '现有游戏目录',
    note: '把已有的 .minecraft 中的版本添加为实例。游戏文件保留在原位置。',
    keywords: 'minecraft import existing directory 导入 现有 目录 便携',
  },

  { id: 'about/version', label: '版本', keywords: 'version about 关于 build 构建' },
  {
    id: 'about/diagnostics',
    label: '诊断信息',
    note: '版本、系统与运行环境。反馈问题时请一并附上。',
    keywords: 'diagnostics report bug 诊断 反馈 报错 版本 系统',
    stack: true,
  },
  {
    id: 'about/update',
    label: '更新',
    keywords: 'update upgrade 更新 升级 新版本 检查',
    stack: true,
  },
  {
    id: 'about/automatic',
    label: '自动检查更新',
    note: '关闭后不会发出任何检查更新的请求。',
    keywords: 'automatic auto update 自动 检查 更新',
  },
  {
    id: 'about/channel',
    label: '更新通道',
    note: '测试版可更早获得新功能，稳定性低于稳定版。',
    keywords: 'channel beta stable 通道 测试版 稳定版 内测',
  },
  { id: 'about/links', label: '源码与反馈', keywords: 'github source issue 源码 仓库 反馈 开源' },
  {
    id: 'about/legal',
    label: '许可与声明',
    keywords: 'license gpl legal 许可 协议 开源 声明 mojang',
    stack: true,
  },
]

const byId = new Map(SETTINGS_ROWS.map((row) => [row.id, row]))

/**
 * 取一行的定义。找不到就抛——一个写错的 id 应该当场停下，而不是渲染出一行
 * 没有标题、而且永远搜不到的设置。
 */
export function settingsRow(id: string): SettingsRow {
  const row = byId.get(id)
  if (!row) throw new Error(`设置目录里没有 ${id}`)
  return row
}

const sectionLabel = (id: string) =>
  SETTINGS_SECTIONS.find((section) => section.id === id.split('/')[0])?.label ?? '设置'

/**
 * 分区和每一行都是可寻址的位置。
 *
 * 七节之后，能直接搜到「圆角」比记得它在「外观」里更省时间——而后者本来就
 * 是要靠记的。
 *
 * 两级并存不重复，靠的是**各自只用自己的标题去匹配**：打分区名只出分区，
 * 打设置项的名字（或它的缩写）只出那一行。行的 hint 写着它在哪一节，那是
 * 找到之后用来确认的，不参与匹配——否则一个「外观」会把那一节的七行全捞
 * 出来，而它们说的是同一件事。
 */
provides(() => [
  ...SETTINGS_SECTIONS.map(
    (section): Subject => ({
      type: 'place',
      id: `settings/${section.id}`,
      // 「外观 · 设置」而不是标题里写「设置 · 外观」：人打的是「外观」，而
      // 一个被整个打中的标题才该拿满分——多出来的两个字只会把它压到那一节
      // 里某一行的下面去。位置归位置，写在 hint 里。
      title: section.label,
      hint: '设置',
      run: () => nav.settings(section.id),
    }),
  ),
  ...SETTINGS_ROWS
    // 一节里只有一行、名字还和这一节相同（账户），那一行就是那一节，不必
    // 让同一个去处出现两次。
    .filter((row) => row.label !== sectionLabel(row.id))
    .map(
      (row): Subject => ({
        type: 'place',
        id: `settings/${row.id}`,
        title: row.label,
        // 位置面包屑，只给眼睛看。要搜的别名在 terms 里——界面上写的是
        // 「垃圾回收器」，而记得它的人多半打 gc。
        hint: `设置 · ${sectionLabel(row.id)}`,
        terms: row.keywords ?? '',
        run: () => nav.settings(row.id),
      }),
    ),
])
