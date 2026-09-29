/**
 * 动效的统一出口。
 *
 * 时长和曲线在 styles/scale.css 里，但 Svelte 的过渡要的是数字，读不了 CSS 变量。
 * 所以这里放一份，并且**全部乘以 `host.motionScale`**——「减弱动效」和「关闭动效」
 * 是启动器设置里真的开关，散在各个组件里写死的 200ms 会绕过它。官网没有那个开关，
 * 壳的默认值是 1。
 *
 * 三条纪律（对应 docs/frond-design-system.md）：
 *
 * 1. **转场压在 200ms 以内。** 标杆是 PS5 的场景切换速度。超过这个数，导航就从
 *    「瞬间到了」变成「在等它过去」。
 * 2. **纵深和横移用不同的动作。** 场景之间是横向平移，推入详情是就地展开；两种
 *    导航共用一种动画，用户就分不清自己是换了地方还是往深处走了。
 * 3. **列表进场只做一次，且只做前几项。** 每一行都飞一下，滚动到第两百行时就成
 *    了噪音。
 */

import { slide } from 'svelte/transition'
import { host } from './host.svelte'

/** 乘上「减弱/关闭动效」的档位。关掉时返回 0，过渡自己会退化成瞬时。 */
export const scaled = (ms: number) => Math.round(ms * host.motionScale)

export const DURATION = {
  /** 悬停、按下这类即时反馈。 */
  fast: 130,
  /** 场景横移、tab 切换。硬指标 200ms 以内。 */
  base: 190,
  /** 纵深展开、横幅收缩。 */
  deep: 300,
}

/*
 * ── 成对的动作 ──
 *
 * 下面的每一个都同时是进场和离场：Svelte 调用时第三个参数会说是哪一边。离场用
 * `fast`、进场用 `base`，两者同时开始，不排队——旧的先让开，新的慢慢站稳，整次
 * 切换的体感长度就是进场那一段，仍在 200ms 以内。这是「前快后长」那条曲线落
 * 在一对动作上的样子。
 *
 * 没有离场的地方，旧的一屏是瞬间消失的：前进有纵深，后退是硬切。
 */

type Side = { direction?: 'in' | 'out' | 'both' }

/** 出现、消失的那些临时的东西：报错、条件、读完的数字、提示。 */
export function appear(_node: Element, _params: unknown = {}, side: Side = {}) {
  if (side.direction === 'out') {
    return {
      duration: scaled(DURATION.fast),
      css: (t: number) => `opacity: ${t};`,
    }
  }
  return {
    duration: scaled(DURATION.base),
    easing: easeOut,
    css: (t: number, u: number) => `opacity: ${t}; transform: translateY(${u * 4}px);`,
  }
}

/**
 * 从触发它的那条边长出来的浮层：菜单、下拉。
 *
 * `from` 是它挂在触发器的哪一侧——向下展开的从上沿长，向上的从下沿长。进出都
 * 用 `fast`：浮层是随手开随手关的，慢一点就像在挡路。
 */
export function popover(
  node: Element,
  { from = 'top' }: { from?: 'top' | 'bottom' } = {},
  side: Side = {},
) {
  ;(node as HTMLElement).style.transformOrigin = from === 'top' ? 'top center' : 'bottom center'
  const shift = from === 'top' ? -4 : 4
  return {
    duration: scaled(DURATION.fast),
    easing: side.direction === 'out' ? undefined : easeOut,
    css: (t: number, u: number) =>
      `opacity: ${t}; transform: translateY(${u * shift}px) scale(${0.97 + t * 0.03});`,
  }
}

/**
 * 就地撑开、收起一段内容：下拉的版本列表、报错、增减的一行。
 *
 * 动的是高度，下面的东西跟着让位——用户看得出这一段是从这里长出来的，而不是
 * 盖在上面；删掉一行时，下面的行是滑上来的，不是跳上来的。
 */
export function unfold(node: Element, { duration = DURATION.base } = {}) {
  return slide(node, { duration: scaled(duration), easing: easeOut })
}

/** 和 scale.css 里 `--ease` 同一条：前段快、尾巴长。 */
function easeOut(t: number) {
  return 1 - (1 - t) ** 3
}
