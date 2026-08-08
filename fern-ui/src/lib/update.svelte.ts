/**
 * 自更新在界面这一侧。
 *
 * 后端返回的是一个**判断结果**而不是 `Update | null`，所以「已是最新」「尚未
 * 轮到这台机器」「当前平台没有构建」在界面上是三句不同的话，而不是同一句
 * 「没有更新」加三种猜测。
 *
 * 三条纪律写在这里，因为它们最容易在改界面时被无意中破坏：
 *
 * - **检查失败不出声。** 端点不可达、DNS 被污染、清单损坏——一个因为更新
 *   服务器故障而弹错误框的启动器，比一个不会自更新的启动器更糟。只有用户
 *   主动点了「检查更新」时才显示失败，那时候沉默才是错的。
 * - **安装失败必须出声。** 用户刚按下一个按钮，正在等一个回答。
 * - **`held_back` 什么都不显示。** 「有更新但不给你」是最令人烦躁的一种提示，
 *   而灰度对用户本来就该是隐形的。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { inTauri } from './instances.svelte'
import { patch, snapshot } from './persist'

/** 和 fern-core 的 `update::Decision` 一一对应。类型标签 snake_case，字段 camelCase。 */
export type UpdateDecision =
  | { kind: 'up_to_date' }
  | {
      kind: 'available'
      version: string
      notes: string | null
      critical: boolean
      /** 本平台那个包的地址，来自清单。**别在这里另拼一个。** */
      url: string
    }
  | { kind: 'held_back'; version: string }
  | { kind: 'ahead_of_channel'; version: string }
  | { kind: 'needs_full_download'; version: string; minVersion: string }
  | { kind: 'no_build'; target: string }
  | { kind: 'no_release' }

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
  /**
   * 用户选定的通道。`null` 表示未选过，此时跟随当前构建。
   *
   * 装了测试版构建却默认查稳定通道，只会一直显示「当前版本高于该通道」——
   * 那一份本来就来自测试通道。判断在后端（`update::Channel::of_version`），
   * 这里只需要知道版本号里有没有预发布段。
   */
  chosen = $state<UpdateChannel | null>(null)
  /** 当前构建的版本号，用来推导默认通道。App 启动时填。 */
  version = $state('')
  automatic = $state(true)

  /** 实际生效的通道。 */
  get channel(): UpdateChannel {
    return this.chosen ?? (this.version.includes('-') ? 'beta' : 'stable')
  }

  /** 这份构建能不能自更新。deb 装的那一份不能。 */
  selfUpdate = $state(true)

  /** 更新已经装好，等一次重启。 */
  installed = $state(false)
  applying = $state(false)
  /** 下载进度，0–1。总长度未知时是 `undefined`。 */
  progress = $state<number | undefined>(undefined)
  /** 安装失败的原因。这个必须显示——用户刚刚按了一个按钮。 */
  error = $state('')

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

  hydrate(build: { version: string; selfUpdate: boolean }) {
    const update = snapshot().update
    this.chosen = update?.channel === 'beta' || update?.channel === 'stable' ? update.channel : null
    this.automatic = update?.automatic !== false
    this.version = build.version
    this.selfUpdate = build.selfUpdate
  }

  setChannel(channel: UpdateChannel) {
    this.chosen = channel
    // 只改这一个字段：`bucket` 由后端生成，整段覆盖会把它抹掉，
    // 那意味着每改一次通道就换一次灰度分桶。
    patch((doc) => (doc.update = { ...doc.update, channel }))
    // 通道变了，上一次的结论就作废了——那是另一条通道上的答案。
    this.decision = undefined
    this.installed = false
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
   * 下载并安装。装完不重启——什么时候重启由用户决定。
   *
   * 失败必须显示：用户刚刚按了一个按钮，沉默在这里是错的。这和自动检查
   * 失败时的沉默不矛盾，两者的区别正是「有没有人在等一个回答」。
   */
  async apply(): Promise<void> {
    if (!inTauri() || this.applying) return
    this.applying = true
    this.error = ''
    this.progress = undefined
    const stop = await listen<{ downloaded: number; total: number | null }>(
      'update_progress',
      (event) => {
        const { downloaded, total } = event.payload
        this.progress = total ? Math.min(1, downloaded / total) : undefined
      },
    )
    try {
      await invoke('update_apply')
      this.installed = true
    } catch (error) {
      this.error = String(error)
    } finally {
      stop()
      this.applying = false
      this.progress = undefined
    }
  }

  restart(): void {
    void invoke('update_restart')
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
