<script lang="ts">
  /**
   * 实例封面（见 docs/UI_DESIGN.md 二「群系」）。
   *
   * 和背景用的是同一套生成器，只是画在一张小画布上：实例名的哈希决定群系
   * 和构图，累积时长决定构图密度，当前小时决定色温。所以列表里每一行的色块
   * 不是随便挑的四个颜色，而是这个实例的脸——同一个实例在哪里出现都长一样，
   * 换个实例就是另一张画。
   *
   * 尺寸默认从 CSS 拿（元素多大就画多大），所以同一个组件能当 32px 的行内
   * 缩略图，也能当详情页顶上的整条封面；给了 w/h 就按那个尺寸摆一张固定的，
   * 排版里不需要再套一层盒子。
   */
  import { onMount } from 'svelte'
  import { paint } from './biome'
  import { host } from '../host.svelte'

  interface Props {
    /** 恒定种子。实例用实例名——名字就是脸。 */
    seed: string
    /** 生长种子：累积游玩小时数。 */
    hours?: number
    /** 环境种子：0–24，决定色温。不给就是此刻。 */
    hour?: number
    /** 场的分辨率倍率。缩略图不需要高质量，整条封面值得多算一点。 */
    quality?: number
    /** 固定尺寸（px）。不给就铺满所在的盒子。 */
    w?: number
    h?: number
    class?: string
  }

  let { seed, hours = 0, hour, quality = 0.55, w, h, class: className = '' }: Props = $props()

  let canvas = $state<HTMLCanvasElement>()
  let token = 0

  async function render() {
    const cv = canvas
    if (!cv) return
    const rect = cv.getBoundingClientRect()
    if (rect.width < 2 || rect.height < 2) return
    // 上限 2 倍：色块没有细节，再高的 DPR 只是多画像素。
    const dpr = Math.min(2, window.devicePixelRatio || 1)
    const width = Math.round(rect.width * dpr)
    const height = Math.round(rect.height * dpr)
    if (cv.width !== width || cv.height !== height) {
      cv.width = width
      cv.height = height
    }

    const mine = ++token
    const options = { name: seed, hours, hour }
    if (host.paintOffscreen) {
      try {
        const bitmap = await host.paintOffscreen(width, height, options, 0, quality)
        if (mine !== token || !canvas) return bitmap.close()
        const ctx = canvas.getContext('2d')!
        ctx.clearRect(0, 0, width, height)
        ctx.drawImage(bitmap, 0, 0)
        bitmap.close()
        return
      } catch {
        // 离屏画笔起不来就在主线程画，小图代价可以接受。
      }
    }
    if (mine !== token || !canvas) return
    paint(canvas, options, 0, quality)
  }

  onMount(() => {
    // 元素尺寸变了就重画：同一个组件在列表里和详情页里是两种尺寸。
    const observer = new ResizeObserver(() => void render())
    if (canvas) observer.observe(canvas)
    return () => observer.disconnect()
  })

  $effect(() => {
    void seed
    void hours
    void hour
    void render()
  })
</script>

<canvas
  bind:this={canvas}
  class={className}
  style:width={w === undefined ? undefined : `${w}px`}
  style:height={h === undefined ? undefined : `${h}px`}
  aria-hidden="true"
></canvas>

<style>
  canvas {
    display: block;
    width: 100%;
    height: 100%;
    flex: none;
    /* 生成的场边缘偏暗，压一道内描边把它和背景分开，不用外框。 */
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
  }
</style>
