<script lang="ts">
  /**
   * 命令面板（见 docs/UI_DESIGN.md 五、十四）。
   *
   * 全局唯一：点实例名呼出的「切换器」和 ⌘K 呼出的面板是同一个东西，区别只是
   * 前者带着一枚锁定实例类型的 chip 进来。下钻（「校验哪个实例」）和预过滤
   * （「只看实例」）共用这一个机制，不写两套。
   *
   * 板子本身是隔壁的 `PalettePanel`，这里只做三件事：套上浮层、接上那个全局
   * store、收键盘。搜什么、怎么排、执行之后关不关，全在 ./palette.svelte 里
   * ——那是语法所在的地方。
   */
  import Dialog from '../ui/Dialog.svelte'
  import Cover from '../ui/Cover.svelte'
  import PalettePanel from './PalettePanel.svelte'
  import { palette, type Row } from './palette.svelte'

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
    /*
     * 「对它能做什么」。→ 和 ⌘K 是同一件事的两种手感：前者延续「往里走」的
     * 空间隐喻，后者是这类工具的通用记法。
     */
    // → 只在光标已经在末尾时才改变含义，否则它还是移动光标的那个键——
    // 一个输入框里的方向键首先属于输入框。
    const atEnd =
      event.currentTarget instanceof HTMLInputElement &&
      event.currentTarget.selectionStart === event.currentTarget.value.length &&
      event.currentTarget.selectionEnd === event.currentTarget.value.length
    const wantsActions =
      (event.key === 'ArrowRight' && atEnd) ||
      (event.key.toLowerCase() === 'k' && (event.metaKey || event.ctrlKey))
    if (wantsActions && rows[palette.cursor] && palette.askActions(rows[palette.cursor])) {
      event.preventDefault()
      event.stopPropagation()
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
</script>

<!-- 有种子的行画封面。行自己不认识封面，也不该认识：它只知道有个种子。 -->
{#snippet cover(seed: string)}<Cover {seed} quality={0.4} />{/snippet}

<Dialog label="命令面板" width="600px" align="top" {onclose}>
  <PalettePanel
    query={palette.query}
    {rows}
    cursor={palette.cursor}
    scope={palette.scope}
    searching={palette.searching}
    thumb={cover}
    onquery={(value) => (palette.query = value)}
    {onkeydown}
    onhover={(index) => (palette.cursor = index)}
    onrun={run}
    onback={() => palette.back()}
    list={(node) => (listEl = node)}
  />
</Dialog>
