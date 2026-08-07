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
  download: { source: string }
  setupDone: boolean
  /** 游戏窗口开出来之后把启动器收起来。 */
  minimizeOnLaunch: boolean
}

const FALLBACK_KEY = 'fern.settings'
const SAVE_DELAY = 300

export const emptyDoc = (): SettingsDoc => ({
  appearance: {},
  account: { kind: 'offline', playerName: '' },
  download: { source: 'official' },
  setupDone: false,
  minimizeOnLaunch: false,
})

let doc: SettingsDoc = emptyDoc()
let timer: ReturnType<typeof setTimeout> | undefined

/** 0.1.0 把设置拆在几个 localStorage 键里。读一次搬过来，之后就只认文件。 */
function migrateLegacy(target: SettingsDoc) {
  try {
    const appearance = localStorage.getItem('fern.theme')
    if (appearance) target.appearance = JSON.parse(appearance) as Record<string, unknown>
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
    }
    const name = localStorage.getItem('fern.account.name')
    if (name && !target.account.playerName) {
      target.account.playerName = name
      target.setupDone = localStorage.getItem('fern.landing.seen') === '1'
    }
  } catch {
    // 旧数据读不出来就当没有，用默认值继续。
  }
}

export async function hydrate(): Promise<SettingsDoc> {
  const next = emptyDoc()
  if (inTauri()) {
    try {
      Object.assign(next, await invoke<SettingsDoc>('get_settings'))
    } catch {
      // 读不出来（首次启动、文件损坏）就用默认值，下一次保存会把它写回去。
    }
    // 文件是空的说明这台机器还没迁过来，把旧的浏览器存储搬进去。
    if (!next.setupDone && !next.account.playerName) migrateLegacy(next)
  } else {
    try {
      const raw = localStorage.getItem(FALLBACK_KEY)
      if (raw) Object.assign(next, JSON.parse(raw) as SettingsDoc)
      else migrateLegacy(next)
    } catch {
      // 同上。
    }
  }
  doc = next
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

export async function flush() {
  clearTimeout(timer)
  const settings = doc
  if (!inTauri()) {
    try {
      localStorage.setItem(FALLBACK_KEY, JSON.stringify(settings))
    } catch {
      // 无痕模式：这次生效，下次打开回到默认。
    }
    return
  }
  try {
    await invoke('save_settings', { settings })
  } catch {
    // 磁盘写不进去（只读、满盘）时界面已经是新的样子了，下次改动会再试一次。
  }
}
