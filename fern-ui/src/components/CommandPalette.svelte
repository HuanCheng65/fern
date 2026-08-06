<script lang="ts" module>
  export interface PaletteAction {
    id: string
    title: string
    hint?: string
    /** 显示在右边的快捷键。没有就不画，不要为了对齐编一个出来。 */
    keys?: string
    run: () => void
  }
</script>

<script lang="ts">
  /**
   * 命令面板（见 docs/UI_DESIGN.md 五）。
   *
   * 全局唯一：点实例名呼出的「切换器」和 ⌘K 呼出的面板是同一个东西，
   * 区别只是前者进来时光标已经落在实例这一组上。
   *
   * 定位是加速器，不是功能的藏身处——这里出现的每一个动作，界面上都有
   * 一个看得见的入口。所以它可以做得很薄。
   */
  import { ArrowRight, CornerDownLeft, Search } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import Cover from './Cover.svelte'
  import { instances } from '../lib/instances.svelte'

  interface Props {
    actions: PaletteAction[]
    onclose: () => void
  }

  let { actions, onclose }: Props = $props()

  let query = $state('')
  let cursor = $state(0)
  let listEl = $state<HTMLElement>()

  /**
   * 子序列匹配：输入的字符按顺序出现就算命中，允许中间跳过。
   * 「fabopt」能命中「Fabulously Optimized」，不用打全。
   */
  function matches(text: string, q: string) {
    if (!q) return true
    const haystack = text.toLowerCase()
    const needle = q.toLowerCase().replace(/\s+/g, '')
    let i = 0
    for (const ch of haystack) {
      if (ch === needle[i]) i++
      if (i === needle.length) return true
    }
    return false
  }

  type Row =
    | { kind: 'instance'; id: string; title: string; hint: string; run: () => void }
    | { kind: 'action'; id: string; title: string; hint?: string; keys?: string; run: () => void }

  const instanceRows = $derived<Row[]>(
    instances.list
      .filter((item) => matches(`${item.name} ${item.gameVersion} ${item.loader}`, query))
      .map((item) => ({
        kind: 'instance' as const,
        id: item.id,
        title: item.name,
        hint: `${item.gameVersion} · ${item.loader}`,
        run: () => instances.select(item.id),
      })),
  )

  const actionRows = $derived<Row[]>(
    actions
      .filter((item) => matches(`${item.title} ${item.hint ?? ''}`, query))
      .map((item) => ({ kind: 'action' as const, ...item })),
  )

  const rows = $derived([...instanceRows, ...actionRows])

  // 结果变了就把光标收回第一行，否则会停在一个已经不存在的位置上。
  $effect(() => {
    void query
    cursor = 0
  })

  function move(delta: number) {
    if (rows.length === 0) return
    cursor = (cursor + delta + rows.length) % rows.length
    listEl?.querySelector<HTMLElement>(`[data-row="${cursor}"]`)?.scrollIntoView({ block: 'nearest' })
  }

  function run(row: Row) {
    onclose()
    row.run()
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    }
    if (event.key === 'Enter' && rows[cursor]) {
      event.preventDefault()
      run(rows[cursor])
    }
  }
</script>

<Overlay label="命令面板" width="600px" align="top" {onclose}>
  <div class="head">
    <Search size={17} strokeWidth={1.8} />
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="query"
      bind:value={query}
      {onkeydown}
      autofocus
      spellcheck="false"
      placeholder="搜索实例与动作"
      aria-label="搜索实例与动作"
    />
    <kbd>esc</kbd>
  </div>

  {#if rows.length === 0}
    <p class="none">没有匹配的结果</p>
  {:else}
    <div class="list scroll" bind:this={listEl}>
      {#if instanceRows.length > 0}<p class="group">实例</p>{/if}
      {#each rows as row, index (row.kind + row.id)}
        {#if row.kind === 'action' && index === instanceRows.length}
          <p class="group">动作</p>
        {/if}
        <button
          class="row"
          data-row={index}
          class:on={cursor === index}
          onmouseenter={() => (cursor = index)}
          onclick={() => run(row)}
        >
          {#if row.kind === 'instance'}
            <span class="thumb"><Cover seed={row.title} quality={0.4} /></span>
          {:else}
            <span class="glyph"><ArrowRight size={14} strokeWidth={2} /></span>
          {/if}
          <span class="text">
            <strong>{row.title}</strong>
            {#if row.hint}<small>{row.hint}</small>{/if}
          </span>
          {#if row.kind === 'action' && row.keys}<kbd>{row.keys}</kbd>{/if}
        </button>
      {/each}
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
