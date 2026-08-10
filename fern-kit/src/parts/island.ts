/**
 * 岛上说的话。
 *
 * 岛不认识作业、不认识游戏、不认识联机房间——只认识 `Presence`。谁想在那里说话，
 * 就交出一个；产品那边由 `island.svelte.ts` 收集各个模块的贡献并排序，官网直接写
 * 几个。两边喂给 `Island` 的是同一种东西。
 */

export interface PresenceRow {
  id: string
  label: string
  /** 人话：在做什么、第几步。 */
  detail?: string
  /** 机器数：字节、速度、时长。等宽显示，看不看都不影响操作。 */
  meta?: string
  /** 0–1。没有就画不定量的样子，不假装知道到了百分之几。 */
  fraction?: number
  /** 有就画一个关掉的按钮。失败的东西要能被点掉。 */
  dismiss?: () => void
}

export interface PresenceAction {
  label: string
  run: () => void
}

export interface Presence {
  id: string
  priority: number
  /** work 在干活、live 活着、alert 要人管。只决定颜色和字形，不决定内容。 */
  tone: 'work' | 'live' | 'alert'
  /** 紧凑形态里的那一行字。 */
  label: string
  fraction?: number
  /**
   * 0–1。在胶囊底边画一条细线。
   *
   * 和 `fraction` 是两件事：那个是进度，会走到头、会「完成」，所以紧凑那一行会把
   * 它读成百分比；这个是**水位**——游戏的堆用了多少，它一直在动，永远不会完成。
   * 用同一个字段表示，岛就会开始报告一个不存在的进度。
   */
  fill?: number
  rows: PresenceRow[]
  actions: PresenceAction[]
}

/**
 * 谁排在前面。
 *
 * 失败排在最前，尽管它不是「最新」也不是「最热闹」的那个：前三种都是在发生的事，
 * 看不看都不影响你；失败是唯一需要你做点什么的。游戏跑着的时候整合包装崩了，岛该
 * 说那个坏消息，而不是继续报告一件你本来就知道的事。
 */
export const PRIORITY = { alert: 0, live: 10, room: 20, work: 30 }
