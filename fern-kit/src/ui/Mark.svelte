<script lang="ts">
  /**
   * Fern 的标志，同时是这个启动器唯一的加载指示。
   *
   * 三种状态用同一段几何（见 ./mark.ts）：
   *
   *   静态    就是标志本身。
   *   不定    沿走线依次点亮，一圈一圈画过去。方向恒定为茎底 → 芽尖。
   *   有进度  按 33 格的粒度点亮，粒度是刻意的——像素感的顿挫比一条平滑的
   *           进度条更像这个标志自己在生长。**螺线画完即完成。**
   *
   * 不用转圈的 spinner：一个通用 spinner 在哪个应用里都长一样，而这条走线
   * 只属于 Fern。加载是这个界面上出现频率最高的状态，把它交给标志，等于让
   * 品牌在每一次等待里出现一次。
   */
  import { CELLS, CELLS_COMPACT, layout } from './mark'
  import { host } from '../host.svelte'

  interface Props {
    size?: number
    /** 不定进度：沿走线跑一遍。 */
    spinning?: boolean
    /** 0–1。给了就是有进度，粒度为格数。 */
    progress?: number
    /** 四周留白，占 box 的比例。小尺寸下收掉。 */
    pad?: number
    /**
     * 默认继承 currentColor——它跟着所在位置的文字色走，不自带颜色。给这个
     * 参数只是省掉一层「专门用来染色的 span」，不改变那条原则。
     */
    color?: string
    label?: string
  }

  let { size = 20, spinning = false, progress, pad, color = 'currentColor', label }: Props = $props()

  /** 16px 以下用茎短一格的精确版，否则那一格会糊成两像素。 */
  const cells = $derived(size <= 18 ? CELLS_COMPACT : CELLS)
  const rects = $derived(layout(cells, 100, pad ?? (size <= 18 ? 2 : 8)))
  /** 一圈跑完的时长。动效关掉时不再跑，静止在半亮。 */
  const cycle = $derived(1800 * (host.motionScale || 1))
  const lit = $derived(progress === undefined ? -1 : Math.round(progress * rects.length))
</script>

<svg
  viewBox="0 0 100 100"
  width={size}
  height={size}
  shape-rendering="crispEdges"
  class:spin={spinning && host.motionScale > 0}
  class:lit={lit >= 0}
  style:--cycle={`${cycle}ms`}
  role={label ? 'img' : 'presentation'}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  {#each rects as rect, index}
    <rect
      x={rect.x.toFixed(3)}
      y={rect.y.toFixed(3)}
      width={rect.size.toFixed(3)}
      height={rect.size.toFixed(3)}
      fill={color}
      style:animation-delay={spinning ? `${(index / rects.length) * cycle}ms` : undefined}
      style:opacity={lit < 0 ? undefined : index < lit ? 1 : 0.14}
    />
  {/each}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }

  /*
   * 未点亮的格子留 14% 示轨——走线本身要看得见，否则亮点像是在虚空里游。
   */
  .spin rect {
    opacity: 0.14;
    animation: fern-pulse var(--cycle) linear infinite;
  }

  /*
   * 进度是连续变化的（下载、滚动），一格一格硬切会闪。给一道短过渡，
   * 长度跟着宿主的动效档位走——--motion 没定义的地方（比如官网）就是 1。
   */
  .lit rect {
    transition: opacity calc(180ms * var(--motion, 1)) linear;
  }

  @keyframes fern-pulse {
    0% {
      opacity: 0.14;
    }
    6% {
      opacity: 1;
    }
    30% {
      opacity: 0.14;
    }
    100% {
      opacity: 0.14;
    }
  }

  /* 系统级的减弱动效也要认。设置里的档位由 host.motionScale 管，这条管系统。 */
  @media (prefers-reduced-motion: reduce) {
    .spin rect {
      animation: none;
      opacity: 0.6;
    }
    .lit rect {
      transition: none;
    }
  }
</style>
