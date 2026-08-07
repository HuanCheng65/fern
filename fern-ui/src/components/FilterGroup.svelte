<script lang="ts">
  /**
   * 浏览布局左栏里的一组筛选。
   *
   * 第三级重量（见 docs/UI_DESIGN.md 十）：12px，几乎退到背景里。选中态
   * 只用前景色，不给色块也不给边框——三级导航全部靠字重和透明度区分，和顶栏
   * 保持同一种安静。
   *
   * 纯文字加可选的数量徽标。一个筛选项长成按钮的样子，就会和它右边的结果
   * 争视觉重量。
   */
  interface Option {
    id: string
    label: string
    /** 有数字才显示。没有数据源就不要留这个位置。 */
    count?: number
  }

  interface Props {
    label: string
    value: string
    options: Option[]
    /** 给一个「不限」的入口。空串是它的值。 */
    anyLabel?: string
    onchange: (value: string) => void
  }

  let { label, value, options, anyLabel = '', onchange }: Props = $props()

  const all = $derived(anyLabel ? [{ id: '', label: anyLabel }, ...options] : options)
</script>

<section class="group">
  <h3 class="heading">{label}</h3>
  <div class="items">
    {#each all as option (option.id)}
      <button
        class="item"
        class:on={value === option.id}
        aria-pressed={value === option.id}
        onclick={() => onchange(option.id)}
      >
        <span class="text">{option.label}</span>
        {#if option.count !== undefined}<span class="t-mono count">{option.count}</span>{/if}
      </button>
    {/each}
  </div>
</section>

<style>
  .group {
    display: grid;
    gap: var(--s2);
    min-width: 0;
  }

  .heading {
    margin: 0;
    color: var(--ink);
    font-size: var(--t-micro);
    font-weight: 500;
    letter-spacing: 0.06em;
    opacity: 0.35;
  }

  .items {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
    padding: 3px 0;
    color: var(--ink);
    font-size: var(--t-small);
    text-align: left;
    opacity: 0.4;
    transition: opacity var(--t-fast) var(--ease);
  }

  .item:hover {
    opacity: 0.75;
  }

  .item.on {
    opacity: 1;
  }

  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex: none;
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
    opacity: 0.5;
  }

  /* 横过来收在搜索框下面时，一组就是一行。 */
  @media (max-width: 880px) {
    .items {
      grid-auto-flow: column;
      grid-auto-columns: max-content;
      gap: var(--s3);
    }
  }
</style>
