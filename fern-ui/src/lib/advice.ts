/**
 * 诊断给出的那颗按钮。
 *
 * 崩溃分析和启动前预检查用的是同一套动作，所以处理也只有这一份。后端给的是
 * 一个封闭枚举，不是自由文本——每一种背后都要有真实的界面动作。
 *
 * **做不了的动作不给按钮。** 后端可能比界面新，那时候宁可只显示一句诊断，也
 * 不要一颗点了没反应的按钮。`label()` 返回空就是没有按钮。
 */

import { invoke } from '@tauri-apps/api/core'
import { instances, inTauri } from './instances.svelte'
import { nav } from './nav.svelte'
import { supply } from './supply.svelte'

export type FixAction =
  | { kind: 'install-mod'; query: string }
  | { kind: 'remove-mod'; file: string }
  | { kind: 'use-java'; major: number }
  | { kind: 'set-memory'; mb: number }
  | { kind: 'open-path'; path: string }
  | { kind: 'open-url'; url: string }

/** 这颗按钮上写什么。返回空表示这一条现在做不了，不该有按钮。 */
export function label(action: FixAction | undefined): string {
  if (!action) return ''
  switch (action.kind) {
    case 'install-mod':
      return '去安装'
    case 'remove-mod':
      return '删除'
    default:
      return ''
  }
}

/** 执行。调用方负责在之后刷新自己那一屏。 */
export async function perform(action: FixAction, instanceId: string) {
  switch (action.kind) {
    case 'install-mod': {
      // 带着名字去补给站，而不是直接装：版本要对得上这个实例，该由用户过目。
      supply.aimAt(instanceId)
      supply.query = action.query
      supply.refresh()
      nav.go('supply')
      return
    }
    case 'remove-mod': {
      if (!inTauri()) return
      await invoke('remove_mod', { instanceId, fileName: action.file })
      await instances.load()
      return
    }
  }
}
