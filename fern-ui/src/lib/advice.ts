/**
 * 诊断给出的那颗按钮。
 *
 * 崩溃分析、启动前预检查和文件对账用的是同一套动作，所以处理也只有这一份。后端给的是
 * 一个封闭枚举，不是自由文本——每一种背后都要有真实的界面动作。
 *
 * **做不了的动作不给按钮。** 后端可能比界面新，那时候宁可只显示一句诊断，也
 * 不要一颗点了没反应的按钮。`label()` 返回空就是没有按钮。
 */

import { invoke } from '@tauri-apps/api/core'
import type { FixAction } from 'fern-kit/parts/advice'
import { instances, inTauri } from './instances.svelte'
import { nav } from './nav.svelte'
import { supply } from './supply.svelte'
import { restoreSnapshot } from './backup'
import { preflight } from './preflight.svelte'

// 枚举本身和按钮上的字都在 kit 里——官网要画同一条诊断，也得认识它们。
export { label, type FixAction } from 'fern-kit/parts/advice'

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
    case 'use-java': {
      // 装上它**并且**把这个实例钉在它上面。只装不钉的话，自动挑选下一次仍然
      // 会挑回原来那个版本——按钮看起来生效了，游戏还是崩在同一句话上。
      if (!inTauri()) return
      await invoke('use_java', {
        instanceId,
        major: action.major,
        title: `换用 Java ${action.major}`,
        subjects: [instanceId],
      })
      await instances.load()
      // 换 Java 会让预检查里那几句话过期：它们说的都是「这个实例会用 Java 17」。
      preflight.refresh(instanceId)
      return
    }
    case 'restore-mods': {
      // 只恢复 mods，存档不动——按下这颗按钮的人是在找退路，不该因此丢进度。
      // replace 会删掉快照里没有的文件，那正是「多出来的东西」要的处理。
      if (!inTauri()) return
      await restoreSnapshot(instanceId, action.snapshot, { kind: 'mods' }, { kind: 'replace' })
      await instances.load()
      return
    }
  }
}
