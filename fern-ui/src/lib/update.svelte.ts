/**
 * 自更新在界面这一侧。
 *
 * 后端返回的是一个**判断结果**而不是 `Update | null`，所以「已是最新」「尚未
 * 轮到这台机器」「当前平台没有构建」在界面上是三句不同的话，而不是同一句
 * 「没有更新」加三种猜测。
 *
 * 四条纪律写在这里，因为它们最容易在改界面时被无意中破坏：
 *
 * - **检查失败不出声。** 端点不可达、DNS 被污染、清单损坏——一个因为更新
 *   服务器故障而弹错误框的启动器，比一个不会自更新的启动器更糟。只有用户
 *   主动点了「检查更新」时才显示失败，那时候沉默才是错的。
 * - **用户按下按钮之后的失败必须出声。** 他正在等一个回答。后台自己下的那一次
 *   不算，它和检查失败一样沉默。
 * - **`held_back` 什么都不显示。** 「有更新但不给你」是最令人烦躁的一种提示，
 *   而灰度对用户本来就该是隐形的。
 * - **自动下载装完也不重启。** 「自动更新」的意思是「装好了等你下次启动」，
 *   不是「替你决定什么时候关掉游戏」。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { format, ui } from './i18n'
import { inTauri } from './instances.svelte'
import { launch } from './launch.svelte'
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

/** 多久检查一次。清单本身在 CDN 上只缓存 60 秒，所以这个间隔不必更密。 */
const CHECK_INTERVAL = 6 * 60 * 60 * 1000

/**
 * 检查失败之后隔多久再试，以及退避的上限。
 *
 * 没有这一条的话，开机那一次赶上网卡还没连上（很常见），下一次要等六小时——
 * 而那六小时里界面上什么都不会说，因为检查失败本来就是沉默的。
 */
const RETRY_DELAY = 2 * 60 * 1000
const RETRY_MAX = 30 * 60 * 1000

/**
 * 后端发回来的文案 id 对应的句子。句子归界面管，后端只发 id。
 *
 * 插件自己的错误没有 id（那是它的 Rust 错误链），走 `failed` 那一条，
 * 原文当细节显示——看不懂总比看不见强。
 */
const ERRORS: Record<string, string> = {
  'update.system-package': ui.about.update.managed,
  'update.not-writable': ui.about.update.errors.notWritable,
  'update.bad-endpoint': ui.about.update.errors.badEndpoint,
  'update.nothing-to-install': ui.about.update.errors.nothingToInstall,
}

const describeError = (raw: string): string =>
  ERRORS[raw] ?? format(ui.about.update.errors.failed, { detail: raw })

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
    void this.check().then(() => this.fetchQuietly())
  }

  setAutomatic(automatic: boolean) {
    this.automatic = automatic
    patch((doc) => (doc.update = { ...doc.update, automatic }))
    // 刚打开就该有反应。定时那一轮最长在六小时之后，而一个「开了却整天没动静」
    // 的开关，用户只会认为它没生效。
    if (automatic) void this.check().then(() => this.fetchQuietly())
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
   * 失败时的沉默不矛盾，两者的区别正是「有没有人在等一个回答」——所以
   * 后台自己下的那一次（`silent`）失败了也不出声，下一轮检查会再来一遍。
   */
  async apply({ silent = false }: { silent?: boolean } = {}): Promise<void> {
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
      if (!silent) this.error = describeError(String(error))
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
   * 查到了就自己下好，不打扰任何人（设计文档 §6）。
   *
   * 「自动更新」这个开关的意思是**自动装好等一次重启**，不是「自动告诉你有更新」。
   * 装完仍然不重启，界面上仍然只有设置键上那一个点。
   */
  private async fetchQuietly(): Promise<void> {
    if (!this.automatic || !this.selfUpdate) return
    if (this.installed || this.applying) return
    if (this.decision?.kind !== 'available') return
    // 有游戏在跑的时候不下：带宽和磁盘这时候都属于游戏，而更新一分钟都不急。
    // 下一次检查会再来，或者用户自己在设置里按。
    if (launch.running) return
    await this.apply({ silent: true })
  }

  /**
   * 定时检查。**要在 `hydrate` 之后调**：`automatic` 是关的就一个请求都不发，
   * 而那个开关的取值在读盘之前是不知道的——那个开关的意思是「别联网」，
   * 不是「联网了但不告诉我」。
   *
   * 第一次是立刻查，不再等三十秒：一份几 KB 的清单跟补全抢不到什么带宽，
   * 而那三十秒的代价是打开就关掉的人永远查不到更新。
   */
  watch(): () => void {
    if (!inTauri()) return () => {}
    let timer: ReturnType<typeof setTimeout> | undefined
    let retry = RETRY_DELAY
    let stopped = false
    const schedule = (delay: number) => {
      if (!stopped) timer = setTimeout(() => void tick(), delay)
    }
    const tick = async () => {
      if (!this.automatic) {
        schedule(CHECK_INTERVAL)
        return
      }
      await this.check()
      if (this.failed) {
        // 退避，但仍然比六小时密得多——失败通常是暂时的（开机、断网、换网络）。
        schedule(retry)
        retry = Math.min(retry * 2, RETRY_MAX)
        return
      }
      retry = RETRY_DELAY
      schedule(CHECK_INTERVAL)
      await this.fetchQuietly()
    }
    void tick()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }
}

export const updates = new UpdateStore()
