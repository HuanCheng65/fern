/**
 * 游戏进程的状态。
 *
 * 这个 store 曾经什么都管：下载进度、启动阶段、日志、崩溃。于是它成了「同一
 * 时刻只有一件事」的隐含前提——装模组也往同一条流里发进度，把启动的进度盖掉，
 * 而因为它的 `busy` 没被立起来，那份进度谁也没显示。「点一下没反应，过一会
 * 自己好了」就是这么来的。
 *
 * 现在它只管一件事：**游戏跑没跑**。那是一个状态，不是一件耗时的事——它没有
 * 进度，也不会「完成」。补全和下载是作业，进度归 [`jobs`](./jobs.svelte.ts)。
 *
 * `busy` 剩下的意思很窄：我点的这一下还没回来。它不是进度，只是防止同一颗
 * 按钮被连点两次——真正的进度由后端宣告的作业说。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { instances, inTauri } from './instances.svelte'
import { contributes, PRIORITY, type Presence } from './island.svelte'
import { commands } from './palette.svelte'
import { jobs } from './jobs.svelte'
import { nav } from './nav.svelte'
import { prefs } from './prefs.svelte'

export type LaunchStage =
  | 'resolving_version'
  | 'checking_files'
  | 'preparing_java'
  | 'building_command'
  | 'starting_process'
  | 'running'
  | 'exited'

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error'

export interface CrashDiagnosis {
  id: string
  title: string
  detail: string
}

export interface CrashReport {
  instanceId: string
  exitCode: number | null
  diagnosis: CrashDiagnosis | null
  reportPath: string | null
  excerpt: string
}

export interface GameLogLine {
  level: LogLevel
  message: string
}

/** 游戏跑着时的堆压力，几秒一条。读不到就不发，所以这里可以一直没有值。 */
export interface MemoryPressure {
  instanceId: string
  usedMb: number
  peakMb: number
  xmxMb: number
}

type LauncherEvent =
  | { type: 'launch_stage'; payload: { instanceId: string; stage: LaunchStage } }
  | { type: 'game_log'; payload: { instanceId: string; level: LogLevel; message: string } }
  | { type: 'game_exited'; payload: { instanceId: string; exitCode: number | null } }
  | { type: 'game_memory'; payload: MemoryPressure }
  | { type: 'game_crashed'; payload: CrashReport }
  | { type: string; payload: unknown }

/** 日志留最近这么多行。再多也没人往上翻，只会让界面越跑越慢。 */
const LOG_LIMIT = 800

/**
 * 作业的标题要给人看，而调用方手里只有 id。
 *
 * 名字在这里查而不是让每个调用点传进来：调用点已经有 id 了，再多要一个名字
 * 只是把同一件事说两遍，而且总有一处会忘。
 */
const nameOf = (instanceId: string) =>
  instances.list.find((item) => item.id === instanceId)?.name ?? '实例'

class LaunchStore {
  /** 我点的这一下还没回来。不是进度——进度是作业的事。 */
  busy = $state(false)
  /** 游戏窗口已经开出来了。 */
  running = $state(false)
  error = $state('')
  /** 崩了才有值。正常退出不该在界面上留下任何痕迹。 */
  crash = $state<CrashReport | null>(null)
  log = $state<GameLogLine[]>([])
  /**
   * 这一轮忙的是哪个实例。
   *
   * 实例详情页的日志 tab 要靠它判断这段日志是不是自己的——把 A 实例的崩溃栈
   * 显示在 B 的页面里，比不显示更糟。
   */
  instanceId = $state('')
  /** 正在跑的是哪个实例，用来在岛上叫出它的名字。 */
  runningName = $state('')
  /**
   * 最近一次读到的堆压力。游戏没在跑、或者读不到 GC 日志时是 null。
   *
   * 它只喂岛上那条细线。**没有值就没有线**——一条编出来的线比没有线更糟。
   */
  memory = $state<MemoryPressure | null>(null)

  #unlisten: UnlistenFn | undefined

  async connect() {
    if (!inTauri() || this.#unlisten) return
    this.#unlisten = await listen<LauncherEvent>('launcher-event', ({ payload }) =>
      this.#onEvent(payload),
    )
  }

  disconnect() {
    this.#unlisten?.()
    this.#unlisten = undefined
  }

