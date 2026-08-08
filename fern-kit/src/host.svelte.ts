/**
 * 宿主往 kit 里装的东西。
 *
 * kit 的准入规则是「不依赖任何一端的运行环境」，但有两样东西组件确实需要，
 * 而它们只有宿主知道：
 *
 *   动效倍率  「减弱/关闭动效」是启动器设置里真的开关。CSS 那边有 --motion
 *             变量可以读，Svelte 的过渡要的却是数字，读不了变量。
 *   离屏画笔  群系图交给常驻 Worker 画，画完零拷贝转移回来。Worker 是产品
 *             那边的基建，官网没有，也不需要有。
 *
 * 所以这里放一个空壳：两样都有能用的默认值（不减速、主线程画），宿主想接
 * 就在启动时接上去。组件只认这个壳，不认谁装的——这样同一个组件既能落在
 * 启动器里，也能落在一张静态页面上。
 */

import type { BiomeOptions } from './biome'

export type OffscreenPainter = (
  width: number,
  height: number,
  options: BiomeOptions,
  phase: number,
  quality: number,
) => Promise<ImageBitmap>

class Host {
  #motion = $state(1)

  /** JS 动画的时长倍率。关掉动效时是 0，过渡自己会退化成瞬时。 */
  get motionScale() {
    return this.#motion
  }
  set motionScale(value: number) {
    this.#motion = value
  }

  /**
   * 有就用它画，没有就退回主线程同步画。
   *
   * 不是 $state：启动时装一次，之后不会变；组件第一次画之前装好就行。
   */
  paintOffscreen: OffscreenPainter | null = null
}

export const host = new Host()
