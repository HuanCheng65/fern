/**
 * 启动之前能看出来的问题。
 *
 * 后端读一遍 mods 目录里那些 jar 的元数据，几百毫秒，不联网。它回答的是「按
 * 现在这样点下去会不会出事」——缺前置、同一个模组装了两份、模组不是给这个
 * 加载器的。这一类失败原本的表现是「点了启动，黑框一闪，没了」。
 *
 * **它不拦启动。** 结果摆在启动键旁边，按不按由用户定——他可能比我们清楚。
 *
 * 结果按实例缓存：翻回同一个实例不该重扫一遍。装了或删了模组之后调用
 * `refresh` 作废那一份。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri } from './instances.svelte'
import { describe } from './i18n'
import type { FixAction } from './advice'

export type Severity = 'blocking' | 'warning'

export interface Finding {
  id: string
  /** 文案 id 的后半段：`preflight.<kind>`。 */
  kind: string
  severity: Severity
  args: Record<string, string>
  action?: FixAction
}

/** 一条已经翻成句子的检查结果。 */
export interface Advisory extends Finding {
  title: string
  detail: string
}

class PreflightStore {
  #byInstance = $state<Record<string, Advisory[]>>({})
  #checking = $state<Record<string, boolean>>({})

  /** 这个实例现在已知的问题。没查过就是空的。 */
  for(instanceId: string): Advisory[] {
    return this.#byInstance[instanceId] ?? []
  }

  checking(instanceId: string) {
    return this.#checking[instanceId] === true
  }

  /** 大概率起不来的那些。启动键旁边只说这些。 */
  blocking(instanceId: string) {
    return this.for(instanceId).filter((item) => item.severity === 'blocking')
  }

  /** 查一次。已经查过就直接用缓存，除非 `force`。 */
  async check(instanceId: string, force = false) {
    if (!inTauri() || !instanceId) return
    if (!force && this.#byInstance[instanceId] !== undefined) return
    if (this.#checking[instanceId]) return
    this.#checking[instanceId] = true
    try {
      const found = await invoke<Finding[]>('preflight', { instanceId })
      this.#byInstance[instanceId] = found.map((finding) => ({
        ...finding,
        ...describe(`preflight.${finding.kind}`, finding.args),
      }))
    } catch {
      // 查不出来不该变成一条错误横在界面上：这一层是锦上添花，不是必经之路。
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

export const preflight = new PreflightStore()
