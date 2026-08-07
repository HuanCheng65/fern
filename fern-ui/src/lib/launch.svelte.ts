/**
 * 启动与文件补全的状态。
 *
 * 界面上只有一颗启动键，所以这里也只暴露一件事的状态：现在忙不忙、忙到
 * 哪一步、进度多少。文档里说启动是英雄交互——那它的进度就该长在那颗按钮
 * 身上，而不是另起一个进度条区域。
 *
 * 进度分两段说：`label` 是人话（在做什么），`detail` 是机器数（多少字节、
 * 多快）。人话给所有人看，机器数用等宽，看不看都不影响操作。
 *
 * 后端只有一条事件流（`launcher-event`）：下载、启动阶段、游戏日志、退出、
 * 崩溃全在里面。分成两条通道听起来更整齐，但它们说的是同一件事——「启动器
 * 现在怎么样了」——界面也就该在一个地方回答。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { inTauri } from './instances.svelte'
import { prefs } from './prefs.svelte'

/** 类型标签是 snake_case，数据字段是 camelCase——后端一条规则，这里照抄。 */
type DownloadEvent =
  | { type: 'status'; message: string }
  | { type: 'task_started'; totalFiles: number; totalBytes: number }
  | { type: 'file_done'; path: string; bytes: number }
  | { type: 'progress'; doneBytes: number; speedBps: number }
  | { type: 'task_finished'; failed: string[] }

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

type LauncherEvent =
  | { type: 'download'; payload: DownloadEvent }
  | { type: 'launch_stage'; payload: { instanceId: string; stage: LaunchStage } }
  | { type: 'game_log'; payload: { instanceId: string; level: LogLevel; message: string } }
  | { type: 'game_exited'; payload: { instanceId: string; exitCode: number | null } }
  | { type: 'game_crashed'; payload: CrashReport }

/** 每个阶段说一句人话。机器名字（`resolving_version`）不该出现在界面上。 */
const STAGE_LABEL: Record<LaunchStage, string> = {
  resolving_version: '读取版本信息',
  checking_files: '检查游戏文件',
  preparing_java: '准备 Java',
  building_command: '组装启动命令',
  starting_process: '启动游戏',
  running: '游戏运行中',
  exited: '游戏已退出',
}

/** 日志留最近这么多行。再多也没人往上翻，只会让界面越跑越慢。 */
const LOG_LIMIT = 800

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(0)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

class LaunchStore {
  busy = $state(false)
  label = $state('')
  detail = $state('')
  /** -1 表示进度未知，按钮画成不定量的样子而不是停在 0%。 */
  progress = $state(-1)
  error = $state('')

  /** 游戏窗口已经开出来了。这之后启动键不该再显示进度。 */
  running = $state(false)
  /** 崩了才有值。正常退出不该在界面上留下任何痕迹。 */
  crash = $state<CrashReport | null>(null)
  log = $state<GameLogLine[]>([])

  #totalBytes = 0
  #unlisten: UnlistenFn | undefined
  #resetTimer: ReturnType<typeof setTimeout> | undefined

  async connect() {
    if (!inTauri() || this.#unlisten) return
    this.#unlisten = await listen<LauncherEvent>('launcher-event', ({ payload }) =>
      this.#onEvent(payload),
    )
  }

  disconnect() {
    this.#unlisten?.()
    this.#unlisten = undefined
    clearTimeout(this.#resetTimer)
  }

  #onEvent(event: LauncherEvent) {
    switch (event.type) {
      case 'download':
        this.#onDownload(event.payload)
        break
      case 'launch_stage':
        this.#onStage(event.payload.stage)
        break
      case 'game_log':
        this.#onLog(event.payload)
        break
      case 'game_exited':
        this.running = false
        this.busy = false
        this.label = ''
        this.detail = ''
        this.progress = -1
        break
      case 'game_crashed':
        this.crash = event.payload
        break
    }
  }

  #onStage(stage: LaunchStage) {
    if (stage === 'running') {
      this.running = true
      // 窗口开出来了，进度条就该功成身退——它描述的是「还要多久能玩上」。
      this.#finish('游戏运行中')
      // 这一刻才最小化，不是点启动那一刻：补全可能要几分钟，中途把启动器
      // 收走，用户就看不到进度了。
      if (prefs.minimizeOnLaunch && inTauri()) {
        void getCurrentWindow().minimize()
      }
      return
    }
    if (stage === 'exited') return
    this.label = STAGE_LABEL[stage] ?? ''
  }

  #onLog(line: { level: LogLevel; message: string }) {
    this.log.push({ level: line.level, message: line.message })
    if (this.log.length > LOG_LIMIT) {
      this.log = this.log.slice(-LOG_LIMIT)
    }
  }

  #onDownload(event: DownloadEvent) {
    if (event.type === 'status') {
      this.label = event.message
    }
    if (event.type === 'task_started') {
      this.#totalBytes = event.totalBytes
      this.label = '补全游戏文件'
      this.detail = `${event.totalFiles} 个文件`
      this.progress = event.totalBytes > 0 ? 0 : -1
    }
    if (event.type === 'progress') {
      if (this.#totalBytes > 0) {
        this.progress = Math.min(99, (event.doneBytes / this.#totalBytes) * 100)
      }
      this.detail = `${formatBytes(event.doneBytes)} / ${formatBytes(this.#totalBytes)} · ${formatBytes(event.speedBps)}/s`
    }
    if (event.type === 'task_finished') {
      this.detail = event.failed.length > 0 ? `${event.failed.length} 个文件需要重试` : ''
    }
  }

  #begin(label: string) {
    clearTimeout(this.#resetTimer)
    this.busy = true
    this.error = ''
    this.crash = null
    this.log = []
    this.running = false
    this.label = label
    this.detail = ''
    this.progress = -1
    this.#totalBytes = 0
  }

  #finish(label: string) {
    clearTimeout(this.#resetTimer)
    this.progress = 100
    this.label = label
    this.detail = ''
    this.#resetTimer = setTimeout(() => {
      this.busy = false
      this.label = ''
      this.progress = -1
    }, 1400)
  }

  #fail(error: unknown) {
    this.error = String(error)
    this.busy = false
    this.label = ''
    this.detail = ''
    this.progress = -1
  }

  async launch(instanceId: string, playerName: string) {
    if (this.busy) return
    this.#begin('读取版本信息')
    if (!inTauri()) return this.#preview()
    try {
      await invoke<{ processId: number }>('launch_instance', { instanceId, playerName })
      // 到这里只是进程起来了。真正的「跑起来了」由 launch_stage 事件说，
      // 那才是窗口已经开出来的时刻。
    } catch (error) {
      this.#fail(error)
    }
  }

  async repair(instanceId: string) {
    if (this.busy) return
    this.#begin('校验游戏文件')
    if (!inTauri()) return this.#preview()
    try {
      await invoke('prepare_instance', { instanceId })
      this.#finish('文件已完整')
    } catch (error) {
      this.#fail(error)
    }
  }

  dismissError() {
    this.error = ''
  }

  dismissCrash() {
    this.crash = null
  }

  /** 浏览器预览没有后端，走一段假进度，只为让布局在两种状态下都看得到。 */
  #preview() {
    this.progress = 0
    this.detail = '浏览器预览'
    const timer = setInterval(() => {
      this.progress = Math.min(99, this.progress + 11)
      if (this.progress >= 99) {
        clearInterval(timer)
        this.#finish('浏览器预览 · 无法真正启动')
      }
    }, 260)
  }
}

export const launch = new LaunchStore()
