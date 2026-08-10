/**
 * 动效的统一出口。
 *
 * 时长和曲线都在 tokens.css 里，但 Svelte 的过渡要的是数字，读不了 CSS 变量。
 * 所以这里放一份，并且**全部乘以 theme.motionScale**——「减弱动效」和「关闭
 * 动效」是设置里真的开关，散在各个组件里写死的 200ms 会绕过它。
 *
 * 三条纪律（对应 docs/UI_DESIGN.md 七）：
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
export { DURATION, scaled } from 'fern-kit/motion'
import { DURATION, scaled } from 'fern-kit/motion'

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
