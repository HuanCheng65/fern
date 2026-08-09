<script lang="ts">
  /**
   * 背景层，四层结构（见 docs/UI_DESIGN.md 六）。自下而上：
   *
   *   背景层  一块 WebGL 画布，场在片元着色器里逐帧实时算
   *   特效层  粒子、视差
   *   遮罩层  可读性暗角
   *   内容层  不在这里，由 App 铺在上面
   *
   * 场的数学搬到了 GPU（lib/biome-gl.ts），主线程不再算一次噪声：首次打开
   * 没有整屏 fbm 那一下卡顿，窗口缩放是免费的，呼吸是真正连续的相位推进。
   * 换房间、人数变化这类样式跳变也因此能做成连续动画：着色器常驻两套参数
   * 交叉淡化，色板跟着逐帧插值，整个界面的颜色一起滑过去而不是跳过去。
   *
   * 建不出 WebGL2 的环境退回 CPU 路径：能用 Worker 就在 Worker 里画
   * （主线程照样不卡），连 Worker 都没有才回到原来的同步 paint。CPU 路径
   * 保留旧的双画布交叉淡入呼吸，样式变化不做过渡。
   *
   * 支点规则也在这里落地：每次配色确定就把色板写进 --c0..--c4，界面其余
   * 部分全部向它取色。
   */
  import { onMount } from 'svelte'
  import {
    envAt,
    fieldRange,
    grainDataUrl,
    paint,
    paletteOf,
    resolve,
    stopsOf,
    type BiomeOptions,
    type Env,
    type RGB,
  } from 'fern-kit/ui/biome'
  import { renderBiome, supportsBiomeWorker } from '../lib/biome-client'
  import { scopeRoot } from 'fern-kit/scope'
  import { createBackdropGl, layerOf, type BackdropGl, type GlLayer } from '../lib/biome-gl'

  interface Props {
    /** 恒定种子。房间码是最自然的选择：一个房间一张脸。 */
    seed: string
    /** 生长种子，这里用连接上的人数——房间越热闹，构图越密。 */
    hours?: number
    /** 特效开关，跟着设置走。 */
    particles?: boolean
    parallax?: boolean
    /** 焦点不在舞台上时（面板展开等），背景下沉。 */
    away?: boolean
  }

  let { seed, hours = 0, particles = true, parallax = true, away = false }: Props = $props()

  let base = $state<HTMLCanvasElement>()
  let phaseCanvas = $state<HTMLCanvasElement>()
  let particleCanvas = $state<HTMLCanvasElement>()
  let grain = $state('')
  let move = $state<HTMLElement>()
  let paused = $state(false)
  let mode = $state<'gl' | 'cpu'>('cpu')

  const options = (): BiomeOptions => ({ name: seed, hours })

  /** 呼吸一轮的时长，与旧 CSS 动画一致。 */
  const BREATH_MS = 168_000
  /** 样式变化的交叉淡化时长。 */
  const TRANSITION_MS = 1_600
  /** 背景以约 30fps 推进就足够；它是环境不是内容。 */
  const FRAME_MS = 33

  /** 相位在 0–0.85 间往复——正是旧版两张图的两个端点，只是中间也存在了。 */
  const phaseAt = (t: number) => 0.425 * (1 - Math.cos((t / BREATH_MS) * 2 * Math.PI))
  const ease = (k: number) => k * k * (3 - 2 * k)
  const hourNow = () => {
    const d = new Date()
    return d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600
  }

  let gl: BackdropGl | null = null
  let from: GlLayer | undefined
  let to: GlLayer | undefined
  let blendStart = -Infinity
  let lastKey = ''

  // 写在挂着 .fern-dark 的那个元素上。写 :root 不生效，原因见 fern-kit/src/scope.ts。
  const root = scopeRoot

  /** 色板写进 token 根，界面其余部分全部向它取色。 */
  function setPalette(palette: RGB[]) {
    palette.forEach((c, i) =>
      root().style.setProperty(`--c${i}`, `rgb(${c.map(Math.round).join(',')})`),
    )
    // 支点规则的另一半：色板不光要交出颜色，还要交出压在强调色上的文字色。
    // --c4 在深色群系或深夜会沉下去，写死深色字就会在那些时候糊掉。
    const [cr, cg, cb] = palette[4]!
    const luminance = (0.2126 * cr + 0.7152 * cg + 0.0722 * cb) / 255
    root().style.setProperty('--on-accent', luminance > 0.55 ? '#0a0f12' : '#f2f5f5')
    root().style.setProperty(
      '--accent-glow',
      `rgba(${Math.round(cr)}, ${Math.round(cg)}, ${Math.round(cb)}, 0.3)`,
    )
  }

  /** GL 路径的色板：过渡期间按 blend 在两套之间插值，整个界面一起滑。 */
  function applyPalette(blend: number, env: Env) {
    if (!to) return
    let palette = paletteOf(to.stops, env, to.r.tMax)
    if (blend < 1 && from) {
      const before = paletteOf(from.stops, env, from.r.tMax)
      palette = palette.map(
        (c, i) => c.map((v, j) => before[i]![j]! + (v - before[i]![j]!) * blend) as RGB,
      )
    }
    setPalette(palette)
  }

  // 半分辨率再放大：这本来就是大色块，没有细节可损失。GPU 画全分辨率也不贵，
  // 但合成器缩放一张小图更省，视觉上与旧版一致。
  const scaledW = () => Math.round(window.innerWidth * 0.55)
  const scaledH = () => Math.round(window.innerHeight * 0.55)

  let raf = 0
  let lastFrame = 0
  let lastRange = 0
  let lastPalette = 0

  function frame(t: number) {
    raf = requestAnimationFrame(frame)
    if (paused || !gl || !from || !to) return
    if (t - lastFrame < FRAME_MS) return
    lastFrame = t

    const phase = phaseAt(t)
    const blend = ease(Math.min(1, (t - blendStart) / TRANSITION_MS))
    const env = envAt(hourNow())

    // 归一化区间跟着相位缓慢漂移。每隔几秒在小探针场上重量一次，低通并入，
    // 对比度就不会打台阶。
    if (blend >= 1 && t - lastRange > 2_500) {
      lastRange = t
      const range = fieldRange(to.r, phase)
      to = { ...to, lo: to.lo + (range.lo - to.lo) * 0.5, hi: to.hi + (range.hi - to.hi) * 0.5 }
    }

    gl.draw({ from, to, blend, phase, env })

    // 过渡期间色板逐帧走；稳态下只需要跟上昼夜的缓慢变化。
    if (blend < 1 || t - lastPalette > 5_000) {
      lastPalette = t
      applyPalette(blend, env)
    }
  }

  /** 样式目标变了：GL 路径开一段交叉淡化，CPU 路径直接重画。 */
  function apply() {
    const key = `${seed}|${hours}`
    if (key === lastKey) return
    lastKey = key
    if (gl) {
      const now = performance.now()
      const next = layerOf(options(), phaseAt(now))
      if (!to) {
        from = to = next
        blendStart = -Infinity
        applyPalette(1, envAt(hourNow()))
        return
      }
      if (next.r.seed === to.r.seed && next.r.g === to.r.g) return
      from = to
      to = next
      blendStart = now
    } else {
      void renderCpu()
    }
  }

  /** CPU 兜底：老的双画布路径，能用 Worker 就不占主线程。 */
  let cpuToken = 0
  async function renderCpu() {
    if (!base || !phaseCanvas) return
    const token = ++cpuToken
    const w = scaledW()
    const h = scaledH()
    if (base.width !== w || base.height !== h) {
      base.width = phaseCanvas.width = w
      base.height = phaseCanvas.height = h
    }
    const o = options()
    // 色板不依赖画完的像素，先交出去，界面不用等图。
    const r = resolve(o)
    setPalette(paletteOf(stopsOf(r.bk), r.env, r.tMax))

    if (supportsBiomeWorker) {
      const still = renderBiome(w, h, o, 0, 0.5)
      const breathing = renderBiome(w, h, o, 0.85, 0.42)
      try {
        const [a, b] = await Promise.all([still.promise, breathing.promise])
        if (token !== cpuToken || !base || !phaseCanvas) {
          a.close()
          b.close()
          return
        }
        blit(base, a)
        blit(phaseCanvas, b)
        return
      } catch {
        // Worker 起不来就退回同步画。
      }
    }
    if (token !== cpuToken) return
    paint(base, o, 0, 0.5)
    paint(phaseCanvas, o, 0.85, 0.42)
  }

  function blit(cv: HTMLCanvasElement, bitmap: ImageBitmap) {
    const ctx = cv.getContext('2d')!
    ctx.clearRect(0, 0, cv.width, cv.height)
    ctx.drawImage(bitmap, 0, 0, cv.width, cv.height)
    bitmap.close()
  }

  function startParticles() {
    if (!particleCanvas) return
    const cv = particleCanvas
    const ctx = cv.getContext('2d')!
    const resize = () => {
      cv.width = window.innerWidth
      cv.height = window.innerHeight
    }
    resize()
    window.addEventListener('resize', resize)

    const P = Array.from({ length: 34 }, () => ({
      x: Math.random(),
      y: Math.random(),
      r: 0.6 + Math.random() * 1.7,
      s: 0.00006 + Math.random() * 0.00018,
      o: 0.2 + Math.random() * 0.5,
    }))
    let particleRaf = 0
    let last = 0
    const particleFrame = (t: number) => {
      particleRaf = requestAnimationFrame(particleFrame)
      // 25fps 足够，粒子是环境不是内容。省下的帧留给别的。
      if (t - last < 40) return
      last = t
      ctx.clearRect(0, 0, cv.width, cv.height)
      if (!particles || paused) return
      ctx.fillStyle = getComputedStyle(root()).getPropertyValue('--c4').trim()
      for (const q of P) {
        q.y -= q.s * 1000
        if (q.y < -0.02) {
          q.y = 1.02
          q.x = Math.random()
        }
        ctx.globalAlpha = q.o * 0.5
        ctx.beginPath()
        ctx.arc(q.x * cv.width, q.y * cv.height, q.r, 0, 6.283)
        ctx.fill()
      }
      ctx.globalAlpha = 1
    }
    particleRaf = requestAnimationFrame(particleFrame)
    return () => {
      cancelAnimationFrame(particleRaf)
      window.removeEventListener('resize', resize)
    }
  }

  onMount(() => {
    grain = grainDataUrl()
    if (base) gl = createBackdropGl(base)
    if (gl) {
      mode = 'gl'
      gl.resize(scaledW(), scaledH())
      raf = requestAnimationFrame(frame)
    }
    apply()

    let resizeTimer: ReturnType<typeof setTimeout>
    const onResize = () => {
      if (gl) {
        // GPU 上重画一帧是免费的，不用防抖。
        gl.resize(scaledW(), scaledH())
        return
      }
      clearTimeout(resizeTimer)
      resizeTimer = setTimeout(() => void renderCpu(), 160)
    }
    window.addEventListener('resize', onResize)

    // 显卡上下文可能被系统收走。恢复事件来了就重建；在那之前背景停在最后
    // 一帧，比整层消失强。
    const onContextLost = (e: Event) => e.preventDefault()
    const onContextRestored = () => {
      if (!base) return
      gl = createBackdropGl(base)
      gl?.resize(scaledW(), scaledH())
    }
    base?.addEventListener('webglcontextlost', onContextLost)
    base?.addEventListener('webglcontextrestored', onContextRestored)

    // 视差：背景反向漂移几像素，产生纵深。前景漂得更少，这里只管背景。
    const onPointer = (e: PointerEvent) => {
      if (!parallax || !move || paused) return
      const dx = (e.clientX / window.innerWidth - 0.5) * -14
      const dy = (e.clientY / window.innerHeight - 0.5) * -10
      move.style.transform = `translate3d(${dx}px, ${dy}px, 0)`
    }
    window.addEventListener('pointermove', onPointer)

    // 性能红线：窗口失焦立刻暂停。
    const onVisibility = () => {
      paused = document.hidden || !document.hasFocus()
      document.body.classList.toggle('paused', paused)
    }
    document.addEventListener('visibilitychange', onVisibility)
    window.addEventListener('blur', onVisibility)
    window.addEventListener('focus', onVisibility)

    const stopParticles = startParticles()
    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener('resize', onResize)
      window.removeEventListener('pointermove', onPointer)
      document.removeEventListener('visibilitychange', onVisibility)
      window.removeEventListener('blur', onVisibility)
      window.removeEventListener('focus', onVisibility)
      base?.removeEventListener('webglcontextlost', onContextLost)
      base?.removeEventListener('webglcontextrestored', onContextRestored)
      stopParticles?.()
      gl?.dispose()
      gl = null
    }
  })

  // 种子或人数变了就是另一张画。GL 路径滑过去，CPU 路径重画。
  $effect(() => {
    void seed
    void hours
    apply()
  })
