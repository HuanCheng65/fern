/**
 * 启动与文件补全的状态。
 *
 * 界面上只有一颗启动键，所以这里也只暴露一件事的状态：现在忙不忙、忙到
 * 哪一步、进度多少。文档里说启动是英雄交互——那它的进度就该长在那颗按钮
 * 身上，而不是另起一个进度条区域。
 *
 * 进度分两段说：`label` 是人话（在做什么），`detail` 是机器数（多少字节、
 * 多快）。人话给所有人看，机器数用等宽，看不看都不影响操作。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { inTauri } from './instances.svelte'

type DownloadEvent =
  | { type: 'status'; message: string }
  | { type: 'task_started'; total_files: number; total_bytes: number }
  | { type: 'file_done'; path: string; bytes: number }
  | { type: 'progress'; done_bytes: number; speed_bps: number }
  | { type: 'task_finished'; failed: string[] }

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

  #totalBytes = 0
  #unlisten: UnlistenFn | undefined
  #resetTimer: ReturnType<typeof setTimeout> | undefined

  async connect() {
    if (!inTauri() || this.#unlisten) return
    this.#unlisten = await listen<DownloadEvent>('download-event', ({ payload }) =>
      this.#onEvent(payload),
    )
  }

  disconnect() {
    this.#unlisten?.()
    this.#unlisten = undefined
    clearTimeout(this.#resetTimer)
  }

  #onEvent(event: DownloadEvent) {
    if (event.type === 'status') {
      this.label = event.message
    }
    if (event.type === 'task_started') {
      this.#totalBytes = event.total_bytes
      this.label = '补全游戏文件'
      this.detail = `${event.total_files} 个文件`
      this.progress = event.total_bytes > 0 ? 0 : -1
    }
    if (event.type === 'progress') {
      if (this.#totalBytes > 0) {
        this.progress = Math.min(99, (event.done_bytes / this.#totalBytes) * 100)
      }
      this.detail = `${formatBytes(event.done_bytes)} / ${formatBytes(this.#totalBytes)} · ${formatBytes(event.speed_bps)}/s`
    }
    if (event.type === 'task_finished') {
      this.detail = event.failed.length > 0 ? `${event.failed.length} 个文件需要重试` : ''
    }
  }

  #begin(label: string) {
    clearTimeout(this.#resetTimer)
    this.busy = true
    this.error = ''
    this.label = label
    this.detail = ''
    this.progress = -1
    this.#totalBytes = 0
  }

  #finish(label: string) {
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
      const result = await invoke<{ processId: number }>('launch_instance', {
        instanceId,
        playerName,
      })
      this.#finish(`游戏已启动 · PID ${result.processId}`)
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
