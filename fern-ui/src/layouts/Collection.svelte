<script lang="ts">
  /**
   * 布局二：集合。
   *
   * 顶部一行轻量控制，下方网格铺开。见 docs/UI_DESIGN.md 十。
   *
   * 控制行贴着顶栏下方，右对齐，纯文字按钮，不做工具栏样式——一个工具栏会
   * 和它下面的封面网格争视觉重量，而这一屏的主视觉就是那些封面。
   */
  import type { Snippet } from 'svelte'

  interface Props {
    /** 左边通常是一句状态（几个实例），右边是动作。 */
    controls?: Snippet
    children: Snippet
  }

  let { controls, children }: Props = $props()
</script>

<div class="collection">
  {#if controls}
    <div class="controls">{@render controls()}</div>
  {/if}
  <div class="grid scroll">{@render children()}</div>
</div>

<style>
  .collection {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    flex: none;
    padding-bottom: var(--s4);
  }

  .grid {
    flex: 1;
    min-height: 0;
    padding-right: var(--s2);
  }
</style>