</script>

<div id="bg" class:away>
  <div id="bgMove" bind:this={move}>
    <div id="bgDrift">
      <canvas bind:this={base}></canvas>
      <!-- 仅 CPU 兜底路径使用：呼吸靠这张相位图交叉淡入。GL 路径的呼吸在
           着色器里连续推进，这一层整个不画。 -->
      <canvas id="bgPhase" bind:this={phaseCanvas} class:hidden={mode === 'gl'}></canvas>
    </div>
  </div>
  <div id="grain" style:background-image={grain ? `url(${grain})` : 'none'}></div>
  <div id="scrim"></div>
  <canvas id="particles" bind:this={particleCanvas} class:hidden={!particles}></canvas>
  <div id="veil"></div>
</div>

<style>
  #bg {
    position: fixed;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    background: var(--c0);
  }

  #bgMove {
    position: absolute;
    inset: -6%;
    transition:
      transform 560ms cubic-bezier(0.22, 1, 0.36, 1),
      filter var(--soft),
      opacity var(--soft);
    will-change: transform;
  }

  #bgMove canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* CPU 兜底的呼吸：168 秒一轮，慢到看不见它在动，只觉得画面是活的。 */
  #bgPhase {
    animation: breathe 168s ease-in-out infinite;
    opacity: 0;
  }

  #bgPhase.hidden {
    display: none;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 0;
    }
    50% {
      opacity: 0.55;
    }
  }

  #bgDrift {
    position: absolute;
    inset: 0;
    animation: drift 150s ease-in-out infinite;
    will-change: transform;
  }

  @keyframes drift {
    0%,
    100% {
      transform: translate3d(-7px, 3px, 0) scale(1);
    }
    50% {
      transform: translate3d(7px, -4px, 0) scale(1.025);
    }
  }

  #grain {
    position: absolute;
    inset: 0;
    opacity: 0.05;
    mix-blend-mode: overlay;
    background-size: 110px 110px;
    pointer-events: none;
  }

  /*
   * 遮罩层。文档里写的是「检测文字区域背后亮度，超标就淡入局部遮罩」，
   * 那需要采样画布，成本和复杂度都不低。这里先用固定的三道渐变兜底：
   * 顶栏和底部信息条的位置是定的，生成式色板的明度也是我们自己控的，
   * 所以最坏情况可以算得出来。真正的自适应遮罩等背景源支持图片和视频
   * 壁纸时再做——那时候才会有算不出来的亮度。
   */
  #scrim {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      linear-gradient(180deg, rgba(0, 0, 0, 0.42) 0%, transparent 26%),
      linear-gradient(0deg, rgba(0, 0, 0, 0.55) 0%, transparent 46%),
      radial-gradient(130% 95% at 50% 42%, transparent 55%, rgba(0, 0, 0, 0.34) 100%);
  }

  #particles {
    position: absolute;
    inset: 0;
    pointer-events: none;
    opacity: 0.55;
  }

  #particles.hidden {
    display: none;
  }

  /* 景深：焦点浮上来时背景模糊下沉。 */
  #veil {
    position: absolute;
    inset: 0;
    background: rgba(6, 9, 11, 0.62);
    opacity: 0;
    transition: opacity var(--pan);
    pointer-events: none;
  }

  #bg.away #veil {
    opacity: 1;
  }

  #bg.away #bgMove {
    filter: blur(22px) saturate(0.88);
  }
</style>
