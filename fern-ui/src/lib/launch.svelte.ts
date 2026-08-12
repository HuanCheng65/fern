/**
 * 游戏进程的状态。
 *
 * 这个 store 曾经什么都管：下载进度、启动阶段、日志、崩溃。于是它成了「同一
 * 时刻只有一件事」的隐含前提——装模组也往同一条流里发进度，把启动的进度盖掉，
 * 而因为它的 `busy` 没被立起来，那份进度谁也没显示。「点一下没反应，过一会
 * 自己好了」就是这么来的。
 *
 * 现在它只管一件事：**哪些游戏在跑，各自跑到哪一段**。那是状态，不是耗时的
 * 事——它没有进度，也不会「完成」。补全和下载是作业，进度归
 * [`jobs`](./jobs.svelte.ts)。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { instances, inTauri } from './instances.svelte'
import { contributes, PRIORITY, type Presence } from './island.svelte'
import { commands } from 'fern-kit/parts/palette'
import { jobs } from './jobs.svelte'
import { nav } from './nav.svelte'
import { prefs } from './prefs.svelte'
import type { FixAction } from './advice'

export type LaunchStage =
  | 'resolving_version'
  | 'checking_files'
  | 'preparing_java'
  | 'building_command'
  | 'starting_process'
  | 'running'
  | 'exited'

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error'

/** 认出来的一条原因。文案在 i18n 那边按 `crash.<id>` 查。 */
export interface Diagnosis {
  id: string
  /** exact 认得出具体是哪两个东西撞了 / named 说得出主语 / generic 只认类别。 */
  level: 'exact' | 'named' | 'generic'
  args: Record<string, string>
  action?: FixAction
}

/** 可能有关的模组。和有没有认出原因无关。 */
export interface Suspect {
  modId: string
  name: string
  version?: string
  /** 它在栈里第几帧出现，越小越可疑。 */
  depth: number
  reason: 'stack' | 'mixin'
}

export interface CrashReport {
  instanceId: string
  exitCode: number | null
  /** 认得越具体的排越前面。空表示一条都没认出来，界面照实说。 */
  diagnoses: Diagnosis[]
  suspects: Suspect[]
  reportPath?: string
  hsErrPath?: string
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

/** 一个实例现在处在哪一段。没有条目就是没在跑。 */
export type GamePhase = 'preparing' | 'starting' | 'running'

export interface GameState {
  phase: GamePhase
  processId?: number
  /** Unix 秒，进程起来的时刻。 */
  startedAt?: number
}

class LaunchStore {
  /**
   * 跑着的（以及正在起来的）游戏，按实例 id。
   *
   * 曾经这里是两个全局布尔：`busy`（我点的这一下还没回来）和 `running`（窗口
   * 开出来了）。两个问题——
   *
   * **中间那一段是空的。** invoke 一返回 `busy` 就落回 false，而 `running` 要
   * 等日志里的窗口标志，等不到还有十五秒兜底。那十几秒里按钮显示的是「启动」，
   * 看起来就是刚才那一下没生效。所以现在的第一段 `preparing` 从**点击那一刻**
   * 就立起来，一直接到 `starting`（进程有了 pid）再到 `running`（窗口出来了），
   * 中间没有缝。
   *
   * **一个布尔说不出是谁在跑。** 于是全局只能跑一个游戏。真正不能重复的是
   * 同一份游戏目录（后端按目录挡，见 `launch::running`），不是「任何一个」。
   */
  games = $state<Record<string, GameState>>({})
  error = $state('')
  /** 崩了才有值。正常退出不该在界面上留下任何痕迹。 */
  crash = $state<CrashReport | null>(null)
  /**
   * 这一轮看的是哪个实例的日志。
   *
   * 实例详情页的日志 tab 要靠它判断这段日志是不是自己的——把 A 实例的崩溃栈
   * 显示在 B 的页面里，比不显示更糟。
   */
  instanceId = $state('')
  #logs = $state<Record<string, GameLogLine[]>>({})
  #memory = $state<Record<string, MemoryPressure>>({})

