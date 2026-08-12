<script lang="ts">
  /**
   * 布局四：浏览。
   *
   * 左侧纵向筛选，右侧结果流，顶部一个搜索框。见 docs/frond-design-system.md。
   *
   * 语法：**横向换地方，纵向改视图。** 筛选改变的是同一批东西的呈现，不是
   * 「我在看另一个东西」，所以它在左侧纵向排布，而不是顶部的一排 chip。
   * 反过来，任何横向的东西都必须是「去别处」。
   *
   * 搜索框是这套布局唯一允许的重型控件——搜索是浏览型页面的核心动作，值得
   * 付出视觉重量。它和筛选栏一起吸附在顶栏下缘，但永不和顶栏融合：顶栏是
   * 场景层的领土，页面元素到此为止。
   */
  import type { Snippet } from 'svelte'

  interface Props {
    /** 顶部那一行。搜索框放这里。 */
    search: Snippet
    /** 左栏。用 FilterGroup 堆起来。 */
    filters: Snippet
    children: Snippet
  }

  let { search, filters, children }: Props = $props()
</script>

<div class="browse">
  <div class="searchbar">{@render search()}</div>
  <div class="body">
    <aside class="rail scroll" aria-label="筛选">{@render filters()}</aside>
    <div class="results scroll">{@render children()}</div>
  </div>
</div>

<style>
  .browse {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .searchbar {
    flex: none;
    padding-bottom: var(--s4);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    gap: clamp(var(--s5), 4vw, var(--s7));
  }

  /* 左栏定宽，结果区吃掉剩下的。 */
  .rail {
    min-height: 0;
    display: grid;
    align-content: start;
    gap: var(--s5);
    padding-right: var(--s2);
  }

  .results {
    min-height: 0;
    padding-right: var(--s2);
  }

  /* 窄窗口下左栏横过来收在搜索框下面——两列挤在一起谁都读不成。 */
  @media (max-width: 880px) {
    .body {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--s4);
    }

    .rail {
      grid-auto-flow: column;
      grid-auto-columns: max-content;
      overflow-x: auto;
      overflow-y: hidden;
      gap: var(--s5);
      padding-bottom: var(--s2);
    }
  }
</style>
