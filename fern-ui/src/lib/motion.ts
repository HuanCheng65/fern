/**
 * 动效的统一出口。
 *
 * 时长和曲线都在 tokens.css 里，但 Svelte 的过渡要的是数字，读不了 CSS 变量。
 * 所以这里放一份，并且**全部乘以 theme.motionScale**——「减弱动效」和「关闭
 * 动效」是设置里真的开关，散在各个组件里写死的 200ms 会绕过它。
 *
 * 三条纪律（对应 docs/frond-design-system.md）：
 *
 * 1. **转场压在 200ms 以内。** 标杆是 PS5 的场景切换速度。超过这个数，导航
 *    就从「瞬间到了」变成「在等它过去」。
 * 2. **纵深和横移用不同的动作。** 场景之间是横向平移，推入详情是就地展开；
 *    两种导航共用一种动画，用户就分不清自己是换了地方还是往深处走了。
 * 3. **列表进场只做一次，且只做前几项。** 每一行都飞一下，滚动到第两百行时
 *    就成了噪音。
 */

import { cubicOut } from 'svelte/easing'
import type { TransitionConfig } from 'svelte/transition'

// 档位和乘数在 kit（它读 host.motionScale，而 theme 启动时就把档位装了进去）。
// 两边各写一份必然会分叉，而分叉的那天没人会发现。
export { DURATION, appear, popover, scaled, unfold } from 'fern-kit/motion'
import { DURATION, scaled } from 'fern-kit/motion'

type Side = { direction?: 'in' | 'out' | 'both' }

/**
 * 就地展开。用于「往深处走」——详情从它在列表里的位置长出来。
 *
 * 和场景横移分开：那个是 x 位移，这个是纵向的舒展加一点放大，两种导航在
 * 体感上必须是两个动作。
 */
export function expand(_node: Element, { duration = DURATION.deep } = {}): TransitionConfig {
  const total = scaled(duration)
  return {
    duration: total,
    easing: cubicOut,
    css: (t, u) => `opacity: ${t}; transform: translateY(${u * 10}px) scale(${0.985 + t * 0.015});`,
  }
}

/**
 * 弹一下。只给那种「刚刚生效」的小标记——选中的对勾。
 *
 * 起点不是 0：从 0 放大像是被扔进来的，从 0.7 起才像是被按出来的。
 */
export function pop(_node: Element, { duration = DURATION.fast } = {}): TransitionConfig {
  return {
    duration: scaled(duration),
    easing: cubicOut,
    css: (t) => `opacity: ${t}; transform: scale(${0.7 + t * 0.3});`,
  }
}

/**
 * 列表逐项进场。
 *
 * `index` 决定延迟，但延迟有上限——第 30 行之后一律不再等，否则长列表的尾巴
 * 要几秒才铺完。
 */
export function riseIn(
  _node: Element,
  { index = 0, duration = DURATION.base }: { index?: number; duration?: number } = {},
): TransitionConfig {
  const total = scaled(duration)
  return {
    delay: Math.min(index, 12) * scaled(18),
    duration: total,
    easing: cubicOut,
    css: (t, u) => `opacity: ${t}; transform: translateY(${u * 8}px);`,
  }
}

/**
 * 横移：换场景、切 tab。
 *
 * `direction` 是镜头往哪边走（1 往右）。新的从那一侧滑进来，旧的朝反方向让开，
 * 让得比进得少、比进得快——视线跟着新的走。`distance` 给场景 26、给 tab 12：
 * tab 换的是同一个东西里的一段，位移小一个量级。
 */
export function lateral(
  _node: Element,
  { direction = 1, distance = 26 }: { direction?: number; distance?: number } = {},
  side: Side = {},
): TransitionConfig {
  if (side.direction === 'out') {
    return {
      duration: scaled(DURATION.fast),
      css: (t, u) => `opacity: ${t}; transform: translateX(${-direction * u * distance * 0.5}px);`,
    }
  }
  return {
    duration: scaled(DURATION.base),
    easing: cubicOut,
    css: (t, u) => `opacity: ${t}; transform: translateX(${direction * u * distance}px);`,
  }
}

/**
 * 纵深：列表和详情之间。
 *
 * 一层是「外面」（列表），一层是「里面」（详情）。`sink` 是这次往哪走
 * （`nav.sink`）：
 *
 *   往里   里面那层就地展开浮上来，外面那层稍稍退后、淡掉
 *   往外   里面那层往下沉着淡掉，外面那层从略微放大的位置落回原处
 *
 * 两个方向是同一个空间的来回，不是同一段动画倒放。`sink` 为 0（换场景、刷新
 * 落地）时这一层不动，让横移独自完成。
 */
export function depth(
  _node: Element,
  { inner = false, sink = 0 }: { inner?: boolean; sink?: number } = {},
  side: Side = {},
): TransitionConfig {
  if (sink === 0) return { duration: 0 }
  const leaving = side.direction === 'out'
  const fast = scaled(DURATION.fast)
  const base = scaled(DURATION.base)
  if (inner) {
    return leaving
      ? {
          duration: fast,
          css: (t, u) =>
            `opacity: ${t}; transform: translateY(${u * 8}px) scale(${0.985 + t * 0.015});`,
        }
      : {
          duration: base,
          easing: cubicOut,
          css: (t, u) =>
            `opacity: ${t}; transform: translateY(${u * 10}px) scale(${0.985 + t * 0.015});`,
        }
  }
  return leaving
    ? { duration: fast, css: (t) => `opacity: ${t}; transform: scale(${0.99 + t * 0.01});` }
    : {
        duration: base,
        easing: cubicOut,
        css: (t, u) => `opacity: ${t}; transform: scale(${1 + u * 0.01});`,
      }
}