  /** 正在看的那个实例的日志。 */
  log = $derived(this.#logs[this.instanceId] ?? [])
  /** 有没有游戏在跑。面板和岛用它，具体到某一个实例要用 `phaseOf`。 */
  anyRunning = $derived(Object.values(this.games).some((game) => game.phase !== 'preparing'))
  running = $derived(Object.keys(this.games).length > 0)

  #unlisten: UnlistenFn | undefined

  phaseOf(instanceId: string): GamePhase | undefined {
    return this.games[instanceId]?.phase
  }

  /** 这个实例现在不该再被点启动。 */
  occupied(instanceId: string) {
    return this.games[instanceId] !== undefined
  }

  memoryOf(instanceId: string) {
    return this.#memory[instanceId]
  }

  async connect() {
    if (!inTauri() || this.#unlisten) return
    this.#unlisten = await listen<LauncherEvent>('launcher-event', ({ payload }) =>
      this.#onEvent(payload),
    )
    await this.sync()
  }

  /**
   * 对一次后端那张表。
   *
   * 事件可能在界面还没挂上监听时就发过了，进程也可能在启动器不知情的时候没了。
   * 只靠事件，界面上迟早留下一个永远「运行中」的按钮。
   */
  async sync() {
    if (!inTauri()) return
    try {
      const live = await invoke<
        { instanceId: string; processId: number; startedAt: number; ready: boolean }[]
      >('running_games')
      const next: Record<string, GameState> = {}
      for (const game of live) {
        next[game.instanceId] = {
          phase: game.ready ? 'running' : 'starting',
          processId: game.processId,
          startedAt: game.startedAt,
        }
      }
      // 正在准备的那些还没有进程，后端不知道它们，别被这一次对表抹掉。
      for (const [id, game] of Object.entries(this.games)) {
        if (game.phase === 'preparing' && !next[id]) next[id] = game
      }
      this.games = next
    } catch {
      // 查不到就维持现状：一次查询失败不该让界面上的状态全部消失。
    }
  }

  disconnect() {
    this.#unlisten?.()
    this.#unlisten = undefined
  }

  #onEvent(event: LauncherEvent) {
    switch (event.type) {
      case 'launch_stage': {
        const payload = event.payload as { instanceId: string; stage: LaunchStage }
        this.#onStage(payload.instanceId, payload.stage)
        break
      }
      case 'game_log': {
        const line = event.payload as GameLogLine & { instanceId: string }
        this.#onLog(line.instanceId, line)
        break
      }
      case 'game_exited': {
        const { instanceId } = event.payload as { instanceId: string }
        delete this.games[instanceId]
        delete this.#memory[instanceId]
        break
      }
      case 'game_memory': {
        const pressure = event.payload as MemoryPressure
        this.#memory[pressure.instanceId] = pressure
        break
      }
      case 'game_crashed':
        this.crash = event.payload as CrashReport
        break
    }
  }

  #onStage(instanceId: string, stage: LaunchStage) {
    if (stage !== 'running') return
    this.#advance(instanceId, 'running')
    // 这一刻才最小化，不是点启动那一刻：补全可能要几分钟，中途把启动器收走，
    // 用户就看不到进度了。
    if (prefs.minimizeOnLaunch && inTauri()) {
      void getCurrentWindow().minimize()
    }
  }

