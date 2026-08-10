<script>
  import { onMount } from 'svelte';
  import { reveal, once } from '$lib/scroll.js';
  import { registerDemo } from '$lib/demo-subjects.js';
  import Cover from 'fern-kit/ui/Cover.svelte';

  // 产品里的那一个命令面板，原样嵌进来：真的匹配、真的排序、真的拼音。
  // 只在浏览器里加载——引擎要读 localStorage 记使用习惯，SSR 里没有它。
  let Palette = $state(null);
  let store = $state(null);
  // 面板的输入框会自动聚焦，所以等这一屏滚到眼前再挂上去——
  // 页面一载入就挂，浏览器会把视口直接拽到这里。
  let open = $state(false);
  let cancelled = false;
  let pending = $state(false);
  let toast = $state('');
  let toastTimer;

  // 站上没有真的实例可以启动，执行的结果就是这一句。面板本身不关。
  function say(text) {
    toast = text;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ''), 2600);
  }

  onMount(async () => {
    const [component, engine] = await Promise.all([
      import('fern-kit/parts/CommandPalette.svelte'),
      import('fern-kit/parts/palette')
    ]);
    registerDemo(engine, say);
    store = engine.palette;
    Palette = component.default;
    if (pending) play(false);
  });

  const DEMO = ['1165', 'wdsj'];
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));

  // 进入视口后自己打一遍，打完把光标交还给访客。
  function play(skip) {
    if (!store) {
      pending = true;
      return;
    }
    open = true;
    if (skip) {
      store.query = DEMO[1];
      return;
    }
    run();
  }

  async function run() {
    for (let i = 0; i < DEMO.length; i++) {
      for (const ch of DEMO[i]) {
        if (cancelled) return;
        store.query += ch;
        await wait(140);
      }
      await wait(2200);
      if (i === DEMO.length - 1) return;
      while (store.query.length) {
        if (cancelled) return;
        store.query = store.query.slice(0, -1);
        await wait(55);
      }
      await wait(400);
    }
  }

  /**
   * 访客一动手就停。
   *
   * 这个面板是真的，所以它必须让人打得动——而自动演示还在往同一个 query 里写字。
   * 两个都想要就只能定一条让位规则：按键或点击一到，演示当场结束且不再恢复。
   *
   * 不听 focus：面板挂上去就自动聚焦输入框，那是它自己干的，不是人干的。
   */
  function takeover() {
    if (cancelled) return;
    cancelled = true;
    if (store) store.query = '';
  }

  $effect(() => () => {
    cancelled = true;
    if (store) {
      store.query = '';
      store.scope = null;
    }
  });
</script>

<section id="direct" class="direct">
  <div class="wrap">
    <div class="head" use:reveal>
      <div class="eyebrow">Fern 直达</div>
      <h2>所想，即达。</h2>
    </div>

    <!--
      transform 让这个盒子成为固定定位的包含块，面板那层全屏浮层就被关在框里，
      不会盖住整页。嵌的是组件本身，不是截图，所以这里可以真的打字。
    -->
    <div
      class="stage fern fern-dark"
      use:reveal={{ delay: 60 }}
      use:once={play}
      onkeydowncapture={takeover}
      onpointerdowncapture={takeover}
    >
      <div class="behind" aria-hidden="true">
        <Cover seed="主世界" hours={320} hour={18} w={900} h={520} quality={0.6} />
      </div>
      {#if Palette && open}
        <!-- 不关：这一格是给人试的，关掉就没得试了。执行的结果由 say() 播报。 -->
        <Palette onclose={() => {}} />
      {/if}
    </div>

    <p class="tryit">这是 Fern 直达本身，可以直接输入。</p>

    <div class="col tail" use:reveal>
      <p>实例、存档、服务器、用户档案、功能、设置，以及来自网络的内容，都汇聚在同一个入口。</p>
      <p>名称、拼音、首字母、版本号，甚至混合输入，都可以直接找到。</p>
      <p>Fern 直达还会随着使用逐渐熟悉你的习惯，让真正需要的内容更快出现。</p>
    </div>
  </div>

  {#if toast}
    <div class="toast" role="status">{toast}</div>
  {/if}
</section>

<style>
  .head {
    text-align: center;
  }
  .head h2 {
    margin-top: 14px;
  }

  .stage {
    position: relative;
    /* 建立包含块。少了它，面板会铺满整个视口。 */
    transform: translateZ(0);
    height: 520px;
    margin: clamp(44px, 6vw, 76px) auto 18px;
    max-width: 900px;
    border-radius: 18px;
    overflow: hidden;
    background: var(--pine);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.1),
      0 24px 70px rgba(20, 32, 26, 0.14);
  }

  .behind {
    position: absolute;
    inset: 0;
  }
  .behind :global(canvas) {
    width: 100% !important;
    height: 100% !important;
    border-radius: 0;
  }

  .tryit {
    text-align: center;
    font-size: 14px;
    color: var(--mut);
  }

  /* 站的口气，不是面板的。所以它在盒子外面，用站自己的颜色。 */
  .toast {
    position: fixed;
    left: 50%;
    bottom: 32px;
    translate: -50% 0;
    z-index: 60;
    padding: 12px 22px;
    border-radius: 999px;
    background: var(--pine);
    color: var(--paper);
    font-size: 14px;
    box-shadow: 0 12px 40px rgba(20, 32, 26, 0.24);
    animation: rise 240ms cubic-bezier(0.2, 0.7, 0.2, 1);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }

  .tail {
    margin: clamp(44px, 6vw, 72px) auto 0;
    text-align: center;
  }
  .tail p {
    color: var(--mut);
    font-size: 17px;
  }
</style>
