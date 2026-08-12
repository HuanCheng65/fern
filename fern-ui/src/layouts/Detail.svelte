<script lang="ts">
  /**
   * 布局三：详情。
   *
   * 顶部横幅加横向 tabs，tabs 以下是单一内容区。见 docs/frond-design-system.md。
   *
   * 滚动行为定死在这里而不是各页面自己实现：横幅照常滚走，tabs 那一条吸附在
   * 顶栏下缘，标题滚出视野之后在这一条里补一个小标题。长模组列表滚到深处，
   * 你仍然看得到自己在哪个实例的哪个 tab。
   *
   * **吸附的那一条高度恒定。** 上一版是让横幅随滚动收缩，那会在滚动过程中改变
   * 文档高度——内容跳一下，短页面上还会在「收缩后变矮、于是不该收缩、于是变高」
   * 之间来回抖。滚动时唯一允许变化的是不占位的东西：透明度。
   *
   * tabs 是「我在看另一个东西」，所以横向、在内容上方——和场景切换同一套语法，
   * 只是重量低一档：13px，当前项实色其余淡出，不画下划线也不套胶囊。
   *
   * **横向 tabs 之下不许再出现横向 tabs。** 二级 tabs 是结构失控的第一个
   * 症状。某个 tab 的内容多到想再分组，用表单布局的纵向锚点。
   */
  import type { Snippet } from 'svelte'
  import { fly } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import { DURATION, scaled } from '../lib/motion'

  interface Tab {
    id: string
    label: string
    /** 阅读类的内容压窄居中，列表类吃满宽度。 */
    reading?: boolean
  }

  interface Props {
    tabs: Tab[]
    tab: string
    ontab: (id: string) => void
    /** 封面那一条。没有就不画。 */
    banner?: Snippet
    /** 详情有真实横幅图时才占据顶部媒体区域。 */
    showBanner?: boolean
    /** 标题和常驻动作。 */
    head: Snippet
    /** 滚下去之后补在吸附条右侧的小标题。不给就只有 tabs。 */
    compactHead?: Snippet
    children: Snippet
  }

  let { tabs, tab, ontab, banner, showBanner = true, head, compactHead, children }: Props = $props()

  let scroller = $state<HTMLElement>()
  let heading = $state<HTMLElement>()
  /** 标题已经滚出视野。只用来淡入小标题，不改变任何元素占的位置。 */
  let compact = $state(false)

  const reading = $derived(tabs.find((item) => item.id === tab)?.reading === true)

  /**
   * 切 tab 时内容横向让一下位。
   *
   * tab 是「我在看另一个东西」，所以延续横向的语法——只是位移比场景切换小
   * 一个数量级：换的是同一个实例里的一段，不是换了个地方。
   */
  let previous = $state(0)
  const index = $derived(tabs.findIndex((item) => item.id === tab))
  const slide = $derived.by(() => {
    const direction = index >= previous ? 1 : -1
    return { x: direction * 12, duration: scaled(DURATION.base), easing: cubicOut, opacity: 0 }
  })

  $effect(() => {
    previous = index
  })

  /** 阈值从真实布局量：标题整块滚过吸附条了才补小标题，不用写死的数字。 */
  function onScroll() {
    if (!scroller || !heading) return
    compact = scroller.scrollTop > heading.offsetTop + heading.offsetHeight - 44
  }
</script>

<div class="detail scroll" data-page-scroll bind:this={scroller} onscroll={onScroll}>
  <div class="safe-top" aria-hidden="true"></div>

  {#if banner && showBanner}
    <div class="banner">{@render banner()}</div>
  {/if}

  <header class="head" bind:this={heading}>{@render head()}</header>

  <div class="bar">
    <nav class="tabs" aria-label="分区">
      {#each tabs as item (item.id)}
        <button
          class:on={tab === item.id}
          aria-current={tab === item.id ? 'page' : undefined}
          onclick={() => ontab(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </nav>

    {#if compactHead}
      <span class="mini" class:on={compact}>{@render compactHead()}</span>
    {/if}
  </div>

  <div class="body" class:reading>
    {#key tab}
      <div in:fly={slide}>{@render children()}</div>
    {/key}
  </div>
</div>

<style>
  .detail {
    position: relative;
    height: calc(100% + var(--top) + var(--s2));
    min-height: 0;
    margin: calc(-1 * (var(--top) + var(--s2))) calc(-1 * var(--pad-x)) 0;
    padding: 0 calc(var(--pad-x) + var(--s2)) 0 var(--pad-x);
  }

  /* 内容从顶栏下方起步；滚动后这段空间随内容离场，不参与 sticky 的定位。 */
  .safe-top {
    height: calc(var(--top) + var(--s2));
  }

  .banner {
    position: relative;
    height: clamp(140px, 22vh, 220px);
    overflow: hidden;
    border-radius: var(--r3);
  }

  .head {
    padding: var(--s4) 0 var(--s3);
  }

  /*
   * 吸附在顶栏下缘。顶栏是场景层的领土，页面元素到此为止。吸附条保持透明，
   * 只用一条发丝线标出页面内导航的边界。
   *
   * 高度恒定 44px。吸附元素的高度在滚动中变化，等于让整页内容跟着抖。
   */
  .bar {
    position: sticky;
    top: var(--top);
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s4);
    height: 44px;
    margin-right: calc(-1 * (var(--pad-x) + var(--s2)));
    margin-left: calc(-1 * var(--pad-x));
    padding: 0 calc(var(--pad-x) + var(--s2)) 0 var(--pad-x);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    -webkit-backdrop-filter: blur(18px) saturate(1.12);
    backdrop-filter: blur(18px) saturate(1.12);
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: var(--s5);
    min-width: 0;
  }

  /* 标题滚走之后补上。只动透明度——它占的位置从头到尾没变过。 */
  .mini {
    min-width: 0;
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-small);
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0;
    transition: opacity var(--t-base) var(--ease);
    pointer-events: none;
  }

  .mini.on {
    opacity: 1;
    pointer-events: auto;
  }

  /* 第二级重量：比场景词小一档，状态语言照抄顶栏。 */
  .tabs button {
    padding: 0;
    color: var(--ink);
    font-size: var(--t-small);
    font-weight: 500;
    opacity: 0.4;
    transition: opacity var(--t-base) var(--ease);
  }

  .tabs button:hover {
    opacity: 0.75;
  }

  .tabs button.on {
    opacity: 1;
  }

  .body {
    padding: var(--s4) 0 var(--s8);
  }

  /* 阅读类压窄。空白是排印的一部分，不满铺是刻意的。 */
  .body.reading {
    max-width: 68ch;
  }
</style>