  /** 只往前走。事件到达的顺序不保证，倒退回去会让按钮闪一下。 */
  #advance(instanceId: string, phase: GamePhase, extra: Partial<GameState> = {}) {
    const order: GamePhase[] = ['preparing', 'starting', 'running']
    const current = this.games[instanceId]
    if (current && order.indexOf(current.phase) > order.indexOf(phase)) {
      this.games[instanceId] = { ...current, ...extra }
      return
    }
    this.games[instanceId] = { ...current, ...extra, phase }
  }

  #onLog(instanceId: string, line: GameLogLine) {
    const lines = this.#logs[instanceId] ?? []
    lines.push({ level: line.level, message: line.message })
    this.#logs[instanceId] = lines.length > LOG_LIMIT ? lines.slice(-LOG_LIMIT) : lines
  }

  #begin(instanceId: string) {
    this.instanceId = instanceId
    this.error = ''
    this.crash = null
    this.#logs[instanceId] = []
    delete this.#memory[instanceId]
  }

  /**
   * 启动。
   *
   * `into` 是「直接进去」：一个存档目录名或一个服务器地址。游戏自己支持这件事
   * （quickPlay 参数），而启动器是唯一知道你有哪些世界和哪些服务器的地方——
   * 把这两半接上，搜一个世界名回车就直接落在那个世界里。
   */
  async launch(instanceId: string, into?: { world?: string; server?: string }) {
    if (this.occupied(instanceId)) return
    this.#begin(instanceId)
    // 从点击这一刻就占住，中间不留缝。
    this.games[instanceId] = { phase: 'preparing' }
    const name = nameOf(instanceId)
    try {
      if (!inTauri()) {
        await jobs.rehearse(`启动 ${name}`, [instanceId])
        this.error = '浏览器预览，无法真正启动'
        delete this.games[instanceId]
        return
      }
      // 标题和 subjects 由这一侧给：作业挂在谁身上是界面的知识，后端只负责
      // 宣告它的存在和进展，不负责编一个显示用的名字。
      const started = await invoke<{ processId: number }>('launch_instance', {
        instanceId,
        world: into?.world ?? null,
        server: into?.server ?? null,
        title: `启动 ${name}`,
        subjects: [instanceId],
      })
      // 到这里只是进程起来了。真正的「跑起来了」由 launch_stage 事件说，
      // 那才是窗口已经开出来的时刻。
      this.#advance(instanceId, 'starting', { processId: started.processId })
    } catch (error) {
      delete this.games[instanceId]
      this.error = String(error)
    }
  }

  /**
   * 强行结束。
   *
   * 是 kill 不是「保存并退出」——没有哪个启动器做得到后者。所以按钮上要说清
   * 没存的进度会丢，而这个按钮存在的理由本来就是游戏已经不响应了。
   */
  async stop(instanceId: string) {
    if (!inTauri()) return
    try {
      await invoke('stop_game', { instanceId })
    } catch (error) {
      this.error = String(error)
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
   *
   * `recheck` 决定要不要把每个文件都真读一遍重算哈希。默认读——这个方法的默认
   * 调用方式就是用户点「校验」，而他点它正是因为不信任磁盘上那份。建完实例后的
   * 「准备」传 `false`：那些文件刚落盘，没有理由再读一遍。
   */
  async repair(
    instanceId: string,
    title = `校验 ${nameOf(instanceId)}`,
    recheck = true,
  ) {
    if (this.occupied(instanceId)) return
    this.#begin(instanceId)
    try {
      if (!inTauri()) {
        await jobs.rehearse(title, [instanceId])
        return
      }
      await invoke('prepare_instance', {
        instanceId,
        recheck,
        title,
        subjects: [instanceId],
      })
    } catch (error) {
      this.error = String(error)
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
  return Object.entries(launch.games)
    .filter(([, game]) => game.phase !== 'preparing')
    .map(([instanceId, game]) => {
      const name = nameOf(instanceId)
      const memory = launch.memoryOf(instanceId)
      // 堆压力是这一句里唯一一个会动的数，而它几秒才变一次——所以它进 detail，
      // 不进那条会被反复念出来的 label。
      const pressure = memory ? `${gigabytes(memory.usedMb)} / ${gigabytes(memory.xmxMb)}` : ''
      const starting = game.phase === 'starting'
      return {
        // 一个实例一条：同时跑两个的时候，两条各说各的。
        id: `game:${instanceId}`,
        priority: PRIORITY.live,
        tone: 'live',
        label: name,
        // 细线画的是水位占堆的比例，不是进度——游戏不会「完成」。
        fill: memory && memory.xmxMb > 0 ? memory.usedMb / memory.xmxMb : undefined,
        rows: [
          {
            id: `game:${instanceId}`,
            label: name,
            detail: starting ? '等待游戏窗口' : pressure ? `内存 ${pressure}` : '运行中',
          },
        ],
        actions: [
          {
            label: '查看日志',
            run: () => {
              launch.instanceId = instanceId
              nav.show('log')
            },
          },
          { label: '强制结束', run: () => void launch.stop(instanceId) },
        ],
      } satisfies Presence
    })
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
    // 这个实例已经在跑的时候不列出启动：再点一下会起第二个进程，两份游戏抢
    // 同一个存档目录。别的实例照常可以启动。
    ...(instances.current && launch.occupied(instances.current.id)
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
