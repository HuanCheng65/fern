<script lang="ts">
  /**
   * 布局三：详情。
   *
   * 顶部横幅加横向 tabs，tabs 以下是单一内容区。见 docs/UI_DESIGN.md 十。
   *
   * 滚动行为定死在这里而不是各页面自己实现：横幅随滚动收缩，tabs 到达顶栏
   * 下缘时吸附。长模组列表滚到深处，你仍然看得到自己在哪个实例的哪个 tab。
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
    /** 标题和常驻动作。 */
    head: Snippet
    children: Snippet
  }

  let { tabs, tab, ontab, banner, head, children }: Props = $props()

  let scroller = $state<HTMLElement>()
  /** 横幅收进去了。阈值取横幅高度的一半，收缩过程本身由 CSS 过渡完成。 */
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

  function onScroll() {
    if (scroller) compact = scroller.scrollTop > 56
  }
</script>

<div class="detail scroll" bind:this={scroller} onscroll={onScroll}>
  {#if banner}
    <div class="banner" class:compact>{@render banner()}</div>
  {/if}

  <header class="head">{@render head()}</header>

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

  <div class="body" class:reading>
    {#key tab}
      <div in:fly={slide}>{@render children()}</div>
    {/key}
  </div>
</div>

<style>
  .detail {
    position: relative;
    height: 100%;
    min-height: 0;
    padding-right: var(--s2);
  }

  /* 横幅收缩：滚下去之后只留一条，名字和启动键还在上面。 */
  .banner {
    position: relative;
    height: clamp(140px, 22vh, 220px);
    overflow: hidden;
    border-radius: var(--r3);
    transition: height var(--t-slow) var(--ease);
  }

  .banner.compact {
    height: 64px;
  }

  .head {
    padding: var(--s4) 0 var(--s3);
  }

  /*
   * 吸附在顶栏下缘。顶栏是场景层的领土，页面元素到此为止——所以这里不做
   * 毛玻璃，只补一条发丝线和一层底色，让下面滚过去的内容不透上来。
   */
  .tabs {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    gap: var(--s5);
    padding: var(--s2) 0 var(--s3);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    backdrop-filter: blur(18px);
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
