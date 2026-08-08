<script lang="ts">
  /**
   * 补给里的一张卡片。
   *
   * 从补给场景里拆出来，因为这张卡不只属于那一页：官网要原样呈现它，拆开
   * 之后两边看到的是同一个组件，而不是一份仿制品。
   *
   * 它不认识数据源，也不认识那一页：给它一条结果，它负责画。点了之后发生
   * 什么由调用方决定——产品那边是进项目详情，官网那边什么都不做。
   */
  import Cover from './Cover.svelte'
  import { compactNumber, type Hit } from './supply'

  interface Props {
    hit: Hit
    /** 不给就是不能点：官网上这几张只是给人看的。 */
    onopen?: () => void
  }

  let { hit, onopen }: Props = $props()
</script>

<svelte:element
  this={onopen ? 'button' : 'div'}
  class="card"
  class:live={onopen}
  onclick={onopen}
  role={onopen ? 'button' : undefined}
  tabindex={onopen ? 0 : undefined}
>
  <span class="icon">
    {#if hit.iconUrl}
      <img src={hit.iconUrl} alt="" loading="lazy" />
    {:else}
      <!-- 没有图标的项目用生成式色块补位，网格才不会破相。 -->
      <Cover seed={hit.slug} quality={0.4} />
    {/if}
  </span>
  <span class="text">
    <strong>{hit.title}</strong>
    <small class="desc">{hit.description}</small>
    <small class="meta">{compactNumber(hit.downloads)} · {hit.author}</small>
  </span>
</svelte:element>

<style>
  /* 自带按钮重置：这张卡要能落在任何一页上，不能指望宿主先铺好基础样式。 */
  .card {
    display: flex;
    gap: var(--s3);
    width: 100%;
    padding: var(--s3);
    border: none;
    border-radius: var(--r2);
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .card.live {
    cursor: pointer;
  }

  .card.live:hover {
    background: var(--tint-1);
  }

  .icon {
    display: block;
    width: 46px;
    height: 46px;
    flex: none;
    overflow: hidden;
    border-radius: var(--r1);
    background: var(--tint-1);
  }

  .icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 描述压到两行：卡片高度一致，网格才立得住。 */
  .desc {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.5;
  }

  .meta {
    color: var(--ink-4);
    font-family: var(--mono);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
</style>