  #onEvent(event: LauncherEvent) {
    switch (event.type) {
      case 'launch_stage':
        this.#onStage((event.payload as { stage: LaunchStage }).stage)
        break
      case 'game_log':
        this.#onLog(event.payload as GameLogLine)
        break
      case 'game_exited':
        this.running = false
        this.runningName = ''
        this.memory = null
        break
      case 'game_memory':
        this.memory = event.payload as MemoryPressure
        break
      case 'game_crashed':
        this.crash = event.payload as CrashReport
        break
    }
  }

  #onStage(stage: LaunchStage) {
    if (stage !== 'running') return
    this.running = true
    // 这一刻才最小化，不是点启动那一刻：补全可能要几分钟，中途把启动器收走，
    // 用户就看不到进度了。
    if (prefs.minimizeOnLaunch && inTauri()) {
      void getCurrentWindow().minimize()
    }
  }

  #onLog(line: GameLogLine) {
    this.log.push({ level: line.level, message: line.message })
    if (this.log.length > LOG_LIMIT) {
      this.log = this.log.slice(-LOG_LIMIT)
    }
  }

  #begin(instanceId: string) {
    this.instanceId = instanceId
    this.busy = true
    this.error = ''
    this.crash = null
    this.log = []
    this.memory = null
  }

  /**
   * 启动。
   *
   * `into` 是「直接进去」：一个存档目录名或一个服务器地址。游戏自己支持这件事
   * （quickPlay 参数），而启动器是唯一知道你有哪些世界和哪些服务器的地方——
   * 把这两半接上，搜一个世界名回车就直接落在那个世界里。
   */
  async launch(instanceId: string, into?: { world?: string; server?: string }) {
    if (this.busy || this.running) return
    this.#begin(instanceId)
    const name = nameOf(instanceId)
    try {
      if (!inTauri()) {
        await jobs.rehearse(`启动 ${name}`, [instanceId])
        this.error = '浏览器预览，无法真正启动'
        return
      }
      this.runningName = name
      // 标题和 subjects 由这一侧给：作业挂在谁身上是界面的知识，后端只负责
      // 宣告它的存在和进展，不负责编一个显示用的名字。
      await invoke<{ processId: number }>('launch_instance', {
        instanceId,
        world: into?.world ?? null,
        server: into?.server ?? null,
        title: `启动 ${name}`,
        subjects: [instanceId],
      })
      // 到这里只是进程起来了。真正的「跑起来了」由 launch_stage 事件说，
      // 那才是窗口已经开出来的时刻。
    } catch (error) {
      this.runningName = ''
      this.error = String(error)
    } finally {
      this.busy = false
    }
  }

  /**
   * 把这个实例补齐到能启动的状态。
   *
   * 建实例之后立刻跑一次，而不是留到第一次点启动——上一版建完只是把选择记在
   * 一个 json 里，装 Forge 要等到你第一次点「启动」的那一刻才开始，而装 Forge
   * 要在本地跑一个第三方安装器，可能好几分钟。用户以为自己只是点了启动。
   *
   * `title` 决定岛上怎么称呼这件事：刚建完叫「准备」，事后手动跑叫「校验」，
   * 做的是同一件事，但对用户来说不是同一个时刻。
   */
  async repair(instanceId: string, title = `校验 ${nameOf(instanceId)}`) {
    if (this.busy) return
    this.#begin(instanceId)
    try {
      if (!inTauri()) {
        await jobs.rehearse(title, [instanceId])
        return
      }
      await invoke('prepare_instance', {
        instanceId,
        title,
        subjects: [instanceId],
      })
    } catch (error) {
      this.error = String(error)
    } finally {
      this.busy = false
    }
  }

  dismissError() {
    this.error = ''
  }

  dismissCrash() {
    this.crash = null
  }
}

export const launch = new LaunchStore()

/**
 * 岛上关于游戏的那一句。
 *
 * 游戏在跑是**状态**不是作业：它没有进度，也不会「完成」。所以它只报告自己
 * 还活着，不带任何百分比。
 */
contributes((): Presence[] => {
  if (!launch.running) return []
  const memory = launch.memory
  // 堆压力是这一句里唯一一个会动的数，而它几秒才变一次——所以它进 detail，
  // 不进那条会被反复念出来的 label。
  const pressure = memory
    ? `${gigabytes(memory.usedMb)} / ${gigabytes(memory.xmxMb)}`
    : ''
  return [
    {
      id: 'game',
      priority: PRIORITY.live,
      tone: 'live',
      label: launch.runningName || '运行中',
      // 细线画的是水位占堆的比例，不是进度——游戏不会「完成」。
      fill: memory && memory.xmxMb > 0 ? memory.usedMb / memory.xmxMb : undefined,
      rows: [
        {
          id: 'game',
          label: launch.runningName || '游戏运行中',
          detail: pressure ? `内存 ${pressure}` : '运行中',
        },
      ],
      actions: [{ label: '查看日志', run: () => nav.show('log') }],
    },
  ]
})

/** MB 变成一句话。和实例设置那一屏用的是同一条规则。 */
function gigabytes(mb: number) {
  const value = mb / 1024
  return Math.abs(value - Math.round(value)) < 0.05
    ? `${Math.round(value)} GB`
    : `${value.toFixed(1)} GB`
}


/**
 * 启动与校验注册在这里，而不是外壳里。
 *
 * 外壳曾经手写一张动作表，于是它被迫认识启动、日志、目录、设置——加一个功能
 * 就要回来改它一次。动作和它在界面上那个看得见的入口住在同一个文件，这条
 * 纪律才守得住。
 */
commands(() => {
  const current = () => {
    const item = instances.current
    if (!item) return undefined
    return {
      type: 'instance' as const,
      id: item.id,
      title: item.name,
      hint: `${item.gameVersion} · ${item.loader}`,
      seed: item.cover,
      run: () => instances.select(item.id),
    }
  }
  return [
    // 游戏已经在跑的时候不列出启动：再点一下会起第二个进程，两份游戏抢同一个
    // 存档目录。
    ...(launch.running
      ? []
      : [
          {
            id: 'instance.launch',
            title: '启动',
            hint: instances.current?.name,
            accepts: 'instance' as const,
            subject: current,
            run: (subject?: { id: string }) => {
              if (!subject) return
              instances.select(subject.id)
              void launch.launch(subject.id)
            },
          },
        ]),
    {
      id: 'instance.repair',
      title: '校验游戏文件',
      accepts: 'instance' as const,
      subject: current,
      run: (subject?: { id: string }) => {
        if (subject) void launch.repair(subject.id)
      },
    },
    // 日志平时不该占地方，但出事的时候必须找得到——所以只在真的有内容时出现。
    ...(launch.log.length > 0
      ? [
          {
            id: 'log.open',
            title: '查看游戏日志',
            hint: `${launch.log.length} 行`,
            accepts: 'none' as const,
            run: () => nav.show('log'),
          },
        ]
      : []),
  ]
})
