/**
 * 身份与网络偏好。
 *
 * 和外观分开：主题码是拿来分享的，用户名和账户类型不该跟着一起发出去。
 * 这里放的都是「向导问过一次、之后在设置里能改」的东西，落在同一个
 * settings.json 的不同段里（见 lib/persist.ts）。
 *
 * 下载源没有「自动」这一档：文件里写的就是实际会发生的事。区域推荐是向导
 * 第一次替用户按下的那一下，不是一个每次启动都要重新解析的状态。
 */

import { patch, snapshot } from './persist'

export type AccountKind = 'offline' | 'microsoft' | 'authlib'
export type DownloadSource = 'official' | 'bmclapi'

/**
 * 按系统区域给下载源一个建议。
 *
 * 这不是测速——真正的测速要在 Rust 侧发请求，webview 的 CSP 不允许。时区和
 * 语言是手上真有的信号，用它推荐并且在界面上说清楚依据，比编一个「延迟
 * 23ms」诚实。
 */
export function suggestedSource(): DownloadSource {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? ''
    const cn = /Shanghai|Chongqing|Harbin|Urumqi|Macau/i.test(zone)
    return cn || navigator.language.toLowerCase().startsWith('zh-cn') ? 'bmclapi' : 'official'
  } catch {
    return 'official'
  }
}

class PrefsStore {
  accountKind = $state<AccountKind>('offline')
  playerName = $state('')
  downloadSource = $state<DownloadSource>('official')
  setupDone = $state(false)

  /** 从磁盘读到的设置装进来。App 启动时调一次。 */
  hydrate() {
    const doc = snapshot()
    const kind = doc.account.kind
    this.accountKind =
      kind === 'microsoft' || kind === 'authlib' || kind === 'offline' ? kind : 'offline'
    this.playerName = typeof doc.account.playerName === 'string' ? doc.account.playerName : ''
    this.downloadSource = doc.download.source === 'bmclapi' ? 'bmclapi' : 'official'
    this.setupDone = doc.setupDone === true
  }

  setAccount(kind: AccountKind, playerName: string) {
    this.accountKind = kind
    this.playerName = playerName
    patch((doc) => (doc.account = { kind, playerName }))
  }

  setPlayerName(playerName: string) {
    this.setAccount(this.accountKind, playerName)
  }

  setDownloadSource(source: DownloadSource) {
    this.downloadSource = source
    patch((doc) => (doc.download = { source }))
  }

  finishSetup() {
    this.setupDone = true
    patch((doc) => (doc.setupDone = true))
  }
}

export const prefs = new PrefsStore()
