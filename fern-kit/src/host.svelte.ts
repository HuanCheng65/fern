/**
 * 宿主往 kit 里装的东西。
 *
 * kit 的准入规则是「不依赖任何一端的运行环境」，但有一样东西组件确实需要，
 * 而它只有宿主知道：
 *
 *   动效倍率  「减弱/关闭动效」是启动器设置里真的开关。CSS 那边有 --motion
 *             变量可以读，Svelte 的过渡要的却是数字，读不了变量。
 *
 * （这里曾经还有一支离屏画笔，把封面交给 Worker 画。封面改成方块之后只要算
 * 十几乘十几个点，主线程画比一次消息往返还快，它就没有了。）
 *
 * 所以这里放一个空壳：有能用的默认值（不减速），宿主想接就在启动时接上去。组件只认这个壳，不认谁装的——这样同一个组件既能落在
 * 启动器里，也能落在一张静态页面上。
 */

class Host {
  #motion = $state(1)

  /** JS 动画的时长倍率。关掉动效时是 0，过渡自己会退化成瞬时。 */
  get motionScale() {
    return this.#motion
  }
  set motionScale(value: number) {
    this.#motion = value
  }
}

export const host = new Host()
