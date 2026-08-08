<script lang="ts">
  /**
   * 「正在读」的统一说法。
   *
   * 以前每处都是一行「读取中」的灰字——文字没有时间感，看不出它是卡住了还是
   * 在动。换成沿标志走线跑的那条动画（见 Mark.svelte），一眼能看出还活着，
   * 顺便让品牌出现在这个界面上出现频率最高的状态里。
   *
   * 延迟 160ms 才出现：本地读盘和缓存命中往往几十毫秒就回来了，立刻画一个
   * 加载指示只会闪一下，那一闪比等待本身更烦人。
   */
  import Mark from 'fern-kit/Mark.svelte'

  interface Props {
    /** 一句话说清在等什么。不给就只有动画。 */
    note?: string
    size?: number
    /** 撑满可用高度并居中。空列表用得上。 */
    fill?: boolean
  }

  let { note = '', size = 22, fill = false }: Props = $props()

  let visible = $state(false)
  $effect(() => {
    const timer = setTimeout(() => (visible = true), 160)
    return () => clearTimeout(timer)
  })
</script>

<div class="loading" class:fill class:visible>
  <Mark {size} spinning />
  {#if note}<span class="t-quiet">{note}</span>{/if}
</div>

<style>
  .loading {
    display: flex;
    align-items: center;
    gap: var(--s3);
    color: var(--ink-3);
    opacity: 0;
    transition: opacity var(--t-base) var(--ease);
  }

  .loading.visible {
    opacity: 1;
  }

  .fill {
    justify-content: center;
    height: 100%;
    min-height: 140px;
  }
</style>
