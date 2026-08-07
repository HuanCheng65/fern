<script lang="ts">
  /**
   * 命令面板（见 docs/UI_DESIGN.md 五、十四）。
   *
   * 全局唯一：点实例名呼出的「切换器」和 ⌘K 呼出的面板是同一个东西，区别只是
   * 前者带着一枚锁定实例类型的 chip 进来。下钻（「校验哪个实例」）和预过滤
   * （「只看实例」）共用这一个机制，不写两套。
   *
   * 这里只负责画和收键盘。搜什么、怎么排、执行之后关不关，全在
   * lib/palette.svelte.ts 里——那是语法所在的地方。
   */
  import { ArrowRight, CornerDownLeft, Search, X } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import Cover from './Cover.svelte'
  import { palette, TYPE_LABEL, type Row } from '../lib/palette.svelte'

  interface Props {
    onclose: () => void
  }

  let { onclose }: Props = $props()

  let listEl = $state<HTMLElement>()

  const rows = $derived(palette.rows)

  // 结果变了就把光标收回第一行，否则它会停在一个已经不存在的位置上。
  $effect(() => {
    void palette.query
    void palette.scope
    palette.reset()
  })

  function scroll() {
    listEl
      ?.querySelector<HTMLElement>(`[data-row="${palette.cursor}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }

  function run(row: Row) {
    if (palette.run(row)) onclose()
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      palette.move(1)
      scroll()
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      palette.move(-1)
      scroll()
    }
    if (event.key === 'Enter' && rows[palette.cursor]) {
      event.preventDefault()
      run(rows[palette.cursor])
    }
    // 由外向内退：先摘 chip，再关面板。Overlay 自己也听 Esc，所以只有
    // 「还有 chip 可摘」时才拦下来。
    if (event.key === 'Escape' && palette.scope) {
      event.preventDefault()
      event.stopPropagation()
      palette.back()
    }
    // 退格退到底再按一下也是退出下钻，和面包屑的手感一致。
    if (event.key === 'Backspace' && !palette.query && palette.scope) {
      event.preventDefault()
      palette.back()
    }
  }

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

<Overlay label="命令面板" width="600px" align="top" {onclose}>
  <div class="head">
    <Search size={17} strokeWidth={1.8} />
    {#if palette.scope}
      <!-- 下钻的落点写在输入框里而不是标题上：它是这次查询的一部分。 -->
      <button class="chip" onclick={() => palette.back()}>
        {palette.scope.label}
        <X size={11} strokeWidth={2.4} />
      </button>
    {/if}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="query"
      bind:value={palette.query}
      {onkeydown}
      autofocus
      spellcheck="false"
      placeholder={palette.scope ? `选择${TYPE_LABEL[palette.scope.type]}` : '搜索实例与动作'}
      aria-label="搜索实例与动作"
    />
    <kbd>esc</kbd>
  </div>

  {#if rows.length === 0 && !palette.searching}
    <p class="none">没有匹配的结果</p>
  {:else}
    <div class="list scroll" bind:this={listEl}>
      {#each rows as row, index (row.key)}
        {@const label = heading(index)}
        {#if label}<p class="group">{label}</p>{/if}
        <button
          class="row"
          data-row={index}
          class:on={palette.cursor === index}
          onmouseenter={() => (palette.cursor = index)}
          onclick={() => run(row)}
        >
          {#if row.kind === 'subject' && row.subject.seed}
            <span class="thumb"><Cover seed={row.subject.seed} quality={0.4} /></span>
          {:else}
            <span class="glyph"><ArrowRight size={14} strokeWidth={2} /></span>
          {/if}
          <span class="text">
            <strong>{row.kind === 'subject' ? row.subject.title : row.action.title}</strong>
            {#if row.kind === 'subject' ? row.subject.hint : row.action.hint}
              <small>{row.kind === 'subject' ? row.subject.hint : row.action.hint}</small>
            {/if}
          </span>
          {#if row.kind === 'action' && row.action.keys}<kbd>{row.action.keys}</kbd>{/if}
        </button>
      {/each}

      <!-- 远端还在答。它排在最后，也只在最后说话——已经画出来的行不会因为
           这一句而移动。 -->
      {#if palette.searching}
        <p class="pending">正在搜索补给…</p>
      {/if}
    </div>
  {/if}

  <footer>
    <span><kbd>↑</kbd><kbd>↓</kbd>选择</span>
    <span><kbd><CornerDownLeft size={10} strokeWidth={2.4} /></kbd>执行</span>
  </footer>
</Overlay>

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
    border-radius: var(--r1);
    background: var(--tint-2);
    color: var(--ink-2);
    font-size: var(--t-small);
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

  .list {
    max-height: min(46vh, 420px);
    padding: var(--s2);
  }

  .group {
    margin: var(--s3) var(--s3) var(--s1);
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2) var(--s3);
    border-radius: var(--r1);
    color: var(--ink-2);
    text-align: left;
  }

  /* 选中态只有一个：键盘和鼠标共用它，不做两套高亮互相打架。 */
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

  .text small {
    color: var(--ink-3);
    font-family: var(--mono);
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
