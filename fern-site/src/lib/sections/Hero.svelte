<script>
  import DownloadButton from '$lib/DownloadButton.svelte';
  import Spiral from '$lib/Spiral.svelte';
  import { brandField } from '$lib/field.js';
  import { onMount } from 'svelte';

  let y = $state(0);
  let vh = $state(900);
  let drawn = $state(false);

  // 整屏一块场，背景和螺线共用同一幅：背景压暗，螺线不压，
  // 于是螺线看起来是同一个世界开的一扇窗，而不是另贴的一块色。
  let hero = $state();
  let art = $state();
  let img = $state('');
  let box = $state({ w: 0, h: 0, ox: 0, oy: 0 });

  function paintField() {
    if (!hero || !art) return;
    const h = hero.getBoundingClientRect();
    const a = art.getBoundingClientRect();
    if (!h.width) return;
    // 半分辨率就够——场本来就是大色块
    const cv = brandField(h.width / 2, h.height / 2, {
      name: 'Fern',
      hours: 900,
      hour: new Date().getHours()
    });
    img = cv.toDataURL('image/webp', 0.92);
    box = { w: h.width, h: h.height, ox: a.left - h.left, oy: a.top - h.top };
  }

  // 自己听滚动，不用 svelte:window 的双向绑定——外壳已经绑了一个，
  // 两处同时绑定会互相写回滚动位置。
  onMount(() => {
    // 环境种子取当下的钟点：上午来和深夜来，这块场不是一个颜色。
    paintField();
    const ro = new ResizeObserver(paintField);
    ro.observe(hero);
    const t = setTimeout(() => (drawn = true), 220);
    let frame = 0;
    const read = () => {
      frame = 0;
      y = window.scrollY;
      vh = window.innerHeight;
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(read);
    };
    read();
    window.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    return () => {
      ro.disconnect();
      clearTimeout(t);
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
    };
  });

  const p = $derived(Math.min(1, y / Math.max(1, vh)));
  // 字往上走，螺线往下沉，两层速度不同才有纵深
  const shift = $derived(p * vh * 0.2);
  const sink = $derived(p * vh * -0.08);
  const zoom = $derived(1 + p * 0.08);
  const fade = $derived(Math.max(0, 1 - y / (vh * 0.72)));
</script>

<section class="dark hero" id="top" bind:this={hero}>
  <div class="bg" aria-hidden="true" style="background-image:url({img})"></div>
  <div class="scrim" aria-hidden="true"></div>

  <div class="art" bind:this={art} style="transform:translateY({sink}px) scale({zoom})">
    <Spiral
      on={drawn}
      {img}
      bw={box.w}
      bh={box.h}
      ox={box.ox}
      oy={box.oy}
    />
  </div>

  <div class="wrap copy" style="transform:translateY({shift}px);opacity:{fade}">
    <h1>
      好看，<br />好用，<br />好好玩。
    </h1>

    <p class="lede">一个为 Minecraft 打造的现代启动器。</p>

    <p class="body">
      管理游戏，整理世界，发现内容，与朋友连接。<br />
      从打开 Fern，到进入游戏，一切都更自然、更顺手。
    </p>

    <!-- 第一屏就给出这台机器该拿的那个文件，不是「往下翻，那儿有个按钮」 -->
    <div class="actions">
      <DownloadButton more="其他系统与版本" />
    </div>
  </div>

  <!-- 页脚一条通栏发丝线：这一屏才有边界，不是飘着的 -->
  <div class="rule" aria-hidden="true" style="opacity:{fade}">
    <span class="cue"></span>
  </div>
</section>

<style>
  .hero {
    position: relative;
    min-height: 100svh;
    display: flex;
    align-items: center;
    overflow: hidden;
    padding: 128px 0 96px;
    /* 螺线的格子边长，整个区块的度量都从它来 */
    --cell: clamp(40px, 7.6vh, 92px);
  }

  /* 整屏的场：压暗压平，只当底子 */
  .bg {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center;
    opacity: 1;
  }
  /* 左边要放字，得先把那一侧压回墨松 */
  .scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(
        100deg,
        rgba(14, 32, 24, 0.92) 0 26%,
        rgba(14, 32, 24, 0.66) 50%,
        rgba(14, 32, 24, 0.4) 76%,
        rgba(14, 32, 24, 0.32) 100%
      ),
      linear-gradient(180deg, rgba(14, 32, 24, 0.62) 0 9%, rgba(14, 32, 24, 0) 34%),
      radial-gradient(120% 80% at 50% 120%, rgba(10, 19, 14, 0.72), transparent 62%);
  }

  /* 出血：螺线右侧压出画面，才不像一枚放大的标志。
     盒子就是 7×9 格，螺线自己铺满它。 */
  .art {
    position: absolute;
    /* 右边缘对齐版心的右边缘——它属于这张网格，不是浮在上面的装饰 */
    right: max(clamp(20px, 4vw, 48px), calc(50% - 540px + clamp(20px, 4vw, 48px)));
    /* 茎底就是这个盒子的下沿（走线止于 7×9 网格的最后一行），所以贴着区块底边
       放，标志的脚正好落在这一屏的底边上——不是浮在半空中的一枚标志 */
    bottom: 0;
    width: calc(var(--cell) * 7);
    height: calc(var(--cell) * 9);
    will-change: transform;
    /* 墨松上的一点辉光，别让它像贴上去的色块 */
    filter: drop-shadow(0 0 70px rgba(53, 113, 74, 0.4));
  }

  .copy {
    position: relative;
    z-index: 1;
    will-change: transform;
  }

  h1 {
    font-size: clamp(46px, 8.4vw, 132px);
    line-height: 1;
    letter-spacing: -0.035em;
  }

  .lede {
    margin-top: clamp(26px, 4vh, 44px);
    max-width: 36ch;
    color: var(--paper);
  }

  .body {
    margin-top: 16px;
    max-width: 40ch;
    color: var(--on-dark-mut);
    font-size: 15px;
    line-height: 2;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 22px;
    flex-wrap: wrap;
    margin-top: clamp(32px, 5vh, 52px);
  }

  .rule {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    height: 74px;
    padding: 0 clamp(20px, 4vw, 48px) 22px;
    border-top: 1px solid var(--on-dark-line);
  }
  /* 下滚提示：一个直角，和标志同一种转法 */
  .cue {
    display: block;
    width: 22px;
    height: 22px;
    border-left: 1px solid var(--sprout);
    border-bottom: 1px solid var(--sprout);
    opacity: 0.5;
    animation: step 2.8s ease-in-out infinite;
  }
  @keyframes step {
    0%,
    100% {
      transform: translateY(0);
      opacity: 0.25;
    }
    50% {
      transform: translateY(7px);
      opacity: 0.6;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cue {
      animation: none;
    }
  }

  @media (max-width: 900px) {
    .hero {
      --cell: clamp(38px, 6.4vh, 66px);
    }
    /* 一列的时候螺线退到右下角当背景：字压在上面，得让它读得动 */
    .art {
      right: calc(var(--cell) * -1.7);
      top: auto;
      bottom: calc(var(--cell) * -1.6);
      translate: 0 0;
      opacity: 0.34;
      filter: none;
    }
  }
</style>
