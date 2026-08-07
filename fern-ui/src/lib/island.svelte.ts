/**
 * 岛：顶栏右段那块会变形的状态区。
 *
 * 借的是灵动岛的**交互语法**，不是它的样子。iPhone 上那块黑色药丸之所以成立，
 * 是因为那个位置本来就有一个挖孔——状态「住」在里面天经地义。Fern 没有挖孔，
 * 凭空浮一个纯黑胶囊只会像贴在封面上的一块创可贴。值得借的是四条：紧凑形态
 * 常驻、交互时连续变形而不是弹出新窗口、多状态时分裂成主胶囊加卫星、零状态时
 * 彻底消失。最后一条和「出厂零挂件」是同一个哲学。
 *
 * **岛只读，不存。** 这一层一份状态都不新增，它是一个投影：把散落在各处的现有
 * 状态映成同一个形状，排个序。没有注册／注销的生命周期，也就没有「作业结束了
 * 但岛没收到」这类第二真相来源的 bug——因为根本没有第二份真相。
 *
 * 各特性在自己的模块里调 `contributes()` 报告自己。加一种新的状态（联机房间）
 * 就是多一行 `contributes()`，**顶栏和岛的组件一个字都不用改**——这是这一层
 * 唯一要证明的事。
 *
 * 什么能进岛，一条判据：**只承载进行时。** 说不出「正在……」的东西不进——
 * 保存成功、复制了邀请码、有新版本可用，那些是 toast、是局部状态、是设置页里
 * 的一行字。岛是「我发起的进程住在那里」，不是通知中心。
 */

/** 展开后列出来的一条。三种状态的面板长得都是这个形状。 */
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
  /**
   * work 在干活、live 活着、alert 要人管。只决定颜色和字形，不决定内容。
   */
  tone: 'work' | 'live' | 'alert'
  /** 紧凑形态里的那一行字。 */
  label: string
  fraction?: number
  /**
   * 0–1。在胶囊底边画一条细线。
   *
   * 和 `fraction` 是两件事：那个是进度，会走到头、会「完成」，所以紧凑那一行
   * 会把它读成百分比；这个是**水位**——游戏的堆用了多少，它一直在动，永远
   * 不会完成。用同一个字段表示，岛就会开始报告一个不存在的进度。
   */
  fill?: number
  rows: PresenceRow[]
  actions: PresenceAction[]
}

/**
 * 谁排在前面。
 *
 * 失败排在最前，尽管它不是「最新」也不是「最热闹」的那个：前三种都是在发生的
 * 事，看不看都不影响你；失败是唯一需要你做点什么的。游戏跑着的时候整合包装崩
 * 了，岛该说那个坏消息，而不是继续报告一件你本来就知道的事。
 */
export const PRIORITY = { alert: 0, live: 10, room: 20, work: 30 }

const sources: Array<() => Presence[]> = []

/** 报告自己现在有什么可说的。返回空数组就是没有——那时岛不存在。 */
export function contributes(source: () => Presence[]) {
  sources.push(source)
}

export const island = {
  get all(): Presence[] {
    return sources.flatMap((read) => read()).sort((left, right) => left.priority - right.priority)
  },
  /** 主胶囊显示优先级最高的那个。 */
  get main(): Presence | undefined {
    return this.all[0]
  },
  /** 其余的分裂成左侧的小圆点，只有字形没有文字。 */
  get satellites(): Presence[] {
    return this.all.slice(1)
  },
}
