/**
 * 自更新在界面这一侧。
 *
 * 现在只回答「有没有」，动作是打开下载页——下载和替换是下一步（见
 * docs/fern-update-design.md §9）。这一层的形状是照着那之后不用改写的样子定的：
 * 后端返回的是一个**判断结果**而不是一个 `Update | null`，所以「已是最新」
 * 「还没轮到你」「这个平台没有构建」在界面上是三句不同的话，而不是同一句
 * 「没有更新」加三种猜测。
 *
 * 两条纪律，都写在这里，因为它们最容易在改界面时被无意中破坏：
 *
 * - **检查失败不说话。** 端点挂了、DNS 被污染、清单是坏的——一个因为更新
 *   服务器挂了而弹错误框的启动器，比一个不会自更新的启动器差。只有用户
 *   自己点了「检查更新」时才把失败显示出来，因为那时候沉默才是错的。
 * - **`held_back` 什么都不显示。** 「有更新但不给你」是最招人烦的一种提示，
 *   而灰度对用户来说本来就该是隐形的。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri } from './instances.svelte'
import { patch, snapshot } from './persist'

/** 和 fern-core 的 `update::Decision` 一一对应。类型标签 snake_case，字段 camelCase。 */
export type UpdateDecision =
  | { kind: 'up_to_date' }
  | { kind: 'available'; version: string; notes: string | null; critical: boolean }
  | { kind: 'held_back'; version: string }
  | { kind: 'ahead_of_channel'; version: string }
  | { kind: 'needs_full_download'; version: string; minVersion: string }
  | { kind: 'no_build'; target: string }

export type UpdateChannel = 'stable' | 'beta'

/**
 * 启动之后隔多久检查第一次。
 *
 * 启动的那一刻网络要留给补全和元数据——用户等的是游戏，不是更新提示。
 */
const FIRST_CHECK_DELAY = 30_000

/** 之后多久检查一次。清单本身在 CDN 上只缓存 60 秒，所以这个间隔不必更密。 */
const CHECK_INTERVAL = 6 * 60 * 60 * 1000

class UpdateStore {
  channel = $state<UpdateChannel>('stable')
  automatic = $state(true)

  /** 最近一次检查的结论。`undefined` 是「还没查过」。 */
  decision = $state<UpdateDecision | undefined>(undefined)
  checking = $state(false)
  /**
   * 最近一次检查失败了。
   *
   * 只有用户自己点的那一次会把它显示出来（见文件头）。自动检查失败时它也会
   * 被设上，但没有人读它——留着是为了让「点一下检查」能立刻说出实情。
   */
  failed = $state(false)

  /** 该不该在设置入口上点一个不打扰的标记。 */
  get available(): boolean {
    return this.decision?.kind === 'available'
  }

  hydrate() {
    const update = snapshot().update
    this.channel = update?.channel === 'beta' ? 'beta' : 'stable'
    this.automatic = update?.automatic !== false
  }

  setChannel(channel: UpdateChannel) {
    this.channel = channel
    // 只改这一个字段：`bucket` 是后端抽的，整段覆盖会把它抹掉，
    // 而那意味着这台机器每改一次通道就换一次灰度分桶。
    patch((doc) => (doc.update = { ...doc.update, channel }))
    // 通道变了，上一次的结论就作废了——它是另一条通道上的答案。
    this.decision = undefined
    void this.check()
  }

  setAutomatic(automatic: boolean) {
    this.automatic = automatic
    patch((doc) => (doc.update = { ...doc.update, automatic }))
  }

  /** 查一次。失败只记在 `failed` 上，由调用处决定说不说。 */
  async check(): Promise<void> {
    if (!inTauri() || this.checking) return
    this.checking = true
    try {
      this.decision = await invoke<UpdateDecision>('check_update')
      this.failed = false
    } catch {
      this.failed = true
    } finally {
      this.checking = false
    }
  }

  /**
   * 定时检查。App 挂载时调一次。
   *
   * `automatic` 是关的就一个请求都不发——那个开关的意思是「别联网」，
   * 不是「联网了但不告诉我」。
   */
  watch(): () => void {
    if (!inTauri()) return () => {}
    const tick = () => {
      if (this.automatic) void this.check()
    }
    const first = setTimeout(tick, FIRST_CHECK_DELAY)
    const timer = setInterval(tick, CHECK_INTERVAL)
    return () => {
      clearTimeout(first)
      clearInterval(timer)
    }
  }
}

export const updates = new UpdateStore()
