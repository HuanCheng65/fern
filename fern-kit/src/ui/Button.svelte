<script lang="ts">
  /**
   * 按钮。
   *
   * 从散在各处的 `.btn` 类收拢成组件，因为 class 管不了三样东西：写错
   * `btn--ghsot` 是静默失效、变体没有自动补全、以及最要命的——**同一个变体会
   * 被各自补出不同的实现**。收拢之前，「危险确认」在三个面板里有三份实现，其中
   * 两份硬编码了 `#c42b1c`，那是另一套设计系统的红。
   *
   * 形状（variant）和语气（tone）是两个维度，不要混成一个枚举：
   * 「实心的主按钮」和「这是个删除动作」是两件独立的事，一个说重量，一个说性质。
   *
   * 布局归调用方。按钮在栅格里站哪、外边距多少，是**它周围那块布局**的知识，
   * 不是按钮的——所以这里不收 margin 之类的 prop，调用方传 `class` 进来。
   * 但 Svelte 的作用域样式到不了组件内部，父组件那条 `.logs { … }` 会被当成
   * 未使用直接删掉（静默的，只有 svelte-check 会哼一声）。所以调用方要写成
   * 由自己拥有的祖先罩着的 `:global`：
   *
   *     .row :global(.logs) { align-self: flex-start }
   *
   * `.row` 带着父组件的哈希，所以不会漏到别人身上。
   */
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  interface Props extends HTMLButtonAttributes {
    /** 重量。default 是最轻的那档实体按钮，link 只有一个词。 */
    variant?: 'default' | 'primary' | 'ghost' | 'icon' | 'link'
    /**
     * 语气。
     *
     * quiet 比默认再淡一档，hover 才回到正常——用在「返回」「高级」这类
     * 随时都在、但不该抢注意力的动作上。
     *
     * danger 在实体按钮上是实心的红（那是**执行**删除的那一颗），在 ghost 和
     * link 上只是 hover 变红（那是**要求确认**的那一颗）。两者重量本来就不同。
     */
    tone?: 'default' | 'quiet' | 'danger'
    /** 布局用。见上面为什么调用方要配 `:global`。 */
    class?: string
    children?: Snippet
  }

  let {
    variant = 'default',
    tone = 'default',
    class: extra = '',
    // HTML 的默认值是 submit，但这套界面里绝大多数按钮不在表单里，
    // 一个漏写 type 的按钮会把最近的表单提交掉。真要提交的地方都显式写了。
    type = 'button',
    children,
    ...rest
  }: Props = $props()

  const solid = $derived(variant === 'default' || variant === 'primary')
</script>

<!--
  变体和语气都用 `class:` 指令，不拼进 class 字符串：Svelte 只对静态看得见的
  类名保留作用域样式，写成动态表达式它会把下面整段当成未使用直接删掉——静默的。

  `btn--*` 这几个名字是**对外的挂钩**：调用方需要定位按钮时写
  `.crumbs :global(.btn--link)`，所以它们不能改名。
-->
<button
  {type}
  class="btn {extra}"
  class:btn--primary={variant === 'primary'}
  class:btn--ghost={variant === 'ghost'}
  class:btn--icon={variant === 'icon'}
  class:btn--link={variant === 'link'}
  class:quiet={tone === 'quiet'}
  class:danger-solid={tone === 'danger' && solid}
  class:danger-hover={tone === 'danger' && !solid}
  {...rest}
>
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--s2);
    min-height: var(--control);
    padding: 0 var(--s4);
    border-radius: var(--r1);
    font-size: var(--t-body);
    font-weight: 500;
    color: var(--ink-2);
    white-space: nowrap;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      opacity var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .btn:hover {
    color: var(--ink);
    background: var(--tint-1);
  }

  .btn:active {
    transform: scale(0.985);
  }

  .btn:disabled {
    opacity: 0.4;
    pointer-events: none;
  }

  .btn--primary {
    min-height: var(--control-lg);
    padding: 0 var(--s5);
    color: var(--accent-ink);
    background: var(--accent);
    font-weight: 580;
    box-shadow: 0 6px 22px -8px var(--accent-soft);
  }

  .btn--primary:hover {
    color: var(--accent-ink);
    background: var(--accent);
    filter: brightness(1.06);
  }

  .btn--ghost {
    box-shadow: inset 0 0 0 1px var(--hairline);
  }

  .btn--ghost:hover {
    box-shadow: inset 0 0 0 1px var(--tint-3);
  }

  /* 只有一个图标的按钮：顶栏、关闭、加号。 */
  .btn--icon {
    min-height: 0;
    width: 30px;
    height: 30px;
    padding: 0;
    color: var(--ink-3);
    border-radius: var(--r1);
  }

  .btn--icon:hover {
    color: var(--ink);
    background: var(--tint-2);
  }

  /* 文字动作。没有背景，只有一个词和一个箭头。 */
  .btn--link {
    min-height: 0;
    padding: 0;
    font-size: var(--t-small);
    color: var(--accent);
  }

  .btn--link:hover {
    background: none;
    color: var(--ink);
  }

  /* ── 语气 ── */

  .quiet {
    color: var(--ink-3);
  }

  .quiet:hover {
    color: var(--ink);
  }

  /* 真的会删东西的那一颗，做成实心。 */
  .danger-solid {
    color: var(--on-danger);
    background: var(--danger);
    font-weight: 560;
  }

  .danger-solid:hover {
    color: var(--on-danger);
    background: var(--danger);
    filter: brightness(1.06);
  }

  /* 只是把确认叫出来的那一颗，别提前吓人，hover 才见红。 */
  .danger-hover:hover {
    color: var(--danger);
  }
</style>
