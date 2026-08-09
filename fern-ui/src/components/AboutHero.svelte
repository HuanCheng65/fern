<script lang="ts">
  /**
   * 关于页顶上那一块。
   *
   * 这是整个应用里为数不多该有身份感的地方。别处的颜色都由背景层生成并注入
   * （见 components/Backdrop.svelte），把品牌色写死等于把「界面向背景学色彩」
   * 那条原则关掉；而**关于页和欢迎屏是两个例外**——它们讲的就是这个产品是
   * 什么，所以用品牌自己的那一对值：墨松底上的嫩芽（docs/fern-brand-system.html 03）。
   *
   * 两处个性都不是凭空加的装饰，而是把已有的机制摆出来看：
   *
   * - **标志是被画出来的。** 它的几何是 7×9 网格上的八段走线（lib/mark.ts），
   *   而 `Mark` 本来就支持按格数点亮。所以入场时让它从零画到满，用的是它自己
   *   的机制，不是另加一个动效。点一下重画一遍。
   * - **每一次构建有自己的一张脸。** 底纹是封面那套生成式图形，种子取当前提交
   *   的哈希——于是每个构建的关于页长得都不一样，而同一个构建永远一样。它和
   *   实例封面是同一套东西：身份由种子决定，不由人挑。
   *
   * 动效关掉时（`theme.motionScale === 0`）不画，直接是完整的标志。
   */
  import Cover from 'fern-kit/ui/Cover.svelte'
  import Mark from 'fern-kit/ui/Mark.svelte'
  import { theme } from '../lib/theme.svelte'
  import { ui } from '../lib/i18n'

  interface Props {
    version: string
    /** 短哈希。源码构建时可能没有。 */
    commit: string
    built: string
  }

  let { version, commit, built }: Props = $props()

  /** 没有哈希就用版本号当种子——总要有一张脸。 */
  const seed = $derived(commit || version || 'fern')

  /** 0–1 时是「正在画」，undefined 是画完了的静态标志。 */
  let drawn = $state<number | undefined>(0)

  function draw() {
    if (theme.motionScale === 0) {
      drawn = undefined
      return
    }
    drawn = 0
    const started = performance.now()
    const span = 900 * theme.motionScale
    const step = (now: number) => {
      const ratio = Math.min(1, (now - started) / span)
      drawn = ratio
      if (ratio < 1) {
        requestAnimationFrame(step)
      } else {
        // 画完就交回静态那一份，别让它一直当进度条使。
        drawn = undefined
      }
    }
    requestAnimationFrame(step)
  }

  $effect(() => {
    draw()
  })
</script>

<section class="hero">
  <span class="face" aria-hidden="true"><Cover {seed} quality={0.5} /></span>

  <button class="lockup" onclick={draw} aria-label="Fern">
    <Mark size={44} progress={drawn} />
    <span class="word">fern</span>
  </button>

  <p class="tagline">{ui.about.tagline}</p>

  <p class="version t-mono">
    {version || '—'}
    <span class="build">
      {commit ? `${commit} · ${built}` : ui.about.unknownBuild}
    </span>
  </p>

  <span class="author">{ui.about.author}</span>
</section>

<style>
  .hero {
    position: relative;
    overflow: hidden;
    padding: var(--s6) var(--s5) var(--s5);
    border-radius: var(--r2);
    /* 品牌自己的底色，不跟背景层走——这一块讲的就是身份。 */
    background: var(--pine);
    color: var(--paper);
    isolation: isolate;
  }

  /* 生成式底纹：种子是提交哈希，所以每一次构建的这一块都不一样。 */
  .face {
    position: absolute;
    inset: 0;
    z-index: -1;
    opacity: 0.28;
    mask-image: linear-gradient(105deg, #000 0%, transparent 62%);
  }

  .lockup {
    display: flex;
    align-items: center;
    gap: var(--s3);
    color: var(--sprout);
  }

  .word {
    color: var(--paper);
    font-size: var(--t-h2);
    font-weight: 650;
    letter-spacing: -0.015em;
  }

  .tagline {
    margin: var(--s4) 0 0;
    color: color-mix(in srgb, var(--paper) 72%, transparent);
    font-size: var(--t-small);
  }

  .version {
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    flex-wrap: wrap;
    margin: var(--s2) 0 0;
    color: var(--paper);
    font-size: var(--t-body);
  }

  .build {
    color: color-mix(in srgb, var(--paper) 55%, transparent);
    font-size: var(--t-micro);
  }

  .author {
    position: absolute;
    right: var(--s5);
    bottom: var(--s5);
    color: color-mix(in srgb, var(--paper) 45%, transparent);
    font-size: var(--t-micro);
  }
</style>
