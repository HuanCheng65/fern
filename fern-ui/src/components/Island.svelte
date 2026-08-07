<script lang="ts">
  /**
   * 岛：顶栏右段那块会变形的状态区。
   *
   * 它不认识作业、不认识游戏、不认识联机房间——只认识 [`Presence`]。谁想在
   * 这里说话，就在自己的模块里调一次 `contributes()`；**这个文件不会因此改
   * 一个字**。见 lib/island.svelte.ts。
   *
   * ## 变形怎么做的
   *
   * 三条规矩，缺一条就会变成上一版那种「弹了个下拉菜单」的观感：
   *
   * 1. **只有一块表面会变尺寸。** 玻璃背景是唯一会长大的东西，紧凑态和展开态
   *    是同一个盒子的两个尺寸，不是两个盒子。
   * 2. **内容从第一帧就按终态排好。** 面板内容定死 320px 宽、绝对定位，展开
   *    只是表面把它露出来。上一版让宽度和高度一起动，里面的文字每一帧换行位置
   *    都不一样——那个抖动就是「诡异」的来源。
   * 3. **紧凑那一行不被替换。** 它就是面板的表头；表面往左长的时候它跟着摊开
   *    （图标滑到左边、百分比留在右边），而不是淡出再淡入另一个东西。
   *
   * 动的只有 `left` 和 `bottom` 两个长度：表面向左长 `320 - 胶囊宽`，向下长
   * 一个面板高。两个数都要量——量的是**不受表面影响**的东西（占位副本的宽、
   * 定死 320px 的面板的高），所以不存在循环依赖。
   *
   * 撑出胶囊宽度的是一个 `visibility: hidden` 的占位副本，和表头由同一个
   * snippet 渲染，保证两份一模一样。有了它，表头才能绝对定位、跟着表面边缘
   * 摊开，而顶栏的布局从头到尾一动不动——**变形时唯一允许动的是不占位的
   * 东西**，这条和详情页那条吸附条是同一个教训。
   *
   * 位置上右缘钉死（顶着设置和头像），向左下方长。中段是五个场景词的圣地。
   *
   * 桌面有悬停，这是比手机强的地方：扫一眼就展开，移开就收回，真想细看再点
   * 一下钉住。进入延迟 180ms，免得你伸手去够设置图标时它蹦出来；离开延迟长
   * 一些，好让鼠标走得进面板。
   *
   * 零状态时整个组件不存在——不是透明度 0。大多数时候 Fern 的顶栏应该只有
   * 一行安静的文字。
   */
  import { flip } from 'svelte/animate'
  import { scale } from 'svelte/transition'
  import { TriangleAlert, X } from 'lucide-svelte'
  import Mark from './Mark.svelte'
  import { island, type Presence } from '../lib/island.svelte'
  import { DURATION, scaled } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'

  /** 展开后的宽度。够放下一行「补全游戏文件 · 412 MB / 1.1 GB · 8.2 MB/s」。 */
  const PANEL = 320

  const main = $derived(island.main)
  const satellites = $derived(island.satellites)
  const pinned = $derived(nav.overlay === 'island')

  let hovering = $state(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  const open = $derived(pinned || hovering)
  /** 展开后列的是全部，不只是主胶囊那一条——面板是全景。 */
  const rows = $derived(island.all.flatMap((presence) => presence.rows))
  const actions = $derived(island.all.flatMap((presence) => presence.actions))

  /** 胶囊自己的宽（占位副本撑出来的）和面板的自然高。表面照这两个数去长。 */
  let pillWidth = $state(0)
  let bodyHeight = $state(0)

  const growLeft = $derived(open ? Math.min(0, pillWidth - PANEL) : 0)
  const growDown = $derived(open ? -bodyHeight : 0)

  function hover(next: boolean) {
    clearTimeout(timer)
    timer = setTimeout(() => (hovering = next), next ? 180 : 320)
  }

  const percent = (presence: Presence) =>
    presence.fraction === undefined ? '' : `${Math.round(presence.fraction * 100)}%`
</script>

{#snippet glyph(presence: Presence, size: number)}
  {#if presence.tone === 'work'}
    <!-- 进度长在标志上：螺线画完即完成（见 docs/fern-brand-system.html 06）。 -->
    <Mark {size} spinning={presence.fraction === undefined} progress={presence.fraction} />
  {:else if presence.tone === 'alert'}
    <TriangleAlert {size} strokeWidth={2.2} />
  {:else}
    <span class="dot"></span>
  {/if}
{/snippet}

<!--
  紧凑那一行只写一次，渲染两遍：一遍藏起来撑宽度，一遍是真正看得见的表头。
  两份必须一模一样，所以不能各写各的。
-->
{#snippet compact(presence: Presence)}
  <span class="lead">
    {@render glyph(presence, 14)}
    <span class="label">{presence.label}</span>
  </span>
  {#if percent(presence)}<span class="pct t-mono">{percent(presence)}</span>{/if}
{/snippet}

{#if main}
  <!--
    role="group" 而不是 status：status 是活区，屏幕阅读器会把每一次进度变化
    都念出来，而这里的数字每秒都在动。要念的是结果，不是每一帧。
  -->
  <div
    class="island"
    role="group"
    aria-label="进行中"
    onmouseenter={() => hover(true)}
    onmouseleave={() => hover(false)}
    transition:scale={{ start: 0.86, duration: scaled(DURATION.base), opacity: 0 }}
  >
    <!--
      次要状态分裂成小圆点。位置由 flip 算——主胶囊和卫星之间的合并与分裂是
      这套设计里唯一值得花功夫的动效，而它恰好是框架已经解决了的那类问题。
    -->
    {#each satellites as item (item.id)}
      <span
        class="sat {item.tone}"
        title={item.label}
        animate:flip={{ duration: scaled(DURATION.base) }}
      >
        {@render glyph(item, 11)}
      </span>
    {/each}

    <div class="pill" class:open bind:clientWidth={pillWidth}>
      <!-- 只为撑出胶囊的宽和高。看不见、不可点、不进无障碍树。 -->
      <span class="spacer" aria-hidden="true">{@render compact(main)}</span>

      <div
        class="surface {main.tone}"
        style:left={`${growLeft}px`}
        style:bottom={`${growDown}px`}
      >
        <button
          class="head"
          title={main.label}
          aria-expanded={open}
          onclick={() => nav.toggle('island')}
        >
          {@render compact(main)}
        </button>

        <div class="body">
          <!-- offsetHeight 而不是 clientHeight：那道分隔线也得算进去。 -->
          <div class="inner" bind:offsetHeight={bodyHeight}>
            {#each rows as row (row.id)}
              <div class="row">
                <div class="row-head">
                  <span class="name">{row.label}</span>
                  {#if row.dismiss}
                    <button class="close" aria-label="不再提示" onclick={() => row.dismiss?.()}>
                      <X size={12} strokeWidth={2.2} />
                    </button>
                  {/if}
                </div>
                {#if row.detail}<p class="detail">{row.detail}</p>{/if}
                {#if row.meta}<p class="meta t-mono">{row.meta}</p>{/if}
                {#if row.fraction !== undefined}
                  <div class="bar"><span style:width={`${row.fraction * 100}%`}></span></div>
                {/if}
              </div>
            {/each}

            {#if actions.length > 0}
              <div class="acts">
                {#each actions as action (action.label)}
                  <button class="btn btn--link" onclick={action.run}>{action.label}</button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .island {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin-right: var(--s2);
    transform-origin: 100% 50%;
  }

  /* 卫星：只有字形，没有文字。 */
  .sat {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--tint-1);
    color: var(--ink-3);
  }

  .sat.alert {
    color: var(--danger);
  }

  /*
   * 胶囊的占位框。它的尺寸永远是紧凑态的尺寸，从头到尾不变——表面在它之外
   * 生长，所以顶栏的其他东西一个像素都不会被推动。
   */
  .pill {
    position: relative;
    height: 32px;
  }

  .spacer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    height: 100%;
    padding: 0 var(--s3);
    font-size: var(--t-micro);
    visibility: hidden;
  }

  /*
   * 唯一会变尺寸的东西。
   *
   * 动的是 left 和 bottom 两个长度：向左长到 320，向下长一个面板高。这两个
   * 属性会触发布局，但这里只有一个小盒子在动，而用 transform 缩放会把里面的
   * 文字一起拉变形——真正的尺寸变化只能这么做。
   */
  .surface {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: 999px;
    background: var(--panel);
    box-shadow: inset 0 0 0 1px var(--panel-line);
    /* `-webkit-` 前缀不能省：WebKitGTK 只认带前缀的那个。 */
    -webkit-backdrop-filter: blur(20px) saturate(1.2);
    backdrop-filter: blur(20px) saturate(1.2);
    transition:
      left var(--t-slow) var(--spring),
      bottom var(--t-slow) var(--spring),
      border-radius var(--t-slow) var(--spring),
      box-shadow var(--t-base) var(--ease);
  }

  .pill.open .surface {
    border-radius: var(--r3);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      var(--shadow-lg);
  }

  /*
   * 表头。左右都钉在表面上，所以表面往左长的时候它自己就摊开了——图标滑向
   * 左缘、百分比留在右缘。不需要为它单独写任何动画。
   */
  .head {
    position: absolute;
    inset: 0 0 auto 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    height: 32px;
    padding: 0 var(--s3);
    color: var(--ink-2);
    font-size: var(--t-micro);
    transition: color var(--t-fast) var(--ease);
  }

  .head:hover {
    color: var(--ink);
  }

  .surface.alert .head {
    color: var(--danger);
  }

  .lead {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-width: 0;
  }

  .label {
    max-width: 22ch;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .pct {
    flex: none;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  /*
   * 面板内容。宽度定死，位置定死，从第一帧起就是终态的样子——展开只是表面
   * 把它露出来。绝不让它跟着动画重排。
   */
  .body {
    position: absolute;
    top: 32px;
    right: 0;
    width: 320px;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-base) var(--ease);
  }

  .pill.open .body {
    opacity: 1;
    pointer-events: auto;
    /* 先让表面开始长，内容再跟上——同时出现会显得内容是「贴」上去的。 */
    transition-delay: 90ms;
  }

  .inner {
    display: grid;
    gap: var(--s3);
    padding: var(--s3) var(--s4) var(--s4);
    border-top: 1px solid var(--panel-line);
  }

  .row {
    display: grid;
    gap: 3px;
  }

  .row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-small);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .close {
    flex: none;
    padding: 0;
    color: var(--ink-4);
  }

  .close:hover {
    color: var(--ink);
  }

  /* 人话给所有人看，机器数用等宽——看不看都不影响操作。 */
  .detail {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-micro);
    overflow-wrap: anywhere;
  }

  .meta {
    margin: 0;
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .bar {
    width: 100%;
    height: 2px;
    margin-top: 3px;
    border-radius: 2px;
    background: var(--tint-2);
  }

  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width var(--t-base) var(--ease);
  }

  .acts {
    display: flex;
    gap: var(--s4);
  }
</style>
