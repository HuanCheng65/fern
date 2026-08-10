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
export type { Presence, PresenceAction, PresenceRow } from 'fern-kit/parts/island'
export { PRIORITY } from 'fern-kit/parts/island'

import type { Presence } from 'fern-kit/parts/island'

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
