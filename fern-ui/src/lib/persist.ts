/**
 * 设置的存取。
 *
 * 真正的存储在 Rust 侧：数据目录下的 `settings.json`（见 fern-core/src/settings.rs）。
 * 这里只是它在界面里的一份镜像——启动时读一次，之后每次改动写回去。
 *
 * localStorage 只在浏览器预览（`pnpm dev` 不带 Tauri）时兜底。它是浏览器的
 * 缓存，不是用户的配置：清一次数据就没了，也不在任何人能打开、能备份、能
 * 贴给别人的地方。
 *
 * 写盘防抖：拖一下颜色选择器会连着触发几十次改动，每次都落盘没有意义。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri } from './instances.svelte'

export interface SettingsDoc {
  /** 外观。Rust 不解释里面有什么，界面自己说了算。 */
  appearance: Record<string, unknown>
  account: { kind: string; playerName: string }
  download: DownloadDoc
  /** 所有实例的起点。实例设置只写它要偏离的那几项。 */
  game: GameDefaults
  update: UpdateDoc
  setupDone: boolean
  /** 游戏窗口开出来之后把启动器收起来。 */
  minimizeOnLaunch: boolean
}

export interface DownloadDoc {
  source: string
  /** 同时下载几个文件。null 用内置默认值。 */
  concurrency: number | null
  /** 每秒最多下载多少 KB。null 表示不限速。 */
  rateLimitKbps: number | null
  proxy: 'system' | 'direct' | 'custom'
  /** proxy 为 custom 时使用的地址。 */
  proxyUrl: string
}

export const emptyDownload = (): DownloadDoc => ({
  source: 'official',
  concurrency: null,
  rateLimitKbps: null,
  proxy: 'system',
  proxyUrl: '',
})

export interface UpdateDoc {
  channel: 'stable' | 'beta'
  /** 自动检查更新。关掉之后一个请求都不发。 */
  automatic: boolean
  /**
   * 灰度分桶，0–99。
   *
   * **界面不生成也不修改它**——后端第一次检查更新时抽一次就固定下来。放在这里
   * 只是因为它和其余设置一样该是用户看得见的（它从不上传）。改这一段设置时
   * 记得合并而不是整段覆盖，否则每改一次通道就换一次分桶。
   */
  bucket: number | null
}

export interface GameDefaults {
  /** 交给游戏的内存上限，MB。null 是「物理内存的一半」。 */
  memoryCeilingMb: number | null
  garbageCollector: 'auto' | 'g1' | 'z' | null
  resolution: { width: number; height: number } | null
  /** 额外 JVM 参数，原样一行。 */
  jvmArguments: string
}

export const emptyGameDefaults = (): GameDefaults => ({
  memoryCeilingMb: null,
  garbageCollector: null,
  resolution: null,
  jvmArguments: '',
})

const FALLBACK_KEY = 'fern.settings'
const SAVE_DELAY = 300

export const emptyDoc = (): SettingsDoc => ({
  appearance: {},
  account: { kind: 'offline', playerName: '' },
  download: emptyDownload(),
  game: emptyGameDefaults(),
  update: { channel: 'stable', automatic: true, bucket: null },
  setupDone: false,
  minimizeOnLaunch: false,
})

let doc: SettingsDoc = emptyDoc()
let timer: ReturnType<typeof setTimeout> | undefined

const LEGACY_KEYS = ['fern.theme', 'fern.prefs', 'fern.account.name', 'fern.landing.seen']

/**
 * 0.1.0 把设置拆在几个 localStorage 键里。
 *
 * 这是**搬走**，不是抄一份：读进来、落盘、删掉旧键。抄一份的话，旧键会一直
 * 留在 webview 里，而它触发的条件恰好是「文件里没有设置」——删掉
 * settings.json 之后设置会原样变回来，文件就不再是唯一的来源。
 *
 * 返回是否真的读到了东西：没读到就不该去动磁盘，也不该删任何键。
 */
function migrateLegacy(target: SettingsDoc): boolean {
  let found = false
  try {
    const appearance = localStorage.getItem('fern.theme')
    if (appearance) {
      target.appearance = JSON.parse(appearance) as Record<string, unknown>
      found = true
    }
    const prefs = localStorage.getItem('fern.prefs')
    if (prefs) {
      const parsed = JSON.parse(prefs) as Record<string, unknown>
      if (typeof parsed.playerName === 'string') target.account.playerName = parsed.playerName
      if (typeof parsed.accountKind === 'string') target.account.kind = parsed.accountKind
      if (parsed.downloadSource === 'bmclapi' || parsed.downloadSource === 'official') {
        target.download.source = parsed.downloadSource
      }
      target.setupDone = parsed.setupDone === true
      target.minimizeOnLaunch = parsed.minimizeOnLaunch === true
      found = true
    }
    const name = localStorage.getItem('fern.account.name')
    if (name && !target.account.playerName) {
      target.account.playerName = name
      target.setupDone = localStorage.getItem('fern.landing.seen') === '1'
      found = true
    }
  } catch {
    // 旧数据读不出来就当没有，用默认值继续。
  }
  return found
}

/** 落盘成功之后才调用——写失败时旧数据还得留着。 */
function dropLegacy() {
  try {
    for (const key of LEGACY_KEYS) localStorage.removeItem(key)
  } catch {
    // 存储被禁用时旧键本来也读不出来，没有要清的东西。
  }
}

export async function hydrate(): Promise<SettingsDoc> {
  const next = emptyDoc()
  let migrated = false
  if (inTauri()) {
    try {
      Object.assign(next, await invoke<SettingsDoc>('get_settings'))
    } catch {
      // 读不出来（首次启动、文件损坏）就用默认值，下一次保存会把它写回去。
    }
    // 文件是空的说明这台机器还没迁过来，把旧的浏览器存储搬进去。
    if (!next.setupDone && !next.account.playerName) migrated = migrateLegacy(next)
  } else {
    try {
      const raw = localStorage.getItem(FALLBACK_KEY)
      if (raw) Object.assign(next, JSON.parse(raw) as SettingsDoc)
      else migrated = migrateLegacy(next)
    } catch {
      // 同上。
    }
  }
  doc = next
  // 立刻落盘再清旧键：搬迁要在这一次启动里就完成，不能等用户碰一下设置。
  if (migrated && (await flush())) dropLegacy()
  return doc
}

export function snapshot(): SettingsDoc {
  return doc
}

/** 改一段设置并排队写盘。 */
export function patch(mutate: (target: SettingsDoc) => void) {
  mutate(doc)
  clearTimeout(timer)
  timer = setTimeout(() => void flush(), SAVE_DELAY)
}

/** 写盘。返回是否写成功——搬迁要靠它决定能不能删旧数据。 */
export async function flush(): Promise<boolean> {
  clearTimeout(timer)
  const settings = doc
  if (!inTauri()) {
    try {
      localStorage.setItem(FALLBACK_KEY, JSON.stringify(settings))
      return true
    } catch {
      // 无痕模式：这次生效，下次打开回到默认。
      return false
    }
  }
  try {
    await invoke('save_settings', { settings })
    return true
  } catch {
    // 磁盘写不进去（只读、满盘）时界面已经是新的样子了，下次改动会再试一次。
    return false
  }
}
