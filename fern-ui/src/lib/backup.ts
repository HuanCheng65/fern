/**
 * 快照与导出：类型、调用和时间的说法。
 *
 * 后端只发 id 和数字，句子在 `i18n/`——`snapshot.<原因>` 与
 * `snapshot.skipped.<原因>` 那几条。这里只负责把它们取出来，以及把 Unix 秒
 * 变成人读的时间。
 *
 * 不做 store：快照列表属于「打开那一屏才要读」的东西，走开就该忘掉。真正
 * 需要跨屏活着的是作业和运行中的游戏，那两样已经有自己的地方。
 */

import { invoke } from '@tauri-apps/api/core'
import { describe } from './i18n'

export interface Skipped {
  path: string
  reason: string
}

export interface Snapshot {
  id: string
  instance: string
  /** Unix 秒。 */
  takenAt: number
  /** `manual`、`before-mod-change`……句子在文案表里。 */
  reason: string
  /** 用户起的名字。有名字的永久保留。 */
  label?: string
  files: number
  /** 内容原本多大。不是它在磁盘上占的——快照之间共用相同的文件。 */
  bytes: number
  mods: number
  saves: string[]
  minecraft: string
  loader: string
  /** 拍摄时加载器的版本。原版没有。 */
  loaderVersion?: string
  /** 拍的时候文件还在变。 */
  inconsistent: boolean
  skipped: Skipped[]
}

export type RestoreScope =
  | { kind: 'all' }
  | { kind: 'save'; name: string }
  | { kind: 'config' }
  | { kind: 'mods' }

export type RestoreMode = { kind: 'replace' } | { kind: 'copy'; name: string }

export interface Missing {
  path: string
  sha1?: string
}

export interface Restored {
  written: number
  bytes: number
  removed: number
  missing: Missing[]
  /** 恢复之前自动拍的那一张。 */
  safety?: string
}

export interface InstanceUsage {
  instance: string
  snapshots: number
  /** 删掉这个实例的全部快照能收回多少。共用的部分不算在内。 */
  reclaimable: number
}

export interface Usage {
  bytes: number
  modsBytes: number
  snapshots: number
  instances: InstanceUsage[]
}

export interface Exported {
  path: string
  bytes: number
  files: number
  /** mrpack 专用：有几个模组是靠下载地址带走的。 */
  linked?: number
}

export interface ExportContents {
  saves: boolean
  mods: boolean
}

export const listSnapshots = (instanceId: string) =>
  invoke<Snapshot[]>('list_snapshots', { instanceId })

export const takeSnapshot = (instanceId: string, label?: string) =>
  invoke<Snapshot>('take_snapshot', { instanceId, label })

export const restoreSnapshot = (
  instanceId: string,
  snapshot: string,
  scope: RestoreScope,
  mode: RestoreMode,
) => invoke<Restored>('restore_snapshot', { instanceId, snapshot, scope, mode })

export const deleteSnapshot = (instanceId: string, snapshot: string) =>
  invoke<void>('delete_snapshot', { instanceId, snapshot })

export const labelSnapshot = (instanceId: string, snapshot: string, label?: string) =>
  invoke<Snapshot>('label_snapshot', { instanceId, snapshot, label })

export const backupUsage = () => invoke<Usage>('backup_usage')

export const exportWorld = (instanceId: string, save: string, destination: string) =>
  invoke<Exported>('export_world', { instanceId, save, destination })

export const exportFernpack = (
  instanceId: string,
  contents: ExportContents,
  destination: string,
) => invoke<Exported>('export_fernpack', { instanceId, contents, destination })

export const exportMrpack = (instanceId: string, destination: string) =>
  invoke<Exported>('export_mrpack', { instanceId, destination })

/** 这一张为什么在这里。 */
export const why = (snapshot: Snapshot) => describe(`snapshot.${snapshot.reason}`)

/** 某一项为什么没进快照。 */
export const whySkipped = (skipped: Skipped) => describe(`snapshot.skipped.${skipped.reason}`)

/** 有名字的永久保留，不会被保留策略剪掉。 */
export const pinned = (snapshot: Snapshot) =>
  snapshot.label !== undefined || snapshot.reason === 'manual'

const at = (seconds: number) => new Date(seconds * 1000)

/** 完整时刻，浮层里那一行用。 */
export const moment = (seconds: number) =>
  at(seconds).toLocaleString('zh-CN', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })

/** 另存一份世界时的默认名字：`家 (2026-08-07)`。 */
export const copyName = (save: string, seconds: number) =>
  `${save} (${at(seconds).toLocaleDateString('sv-SE')})`

/** 导出时默认的文件名。去掉在文件名里会出事的字符。 */
export const fileStem = (name: string) => name.replace(/[/\\:*?"<>|]/g, '_').trim() || 'instance'
