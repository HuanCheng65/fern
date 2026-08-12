/**
 * 网络与行为偏好。
 *
 * 和外观分开：主题码是拿来分享的，这些不该跟着一起发出去。这里放的都是
 * 「向导问过一次、之后在设置里能改」的东西，落在同一个 settings.json 的
 * 不同段里（见 lib/persist.ts）。
 *
 * 身份不在这里——它有自己的名册（lib/accounts.svelte.ts）。之前账户类型和
 * 玩家名躺在这份偏好里，那正是「只能有一个身份」的根源：偏好天然是单值的。
 *
 * 下载源没有「自动」这一档：文件里写的就是实际会发生的事。区域推荐是向导
 * 第一次替用户按下的那一下，不是一个每次启动都要重新解析的状态。
 */

import {
  emptyDownload,
  emptyGameDefaults,
  patch,
  snapshot,
  type DownloadDoc,
  type GameDefaults,
} from './persist'
import { looksLikeChina } from './region'

export type DownloadSource = 'official' | 'bmclapi'

/**
 * 按系统区域给下载源一个建议。
 *
 * 这不是测速——真正的测速要在 Rust 侧发请求，webview 的 CSP 不允许。时区和
 * 语言是手上真有的信号，用它推荐并且在界面上说清楚依据，比编一个「延迟
 * 23ms」诚实。
 */
export function suggestedSource(): DownloadSource {
  return looksLikeChina() ? 'bmclapi' : 'official'
}

class PrefsStore {
  /**
   * 下载的那一段整个存下来。
   *
   * 之前这里只留了一个 `downloadSource`，写回时是 `doc.download = { source }`——
   * 那一段里此后多出来的每一项都会被这一句抹掉。
   */
  download = $state<DownloadDoc>(emptyDownload())
  /**
   * 所有实例的起点。
   *
   * 这一层的存在理由：没有它，每建一个实例都要把同样的选择再做一遍，而人
   * 只会做一次，之后的实例全都带着一份自己没选过的默认值。
   */
  game = $state<GameDefaults>(emptyGameDefaults())
  setupDone = $state(false)
  minimizeOnLaunch = $state(false)

  /** 当前下载源。写起来到处都要，单独给一个。 */
  get downloadSource(): DownloadSource {
    return this.download.source === 'bmclapi' ? 'bmclapi' : 'official'
  }

  /** 从磁盘读到的设置装进来。App 启动时调一次。 */
  hydrate() {
    const doc = snapshot()
    this.download = { ...emptyDownload(), ...(doc.download ?? {}) }
    this.game = { ...emptyGameDefaults(), ...(doc.game ?? {}) }
    this.setupDone = doc.setupDone === true
    this.minimizeOnLaunch = doc.minimizeOnLaunch === true
  }


  /** 改一项全局默认。整份写回去，Rust 那边的 serde 只认完整的一段。 */
  setGame(change: Partial<GameDefaults>) {
    this.game = { ...this.game, ...change }
    const game = this.game
    patch((doc) => (doc.game = game))
  }

  /** 改一项下载设置。整段写回去，Rust 那边的 serde 只认完整的一段。 */
  setDownload(change: Partial<DownloadDoc>) {
    this.download = { ...this.download, ...change }
    const download = this.download
    patch((doc) => (doc.download = download))
  }

  setDownloadSource(source: DownloadSource) {
    this.setDownload({ source })
  }

  setMinimizeOnLaunch(minimize: boolean) {
    this.minimizeOnLaunch = minimize
    patch((doc) => (doc.minimizeOnLaunch = minimize))
  }

  finishSetup() {
    this.setupDone = true
    patch((doc) => (doc.setupDone = true))
  }
}

export const prefs = new PrefsStore()
