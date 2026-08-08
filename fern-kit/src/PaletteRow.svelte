<script lang="ts">
  /**
   * 直达里的一行。
   *
   * 从命令面板里拆出来，因为这一行不只属于面板：官网要在别的地方原样呈现
   * 它，拆开之后两边看到的是同一个组件，而不是一份仿制品。
   *
   * 它不认识数据源，也不认识面板：给它一个 Row 和一个选中态，它负责画。
   * 缩略图由调用方给——产品那边是带 worker 的封面，官网那边是一张画布，
   * 这一行不必知道区别。
   */
  import { ArrowRight } from 'lucide-svelte'
  import { pieces, type Row } from './palette.svelte'
  import type { Snippet } from 'svelte'

  interface Props {
    row: Row
    /** 选中态只有一个：键盘和鼠标共用它，不做两套高亮互相打架。 */
    active?: boolean
    /** 这一行还能不能往里走。只在高亮时出现——每一行都挂一个提示是噪音。 */
    deeper?: boolean
    /** 有种子的行画缩略图，画什么由调用方决定。 */
    thumb?: Snippet<[string]>
    onhover?: () => void
    onrun?: () => void
  }

  let { row, active = false, deeper = false, thumb, onhover, onrun }: Props = $props()

  const title = $derived(row.kind === 'subject' ? row.subject.title : row.action.title)
  const seed = $derived(row.kind === 'subject' ? row.subject.seed : undefined)

  /**
   * 标题下那一行小字。
   *
   * 命中落在看不见的别名上时（打 gc 出来「垃圾回收器」），把那个词补在末尾
   * ——一行凭一个你看不见的词进了列表，是这里最让人困惑的情况。
   */
  const note = $derived(
    [row.kind === 'subject' ? row.subject.hint : row.action.hint, row.via].filter(Boolean).join(' · ')
  )
</script>

<!--
  打中的字加重：这一行为什么在这儿，看一眼就知道，不用猜。写成一整行是因为
  标签之间的换行会变成真的空格，跑到标题最前面去。
-->
{#snippet marked(text: string, at: number[])}{#each pieces(text, at) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}{/snippet}

<button class="row" class:on={active} onmouseenter={onhover} onclick={onrun}>
  {#if seed && thumb}
    <span class="thumb">{@render thumb(seed)}</span>
  {:else}
    <span class="glyph"><ArrowRight size={14} strokeWidth={2} /></span>
  {/if}
  <span class="text">
    <strong>{@render marked(title, row.at)}</strong>
    {#if note}<small>{note}</small>{/if}
  </span>
  {#if row.kind === 'action' && row.action.keys}<kbd>{row.action.keys}</kbd>{/if}
  {#if deeper}<kbd class="deeper">→</kbd>{/if}
</button>

<style>
  /* 自带按钮重置：这个组件要能落在任何一页上，不能指望宿主先铺好基础样式。 */
  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2) var(--s3);
    border: none;
    border-radius: var(--r1);
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row.on {
    color: var(--ink);
    background: var(--tint-2);
  }

  .thumb {
    display: block;
    width: 26px;
    height: 26px;
    flex: none;
    overflow: hidden;
    border-radius: calc(var(--r1) * 0.8);
  }

  .glyph {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: calc(var(--r1) * 0.8);
    background: var(--tint-1);
    color: var(--accent);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .text strong {
    overflow: hidden;
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /*
   * 命中不换字重、不上底色：一行里出现两种粗细，会先被当成两个层级读，而它
   * 们是同一句话。只提一档颜色。
   */
  .text mark {
    background: none;
    color: var(--accent);
  }

  .text small {
    color: var(--ink-3);
    font-family: var(--mono);
    font-size: var(--t-micro);
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    min-width: 20px;
    height: 19px;
    padding: 0 5px;
    border-radius: 5px;
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
    color: var(--ink-3);
    font-family: var(--mono);
    font-size: 10px;
  }
</style>
