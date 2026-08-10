<script>
  import { ChevronDown } from 'lucide-svelte';
  import { CHANNEL_NAME, currentRelease, detectOs, filesFor, osName } from '$lib/downloads.js';

  /*
   * 「下载当前系统的包」那颗按钮。首页顶上、页尾、下载页顶上是同一颗。
   *
   * 三处各写一遍的话，规则一变（比如 Linux 多一种格式）就会漏掉其中一处，而漏掉的
   * 那一处不会报错，只会一直发出一个过时的文件。
   *
   * 同一平台有多个格式时，右边才长那个箭头。只有一个格式的平台不长——没有可选的
   * 东西，就不该摆出一副可选的样子。
   *
   * 认不出系统、或者读不到版本，按钮退回「获取 Fern」并指向下载页。**不猜**：猜错
   * 一次的代价是用户下到一个装不上的包。
   */
  let {
    /** 按钮底下那行小字：架构、版本号、通道。 */
    meta = false,
    /** 给一句话就在底下挂一个指向下载页的文本链接，不给就没有。 */
    more = '',
    /**
     * 认不出系统时退回「获取 Fern」并指向下载页。下载页自己关掉它——在那一页上，
     * 那颗按钮指向的正是当前这一页，而底下的全平台清单本来就是答案。
     */
    fallback = true
  } = $props();

  let os = $state(null);
  let release = $state(null);

  $effect(() => {
    os = detectOs();
    currentRelease().then((found) => (release = found));
  });

  const options = $derived(os && release ? filesFor(release.version)[os] : null);
  const primary = $derived(options?.[0] ?? null);

  /*
   * 格式选单走 popover。
   *
   * 两个理由，都不是「新东西好看」：一是它在顶层，**不受祖先 overflow 裁剪**——
   * 第一屏那颗按钮所在的区块是 `overflow: hidden`（螺线要出血），普通的绝对定位
   * 浮层会被切掉一截；二是点外面关掉、Esc 关掉、按钮自己开合，全都由浏览器管，
   * 不用在 window 上挂一对互相打架的监听。
   *
   * 代价是位置得自己算：顶层元素不跟着页面滚，所以 fixed 坐标得一直跟着按钮量。
   * 只听 scroll 不够——第一屏的字有视差，是在滚动事件之后的那一帧才移到位的，
   * 照着滚动事件量出来的位置会差出视差那一截。所以开着的时候按帧量。
   */
  const menuId = $props.id();

  let anchor = $state();
  let menu = $state();
  let open = $state(false);
  let at = $state({ left: 0, top: 0 });
  /* 量出来之前先不显形：浮层是先出现再收到 toggle 的，不挡一下会闪一帧。 */
  let placed = $state(false);

  /** 离视口边留的余量，和浮层与按钮之间的缝。 */
  const EDGE = 12;
  const GAP = 10;

  function place() {
    if (!anchor || !menu) return;
    const a = anchor.getBoundingClientRect();

    /* 按钮滚出视口就收起来：夹在屏幕边上的浮层已经不属于任何一颗按钮了。 */
    if (a.bottom < 0 || a.top > window.innerHeight) {
      menu.hidePopover();
      return;
    }

    const w = menu.offsetWidth;
    const h = menu.offsetHeight;

    /* 默认右边对齐按钮的右边。贴到左边界就翻成左对齐，两头都放不下才夹住。 */
    let left = a.right - w;
    if (left < EDGE) left = a.left;
    left = Math.min(Math.max(left, EDGE), Math.max(EDGE, window.innerWidth - w - EDGE));

    /* 底下放不下就翻到按钮上方——首页第一屏那颗按钮本来就在屏幕下缘。 */
    let top = a.bottom + GAP;
    if (top + h > window.innerHeight - EDGE && a.top - GAP - h > EDGE) top = a.top - GAP - h;
    top = Math.min(Math.max(top, EDGE), Math.max(EDGE, window.innerHeight - h - EDGE));

    /* 位置没变就别写回去，不然每一帧都触发一次更新。 */
    if (at.left !== left || at.top !== top) at = { left, top };
    placed = true;
  }

  $effect(() => {
    if (!open) return;
    let frame = requestAnimationFrame(function tick() {
      place();
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

{#if primary || fallback}
  {#if primary}
    <div class="split" class:multi={options.length > 1} bind:this={anchor}>
      <a class="cta" href={primary.url} download>下载 {osName(os)} 版</a>

      {#if options.length > 1}
        <!-- 开合交给 popovertarget：浏览器知道谁是触发者，点它关掉时不会先被
             「点外面」关一次再被点击开一次。 -->
        <button class="more" popovertarget={menuId} aria-label="其他格式" aria-expanded={open}>
          <ChevronDown size={18} strokeWidth={2} />
        </button>

        <div
          class="menu"
          id={menuId}
          popover="auto"
          bind:this={menu}
          style="left:{at.left}px;top:{at.top}px;opacity:{placed ? 1 : 0}"
          ontoggle={(e) => {
            open = e.newState === 'open';
            if (open) place();
            else placed = false;
          }}
        >
          {#each options as file (file.id)}
            <a href={file.url} download>
              <span class="n">{file.label}{#if file.ext}<em>{file.ext}</em>{/if}</span>
              <span class="h">{file.note}</span>
            </a>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <a class="cta" href="/download">获取 Fern</a>
  {/if}

  {#if meta}
    <p class="meta mono">
      {#if primary && release}
        {primary.note} · {release.version} · {CHANNEL_NAME[release.channel]}
      {:else}
        Windows · macOS · Linux
      {/if}
    </p>
  {/if}

  {#if more}
    <a class="ghost" href="/download">{more}</a>
  {/if}
{/if}

<style>
  /* 主按钮和那个箭头是一颗按钮的两半，中间只留一道发丝线。 */
  .split {
    display: flex;
    align-items: stretch;
  }
  .split.multi .cta {
    border-radius: 999px 0 0 999px;
    padding-right: 22px;
  }

  .more {
    display: grid;
    place-items: center;
    width: 46px;
    padding: 0;
    border: 0;
    border-radius: 0 999px 999px 0;
    background: var(--fern);
    color: var(--paper);
    box-shadow: inset 1px 0 0 rgba(246, 244, 236, 0.22);
    cursor: pointer;
    transition: background 200ms ease;
  }
  .more:hover {
    background: #2c5f3e;
  }
  /* 深底上和 .cta 一样翻个个儿。这两条要跟着 app.css 里的 .cta 一起改。 */
  :global(.dark) .more {
    background: var(--sprout);
    color: var(--pine);
    box-shadow: inset 1px 0 0 rgba(14, 32, 24, 0.18);
  }
  :global(.dark) .more:hover {
    background: #cdeec1;
  }

  /*
   * 浮层跟着所在的这一页走：纸白页上是一张纸，深底上才是一块深色的板。
   *
   * 位置全部由脚本写在 left/top 上。`[popover]` 的浏览器默认样式是
   * `inset: 0; margin: auto`，不清掉的话它会端端正正地停在屏幕正中；边框和
   * 内边距也一并接管。顶层元素不参与 z-index，所以这里不用再写层级。
   */
  .menu {
    position: fixed;
    inset: auto;
    margin: 0;
    min-width: 264px;
    padding: 6px;
    border: 0;
    border-radius: 14px;
    overflow: visible;
    background: var(--paper);
    color: var(--ink);
    box-shadow:
      inset 0 0 0 1px var(--line),
      0 20px 50px rgba(20, 32, 26, 0.16);
    text-align: left;
    transition: opacity 120ms ease;
  }
  /*
   * 关着的时候由浏览器的 `[popover]:not(:popover-open) { display: none }` 藏起来，
   * 而作者样式压过浏览器样式——所以 display 只能写在打开这一态上，写在 .menu 上
   * 会把那条藏起来的规则顶掉，浮层就一直挂在屏幕上。
   */
  .menu:popover-open {
    display: grid;
  }
  .menu a {
    display: grid;
    gap: 2px;
    padding: 11px 13px;
    border-radius: 9px;
    color: var(--ink);
    text-decoration: none;
    transition: background 160ms ease;
  }
  .menu a:hover {
    background: rgba(45, 95, 62, 0.07);
  }
  .menu .n {
    font-size: 15px;
    font-weight: 560;
  }
  .menu .n em {
    margin-left: 7px;
    font-family: var(--mono);
    font-size: 12px;
    font-style: normal;
    opacity: 0.6;
  }
  .menu .h {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--mut);
  }

  :global(.dark) .menu {
    background: #0f2019;
    box-shadow:
      inset 0 0 0 1px rgba(246, 244, 236, 0.14),
      0 20px 50px rgba(0, 0, 0, 0.4);
  }
  :global(.dark) .menu a {
    color: var(--paper);
  }
  :global(.dark) .menu a:hover {
    background: rgba(246, 244, 236, 0.08);
  }
  :global(.dark) .menu .h {
    color: var(--on-dark-mut);
  }

  .meta {
    font-size: 12px;
    letter-spacing: 0.1em;
    color: var(--mut);
  }
  :global(.dark) .meta {
    color: var(--on-dark-mut);
  }
</style>
