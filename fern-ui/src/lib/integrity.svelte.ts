/**
 * 这些文件还是上次那些吗。
 *
 * 后端把 mods、资源包、光影里每个文件的 sha1 和上次记下来的比一遍。**绝大多数
 * 时候它什么都不说**——没有话说就是空列表，界面上不该出现任何东西。
 *
 * 和预检查分开是有意的：那边回答「这样点下去会不会起不来」，这边回答「这些文
 * 件还是上次那些吗」。两件事不在一条轴上，所以这里没有 severity——该用什么分量
 * 呈现由是哪一条 `kind` 决定，程度由 `args` 里的数字说话。
 *
 * 调它的成本很低：没有变化时是一遍 stat，毫秒级。彻底的那一遍在游戏退出之后
 * 由后端自己跑，不占用户的时间。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri } from './instances.svelte'
import { describe } from './i18n'
import type { FixAction } from './advice'

export interface Notice {
  id: string
  /** 文案 id 的后半段：`integrity.<kind>`。 */
  kind: string
  args: Record<string, string>
  action?: FixAction
}

/** 一条已经翻成句子的对账结果。 */
export interface Reading extends Notice {
  title: string
  detail: string
  tone: 'blocking' | 'warning'
}

/**
 * 每一条用多重的分量呈现。
 *
 * 后端不给严重程度：预检查那条 `Severity` 轴说的是「点下去会不会起不来」，
 * 而一批 jar 被改写完全不影响游戏能否启动。轻重由是哪一条决定，就写在这里。
 *
 * 前两条重：一次是「几十个文件被同时改写」，一次是「记录本身不可信」——这两
 * 句都该在点启动之前看到。后两条轻：单个文件的变化，用户多半自己就知道原因。
 */
const TONE: Record<string, 'blocking' | 'warning'> = {
  'rewritten-together': 'blocking',
  'ledger-broken': 'blocking',
  'left-upstream': 'warning',
  'silent-rewrite': 'warning',
}

class IntegrityStore {
  #byInstance = $state<Record<string, Reading[]>>({})
  #checking = $state<Record<string, boolean>>({})

  /** 这个实例现在有什么要说的。没查过就是空的。 */
  for(instanceId: string): Reading[] {
    return this.#byInstance[instanceId] ?? []
  }

  checking(instanceId: string) {
    return this.#checking[instanceId] === true
  }

  /** 查一次。已经查过就直接用缓存，除非 `force`。 */
  async check(instanceId: string, force = false) {
    if (!inTauri() || !instanceId) return
    if (!force && this.#byInstance[instanceId] !== undefined) return
    if (this.#checking[instanceId]) return
    this.#checking[instanceId] = true
    try {
      const found = await invoke<Notice[]>('integrity', { instanceId })
      this.#byInstance[instanceId] = found.map((notice) => ({
        ...notice,
        ...describe(`integrity.${notice.kind}`, notice.args),
        tone: TONE[notice.kind] ?? 'warning',
      }))
    } catch {
      // 查不出来不该变成一条错误横在界面上。离线时上游那条信号本来就查不了，
      // 那是少一条依据，不是多一条问题。
      this.#byInstance[instanceId] = []
    } finally {
      this.#checking[instanceId] = false
    }
  }

  /** 模组变了，之前那一份不作数了。 */
  refresh(instanceId: string) {
    void this.check(instanceId, true)
  }
}

export const integrity = new IntegrityStore()
