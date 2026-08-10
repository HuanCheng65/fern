<script>
  import { page } from '$app/state';
  import Mark from 'fern-kit/ui/Mark.svelte';
  // 设计系统引一次就够，页面里那几块「真实界面」靠 .fern 圈作用域。
  import 'fern-kit/styles';
  import '../app.css';

  let { children } = $props();
  let y = $state(0);
  let vh = $state(900);

  /* 只有首页顶上那一屏是深底，别的页面从头到尾都是纸白。 */
  const home = $derived(page.url.pathname === '/');
  // 顶栏在 Hero（满屏深底）之上反白，越过之后转为纸白毛玻璃。
  const past = $derived(!home || y > vh - 72);
</script>

<svelte:window bind:scrollY={y} bind:innerHeight={vh} />

<header class="nav" class:past>
  <a class="wordmark" href="/" aria-label="Fern">
    <Mark size={22} pad={4} />
    <span>fern</span>
  </a>
  <!-- 指向下载页，不是首页那一段锚点：它在任何一页上都要是同一个去处。 -->
  <a class="get" href="/download">获取 Fern</a>
</header>

{@render children()}

<style>
  .nav {
    position: fixed;
    inset: 0 0 auto 0;
    z-index: 40;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 13px clamp(20px, 4vw, 48px);
    color: var(--paper);
    transition:
      background-color 400ms ease,
      color 400ms ease,
      border-color 400ms ease;
    border-bottom: 1px solid transparent;
  }
  .nav.past {
    color: var(--ink);
    background: color-mix(in srgb, var(--paper) 82%, transparent);
    border-bottom-color: var(--line);
    -webkit-backdrop-filter: saturate(180%) blur(18px);
    backdrop-filter: saturate(180%) blur(18px);
  }

  .wordmark {
    display: flex;
    align-items: center;
    gap: 9px;
    color: inherit;
    text-decoration: none;
  }
  .wordmark span {
    font-size: 19px;
    font-weight: 650;
    letter-spacing: -0.015em;
  }

  .get {
    font-size: 13px;
    color: inherit;
    text-decoration: none;
    padding: 7px 15px;
    border: 1px solid currentColor;
    border-radius: 999px;
    opacity: 0.85;
    transition: opacity 200ms ease;
  }
  .get:hover {
    opacity: 1;
  }
</style>
