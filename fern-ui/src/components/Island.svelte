<script lang="ts">
  /**
   * 岛：顶栏右段那块会变形的状态区。
   *
   * 它不认识作业、不认识游戏、不认识联机房间——只认识 [`Presence`]。谁想在
   * 这里说话，就在自己的模块里调一次 `contributes()`；**这个文件不会因此改
   * 一个字**。见 lib/island.svelte.ts。
   *
   * 形态：主胶囊右缘钉死（顶着设置和头像），向左长；卫星在它左边，只有字形
   * 没有文字。展开的面板从胶囊下缘长出来，共用同一层玻璃和同一个圆角，读起来
   * 是同一块东西在变形，而不是弹出了一个新窗口。
   *
   * 胶囊本身留在 flex 流里、面板绝对定位：胶囊要为自己占位，否则设置图标会
   * 被压在下面；面板不占位，所以它怎么长都不会推动别人。**变形时唯一允许动
   * 的是不占位的东西**——这条和详情页那条吸附条是同一个教训。
   *
   * 桌面有悬停，这是比手机强的地方：扫一眼就展开，移开就收回，真想细看再点
   * 一下钉住。进入延迟 180ms，免得你伸手去够设置图标时它蹦出来；离开延迟长
   * 一些，好让鼠标走得进面板。
   *
   * 零状态时整个组件不存在——不是透明度 0。大多数时候 Fern 的顶栏应该只有
   * 一行安静的文字。
   */
  import { flip } from 'svelte/animate'
  import { fly } from 'svelte/transition'
  import { TriangleAlert, X } from 'lucide-svelte'
  import Mark from './Mark.svelte'
  import { island, type Presence } from '../lib/island.svelte'
  import { DURATION, scaled } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'

  const main = $derived(island.main)
  const satellites = $derived(island.satellites)
  const pinned = $derived(nav.overlay === 'island')

  let hovering = $state(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  const open = $derived(pinned || hovering)
  /** 展开后列的是全部，不只是主胶囊那一条——面板是全景。 */
  const rows = $derived(island.all.flatMap((presence) => presence.rows))
  const actions = $derived(island.all.flatMap((presence) => presence.actions))

  function hover(next: boolean) {
    clearTimeout(timer)
    timer = setTimeout(() => (hovering = next), next ? 180 : 320)
  }

  const percent = (presence: Presence) =>
    presence.fraction === undefined ? '' : `${Math.round(presence.fraction * 100)}%`
</script>

{#if main}
  <!--
    role="group" 而不是 status：status 是活区，屏幕阅读器会把每一次进度变化
    都念出来，而这里的数字每秒都在动。要念的是结果，不是每一帧。
  -->
  <div
    class="island"
    class:open
    role="group"
    aria-label="进行中"
    onmouseenter={() => hover(true)}
    onmouseleave={() => hover(false)}
    transition:fly={{ x: 12, duration: scaled(DURATION.base) }}
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
        {#if item.tone === 'work'}
          <Mark size={11} spinning={item.fraction === undefined} progress={item.fraction} />
        {:else if item.tone === 'alert'}
          <TriangleAlert size={11} strokeWidth={2.2} />
        {:else}
          <span class="dot"></span>
        {/if}
      </span>
    {/each}

    <button
      class="capsule {main.tone}"
      title={main.label}
      aria-expanded={open}
      onclick={() => nav.toggle('island')}
    >
      {#if main.tone === 'work'}
        <!-- 进度长在标志上：螺线画完即完成（见 docs/fern-brand-system.html 06）。 -->
        <Mark size={14} spinning={main.fraction === undefined} progress={main.fraction} />
      {:else if main.tone === 'alert'}
        <TriangleAlert size={13} strokeWidth={2.2} />
      {:else}
        <span class="dot"></span>
      {/if}
      <span class="label">{main.label}</span>
      {#if percent(main)}<span class="pct t-mono">{percent(main)}</span>{/if}
    </button>

    <div class="sheet" aria-hidden={!open}>
      <div class="clip">
        <div class="inner">
          {#each rows as row (row.id)}
            <div class="row">
              <div class="head">
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
{/if}

<style>
  .island {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin-right: var(--s2);
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

  .capsule {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 32px;
    padding: 0 var(--s3);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-2);
    font-size: var(--t-micro);
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      border-radius var(--t-base) var(--ease);
  }

  .capsule:hover {
    background: var(--tint-2);
    color: var(--ink);
  }

  .capsule.alert {
    color: var(--danger);
  }

  /* 展开时下缘变方，和底下的面板接成一块。 */
  .island.open .capsule {
    border-bottom-left-radius: var(--r2);
    border-bottom-right-radius: var(--r2);
  }

  .label {
    max-width: 18ch;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .pct {
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
   * 面板：右缘和胶囊对齐，向左下方长。
   *
   * 高度用 grid-template-rows 从 0fr 到 1fr——这是唯一能真正动起来的「到内容
   * 高度」的过渡，比写死一个 max-height 诚实（内容多了不会被截掉，少了也不会
   * 留一段空白）。宽度用 min-width，`width: fit-content` 会尊重它，于是紧凑
   * 时贴着内容、展开时长到 300。
   */
  .sheet {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    display: grid;
    grid-template-rows: 0fr;
    width: fit-content;
    min-width: 0;
    border-radius: var(--r3);
    opacity: 0;
    pointer-events: none;
    transition:
      grid-template-rows var(--t-base) var(--ease),
      min-width var(--t-base) var(--ease),
      opacity var(--t-fast) var(--ease);
  }

  .island.open .sheet {
    grid-template-rows: 1fr;
    min-width: 300px;
    opacity: 1;
    pointer-events: auto;
  }

  .clip {
    overflow: hidden;
    border-radius: inherit;
  }

  .inner {
    display: grid;
    gap: var(--s3);
    padding: var(--s3) var(--s4);
    border-radius: inherit;
    background: var(--panel);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      var(--shadow-lg);
    /* `-webkit-` 前缀不能省：WebKitGTK 只认带前缀的那个。 */
    -webkit-backdrop-filter: blur(20px) saturate(1.2);
    backdrop-filter: blur(20px) saturate(1.2);
  }

  .row {
    display: grid;
    gap: 3px;
  }

  .head {
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
