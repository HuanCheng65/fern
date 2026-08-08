<script lang="ts">
  /**
   * 直达的那块板子：一个输入行、一列结果、一条脚注。
   *
   * 只负责画。它不认识数据源，也不认识那个全局的 store——查询、结果、光标
   * 都由调用方给。产品里由 `CommandPalette` 接上 store 和键盘；官网要在页面上
   * 露一角，给它两行写死的结果就行，两边看到的是同一块板子。
   */
  import { CornerDownLeft, Search, X } from 'lucide-svelte'
  import PaletteRow from './PaletteRow.svelte'
  import { TYPE_LABEL, type Row, type Scope } from './palette.svelte'
  import type { Snippet } from 'svelte'

  interface Props {
    query: string
    rows: Row[]
    cursor?: number
    /** 下钻的落点。写在输入框里而不是标题上：它是这次查询的一部分。 */
    scope?: Scope | null
    /** 远端还在答。 */
    searching?: boolean
    /** 只看不动：官网上那块是静止的，不聚焦、不能打字。 */
    still?: boolean
    thumb?: Snippet<[string]>
    onquery?: (value: string) => void
    onkeydown?: (event: KeyboardEvent) => void
    onhover?: (index: number) => void
    onrun?: (row: Row) => void
    onback?: () => void
    list?: (node: HTMLElement) => void
  }

  let {
    query,
    rows,
    cursor = 0,
    scope = null,
    searching = false,
    still = false,
    thumb,
    onquery,
    onkeydown,
    onhover,
    onrun,
    onback,
    list
  }: Props = $props()

  let inputEl = $state<HTMLInputElement>()

  /*
   * 自己聚焦，不用 autofocus 属性。
   *
   * autofocus 只在元素**带着这个属性被插进文档**的那一刻算数，而且一份文档
   * 只认第一次；面板是开的时候才挂上去的，属性又是动态给的，两条都不占。
   * preventScroll 顺带解决另一件事：聚焦不该把页面拽到面板这里来。
   */
  $effect(() => {
    if (!still && inputEl) inputEl.focus({ preventScroll: true })
  })

  const placeholder = $derived(
    !scope
      ? '搜索实例与动作'
      : scope.kind === 'subjects'
        ? `选择${TYPE_LABEL[scope.type]}`
        : '选择一个操作'
  )

  /** 类型变化的地方插一条分隔线。组序由分数决定，不是固定的。 */
  function heading(index: number): string | undefined {
    const row = rows[index]
    if (!row) return undefined
    const label = row.kind === 'subject' ? TYPE_LABEL[row.subject.type] : '动作'
    const before = rows[index - 1]
    const previous = before
      ? before.kind === 'subject'
        ? TYPE_LABEL[before.subject.type]
        : '动作'
      : undefined
    return label === previous ? undefined : label
  }
</script>

<div class="head">
  <Search size={17} strokeWidth={1.8} />
  {#if scope}
    <button class="chip" onclick={onback}>
      {scope.label}
      <X size={11} strokeWidth={2.4} />
    </button>
  {/if}
  <input
    class="query"
    bind:this={inputEl}
    value={query}
    oninput={(event) => onquery?.(event.currentTarget.value)}
    {onkeydown}
    readonly={still}
    tabindex={still ? -1 : 0}
    spellcheck="false"
    {placeholder}
    aria-label="搜索实例与动作"
  />
  <kbd>esc</kbd>
</div>

{#if rows.length === 0 && !searching}
  <p class="none">没有匹配的结果</p>
{:else}
  <div class="list" use:list>
    {#each rows as row, index (row.key)}
      {@const label = heading(index)}
      {#if label}<p class="group">{label}</p>{/if}
      <div data-row={index}>
        <PaletteRow
          {row}
          active={cursor === index}
          deeper={row.kind === 'subject' && cursor === index && !scope}
          {thumb}
          onhover={() => onhover?.(index)}
          onrun={() => onrun?.(row)}
        />
      </div>
    {/each}

    <!-- 远端还在答。它排在最后，也只在最后说话——已经画出来的行不会因为
         这一句而移动。 -->
    {#if searching}
      <p class="pending">正在搜索补给…</p>
    {/if}
  </div>
{/if}

<footer>
  <span><kbd>↑</kbd><kbd>↓</kbd>选择</span>
  <span><kbd><CornerDownLeft size={10} strokeWidth={2.4} /></kbd>执行</span>
  <span><kbd>→</kbd>可用操作</span>
</footer>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s4) var(--s5);
    color: var(--ink-3);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 3px var(--s2);
    border: none;
    border-radius: var(--r1);
    background: var(--tint-2);
    color: var(--ink-2);
    font: inherit;
    font-size: var(--t-small);
    cursor: pointer;
  }

  .chip:hover {
    color: var(--ink);
  }

  .query {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: none;
    color: var(--ink);
    font: inherit;
    font-size: var(--t-lead);
  }

  .query::placeholder {
    color: var(--ink-4);
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
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

  /* 滚动条自己带着，不指望宿主铺过基础样式 */
  .list {
    max-height: min(46vh, 420px);
    padding: var(--s2);
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
    scrollbar-color: var(--tint-3) transparent;
  }

  .list::-webkit-scrollbar {
    width: 8px;
  }

  .list::-webkit-scrollbar-thumb {
    border: 2px solid transparent;
    border-radius: 99px;
    background: var(--tint-2);
    background-clip: content-box;
  }

  .group {
    margin: var(--s3) var(--s3) var(--s1);
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .pending {
    margin: 0;
    padding: var(--s3);
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .none {
    margin: 0;
    padding: var(--s7) 0;
    color: var(--ink-3);
    font-size: var(--t-small);
    text-align: center;
  }

  footer {
    display: flex;
    gap: var(--s4);
    padding: var(--s2) var(--s5);
    box-shadow: inset 0 1px 0 var(--hairline-2);
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  footer span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
</style>
