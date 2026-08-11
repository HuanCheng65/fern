/**
 * 存储占用与瘦身：类型和调用。
 *
 * 数字全是后端量出来的真实字节，界面不自己攒数——报出来的数必须兑现得了，
 * 这条准绳是快照用量页立的（backup.ts），这里沿用。
 */

import { invoke } from '@tauri-apps/api/core'

/** 数据根下各分区的占用。各分区加起来等于 total。 */
export interface StorageReport {
  total: number
  /** 外部实例只算落在数据根下的描述文件，游戏本体不在我们的地盘上。 */
  instances: number
  snapshots: number
  versions: number
  libraries: number
  assets: number
  runtimes: number
  cache: number
  logs: number
  other: number
}

/** 瘦身预检的结果，也是执行的回执。 */
export interface SlimPlan {
  /** 没有实例使用的版本目录名。 */
  versions: string[]
  versionsBytes: number
  /** 没有实例需要的 Java 运行时目录名。 */
  runtimes: string[]
  runtimesBytes: number
  librariesFiles: number
  librariesBytes: number
  assetsFiles: number
  assetsBytes: number
}

/** 这次执行哪几类。 */
export interface SlimContents {
  versions: boolean
  runtimes: boolean
  libraries: boolean
  assets: boolean
}

export const storageReport = () => invoke<StorageReport>('storage_report')

/** 一个实例占多大。逐个拉，几十 GB 的实例不挡住整张报告。 */
export const instanceStorage = (instanceId: string) =>
  invoke<number>('instance_storage', { instanceId })

/** 清空元数据缓存，返回省下的字节数。 */
export const clearCache = () => invoke<number>('clear_cache')

/** 清空日志，返回省下的字节数。 */
export const clearLogs = () => invoke<number>('clear_logs')

export const slimPreview = () => invoke<SlimPlan>('slim_preview')

export const slimApply = (contents: SlimContents) =>
  invoke<SlimPlan>('slim_apply', { contents })

/** 没什么可省的。 */
export const slimEmpty = (plan: SlimPlan) =>
  plan.versions.length === 0 &&
  plan.runtimes.length === 0 &&
  plan.librariesFiles === 0 &&
  plan.assetsFiles === 0

/** 一份计划（或回执）合计多少字节。 */
export const slimBytes = (plan: SlimPlan) =>
  plan.versionsBytes + plan.runtimesBytes + plan.librariesBytes + plan.assetsBytes
