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
  import Mark from './Mark.svelte'
  import { host } from '../host.svelte'

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
    /**
     * 这颗按钮正在做它那件事。
     *
     * 隐含 disabled，但**不改标签**：把「清除」换成「正在清除……」会让按钮当场
     * 变宽，一排按钮跟着动，而且各处的省略号还各写各的。动的是标志和掠过表面的
     * 那道光——它们比一句话更能说明「还活着」，也更安静。
     */
    loading?: boolean
    /**
     * 0–1。给了就是一道跟着走的填充，不给就是一道自己走一趟的扫光。
     *
     * 不必和 `loading` 一起给：知道进度本来就意味着在忙。
     */
    progress?: number
    /** 布局用。见上面为什么调用方要配 `:global`。 */
    class?: string
    children?: Snippet
  }

  let {
    variant = 'default',
    tone = 'default',
    loading = false,
    progress,
    class: extra = '',
    // HTML 的默认值是 submit，但这套界面里绝大多数按钮不在表单里，
    // 一个漏写 type 的按钮会把最近的表单提交掉。真要提交的地方都显式写了。
    type = 'button',
    disabled,
    children,
    ...rest
  }: Props = $props()

  const solid = $derived(variant === 'default' || variant === 'primary')
  const working = $derived(loading || progress !== undefined)

  /**
   * 忙着，而且已经忙了一会儿。
   *
   * 拦住点击是立刻的事——连点两下会把同一件事做两遍。**画出来**要等 160ms：
   * 本地读盘和缓存命中往往几十毫秒就回来了，立刻亮一下再灭掉，那一闪比等待
   * 本身更烦人。和 Loading 那一处是同一条规矩。
   */
  const SETTLE = 160
  let shown = $state(false)
  $effect(() => {
    if (!working) {
      shown = false
      return
    }
    const timer = setTimeout(() => (shown = true), SETTLE)
    return () => clearTimeout(timer)
  })

  /** 标志多大。图标按钮里没有文字，跟着它自己的尺寸走。 */
  const markSize = $derived(variant === 'link' ? 12 : 14)
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
  class:busy={working}
  class:shown
  disabled={disabled || loading}
  aria-busy={working ? 'true' : undefined}
  style:--mark="{markSize}px"
  {...rest}
>
  <!--
    进度长在按钮内部，不另起一根条：这件事就是这颗按钮在做，它的位置就该是
    答案的位置。这套画法原本只长在启动键上（parts/LaunchHero.svelte），现在
    是所有按钮共有的。
  -->
  {#if working}
    <span
      class="fill"
      class:sweep={progress === undefined && host.motionScale > 0}
      style:width={progress === undefined ? '100%' : `${Math.min(1, Math.max(0, progress)) * 100}%`}
    ></span>
  {/if}
  <!--
    标志从零宽长出来，而不是「啪」地占一格。间距自己带着走，收起来时一并
    收掉——`gap` 对零宽的孩子照样生效，不这样处理就会留下一道空隙。

    只在忙的时候才挂上去：标志是 33 个 rect，而这个界面上同时存在几十颗按钮。
  -->
  {#if working}
    <span class="spinner" aria-hidden="true">
      <Mark size={markSize} spinning={shown} />
    </span>
  {/if}
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    /* 标志收起来时要靠一个负外边距把这道间距抵掉，所以两处必须是同一个数。 */
    --btn-gap: var(--s2);
    gap: var(--btn-gap);
    min-height: var(--control);
    padding: 0 var(--s4);
    border-radius: var(--r1);
    /*
     * 按钮永远不跟着周围变等宽。它可能落在一段 .t-mono 的说明里（启动屏那句
     * 「Minecraft 1.21.4 · NeoForge · 管理」就是），继承过去就成了机器数据的
     * 长相——而它是个动作。这一行之前是在调用方一处一处补的。
     */
    font-family: var(--sans);
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

  /* ── 忙 ── */

  /*
   * 忙着的按钮不是「不可用」，是「正在做」。压到 0.4 的灰会让它读起来像被
   * 关掉了，而它恰恰是这一刻唯一在动的东西。
   */
  .btn.busy:disabled {
    opacity: 1;
    cursor: progress;
  }

  /* 填充要盖在底色之上、文字之下，所以这一层容器只在忙的时候立起来。 */
  .btn.busy {
    position: relative;
    isolation: isolate;
    overflow: hidden;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    z-index: -1;
    /* 实心按钮上压暗，其余的用一层淡色垫底——透明底色上压黑什么也看不见。 */
    background: var(--tint-2);
    opacity: 0;
    transition:
      width var(--t-slow) var(--ease),
      opacity var(--t-base) var(--ease);
  }

  .btn--primary .fill,
  .danger-solid .fill {
    background: rgba(0, 0, 0, 0.24);
  }

  .btn.shown .fill {
    opacity: 1;
  }

  /* 进度未知时不停在 0%，让一道暗光自己走一趟。 */
  .fill.sweep {
    background: linear-gradient(90deg, transparent, var(--tint-3) 50%, transparent);
    animation: btn-sweep calc(1600ms * var(--motion, 1)) var(--ease) infinite;
  }

  .btn--primary .fill.sweep,
  .danger-solid .fill.sweep {
    background: linear-gradient(90deg, transparent, rgba(0, 0, 0, 0.26) 50%, transparent);
  }

  @keyframes btn-sweep {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(100%);
    }
  }

  /*
   * 标志占的宽度从 0 长到一格。间距连同宽度一起收，所以不忙的时候它对布局
   * 完全没有影响——包括那些从来不传 loading 的按钮。
   */
  .spinner {
    display: inline-flex;
    align-items: center;
    overflow: hidden;
    width: 0;
    margin-right: calc(var(--btn-gap) * -1);
    opacity: 0;
    transition:
      width var(--t-base) var(--ease),
      margin-right var(--t-base) var(--ease),
      opacity var(--t-fast) var(--ease);
  }

  .btn.shown .spinner {
    width: var(--mark);
    margin-right: 0;
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .fill.sweep {
      animation: none;
    }
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
    /* 前导箭头和字要贴得比实体按钮紧。四个调用方各自调过这个数（2/4/6px），
       说明它是系统该回答的，不是每处自己拿捏的。 */
    --btn-gap: var(--s1);
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
