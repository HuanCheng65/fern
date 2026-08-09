/**
 * 崩溃诊断里那两个形状。
 *
 * 后端给的 `Diagnosis` 带的是文案 id 和参数，翻成句子是产品那边的事（它拿着
 * 语言包）。这块板子只认翻好之后的样子，所以这里有一个 `Diagnosed`。
 */

import type { FixAction } from './advice'

/** 已经翻成句子的一条诊断。 */
export interface Diagnosed {
  id: string
  title: string
  detail: string
  action?: FixAction
}

/** 可能有关的模组。和有没有认出原因无关。 */
export interface Suspect {
  modId: string
  name: string
  version?: string
}
