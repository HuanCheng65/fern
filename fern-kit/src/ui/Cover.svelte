<script lang="ts">
  /**
   * 实例封面（见 docs/frond-design-system.md）。
   *
   * 和背景用的是同一套生成器：种子决定群系和构图，累积时长决定构图密度。区别
   * 在后处理——封面按方块采样，每一块量化到群系那五个颜色，再撒一点块内的颗
   * 粒。平滑的渐变是桌面壁纸的气质，方块和硬边才是这个游戏的。像素只在画里，
   * 控件和字照旧是平滑的。
   *
   * 封面是实例的脸，所以它不跟着时间变：色温只进背景，不进封面。量化到群系的
   * 原色本身就和钟点无关，`hour` 只是让采样那一步有个固定的参照。
   *
   * 尺寸默认从 CSS 拿（元素多大就画多大），所以同一个组件能当 32px 的行内
   * 缩略图，也能当一张大卡片；给了 w/h 就按那个尺寸摆一张固定的。横向永远是
   * 16 块，纵向跟着比例走——缩略图和大图是同一张画，只是放大倍数不同。
   */
  import { onMount } from 'svelte'

  interface Props {
    /** 恒定种子。实例用它自己的 cover 标识——改名不换脸。 */
    seed: string
    /** 建实例时挑定的群系。不给就按种子算。 */
    biome?: BiomeKey
    /** 生长种子：累积游玩小时数。 */
    hours?: number
    /** 采样用的钟点。量化之后几乎不影响结果，留着只是给官网对齐讲解用的图。 */
    hour?: number
    /** 固定尺寸（px）。不给就铺满所在的盒子。 */
    w?: number
    h?: number
    class?: string
  }

  let { seed, biome, hours = 0, hour = 13, w, h, class: className = '' }: Props = $props()

  let canvas = $state<HTMLCanvasElement>()

  function render() {
    const cv = canvas
    if (!cv) return
    const rect = cv.getBoundingClientRect()
    if (rect.width < 2 || rect.height < 2) return
    const rows = Math.max(1, Math.round((COLS * rect.height) / rect.width))
    const image = pixels(seed, biome, hours, hour, rows)
    if (cv.width !== image.width || cv.height !== image.height) {
      cv.width = image.width
      cv.height = image.height
    }
    cv.getContext('2d')!.putImageData(image, 0, 0)
  }

  onMount(() => {
    // 元素比例变了就重画：同一个组件在列表里和别处是两种比例。
    const observer = new ResizeObserver(() => render())
    if (canvas) observer.observe(canvas)
    return () => observer.disconnect()
  })

  $effect(() => {
    void seed
    void biome
    void hours
    void hour
    render()
  })
</script>

<script module lang="ts">
  import { BIOMES, paint, type BiomeKey, type RGB } from './biome'

  /** 横向的块数。 */
  const COLS = 16
  /** 每块里的颗粒数（边长）。块内的明暗起伏让它像一块方块，而不是一格色卡。 */
  const GRAIN = 4

  const hex = (value: string): RGB => [
    parseInt(value.slice(1, 3), 16),
    parseInt(value.slice(3, 5), 16),
    parseInt(value.slice(5, 7), 16),
  ]

  /**
   * 画好的封面按参数记下来。
   *
   * 补给那一屏每个没有图标的结果都会画一张，翻几页就是几百张——所以有上限，
   * 先进先出。一张只有 64×48 个点，重画也不贵，缓存只是省掉重复的那一次。
   */
  const cache = new Map<string, ImageData>()
  const CACHE_LIMIT = 200
  let sampler: HTMLCanvasElement | null = null

  function pixels(
    seed: string,
    biome: BiomeKey | undefined,
    hours: number,
    hour: number,
    rows: number,
  ): ImageData {
    const key = `${seed}|${biome ?? ''}|${Math.round(hours)}|${hour}|${rows}`
    const hit = cache.get(key)
    if (hit) return hit

    const cols = COLS
    const grain = GRAIN
    sampler ??= document.createElement('canvas')
    sampler.width = cols
    sampler.height = rows
    const { r } = paint(sampler, { name: seed, hours, hour, biomeKey: biome ?? '' }, 0, 1)
    const small = sampler.getContext('2d')!.getImageData(0, 0, cols, rows).data
    const stops = BIOMES[r.bk].stops.map(hex)

    const out = new ImageData(cols * grain, rows * grain)
    const data = out.data
    // 颗粒用种子驱动的伪随机，同一张脸每次都是同一片颗粒。
    let state = r.seed || 1
    const noise = () => {
      state = (Math.imul(state, 1664525) + 1013904223) >>> 0
      return state / 4294967296 - 0.5
    }
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < cols; x++) {
        const i = (y * cols + x) * 4
        let base = stops[0]!
        let best = Infinity
        for (const stop of stops) {
          const d =
            (stop[0] - small[i]!) ** 2 + (stop[1] - small[i + 1]!) ** 2 + (stop[2] - small[i + 2]!) ** 2
          if (d < best) {
            best = d
            base = stop
          }
        }
        for (let gy = 0; gy < grain; gy++) {
          for (let gx = 0; gx < grain; gx++) {
            const j = noise() * 16
            const o = ((y * grain + gy) * cols * grain + x * grain + gx) * 4
            data[o] = base[0] + j
            data[o + 1] = base[1] + j
            data[o + 2] = base[2] + j
            data[o + 3] = 255
          }
        }
      }
    }

    if (cache.size >= CACHE_LIMIT) cache.delete(cache.keys().next().value!)
    cache.set(key, out)
    return out
  }
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
    /* 放大时保持硬边。块是画出来的，不是缩放的副产品。 */
    image-rendering: pixelated;
    /* 生成的场边缘偏暗，压一道内描边把它和背景分开，不用外框。 */
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
  }
</style>
