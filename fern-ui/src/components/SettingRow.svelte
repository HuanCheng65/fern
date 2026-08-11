<script lang="ts">
  /**
   * 设置页里的一行：一个名字，一个控件。
   *
   * 标题和说明从目录里取（lib/settings-catalog.ts），不写在这里——那样每加
   * 一项设置就要在标记和面板的清单里各写一遍同样的话，而两份说明迟早会说得
   * 不一样。这一行只负责排版和「被找到时亮一下」。
   */
  import type { Snippet } from 'svelte'
  import { settingsRow } from '../lib/settings-catalog'

  interface Props {
    id: string
    /** 被命令面板直接送到这一行时亮一下。 */
    found?: boolean
    /** 覆盖目录里的说明。只给那些说明本身是算出来的行用。 */
    note?: Snippet
    children: Snippet
  }

  let { id, found = false, note, children }: Props = $props()

  const row = $derived(settingsRow(id))
</script>

<div
  class="row"
  class:stack={row.stack}
  class:found
  data-setting={id}
>
  <span class="label">
    {row.label}
    {#if note}<small>{@render note()}</small>{:else if row.note}<small>{row.note}</small>{/if}
  </span>
  {@render children()}
</div>

<style>
  /* 每一行是「一个名字，一个控件」。说明文字只在没有它就会用错的地方出现。 */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s5);
    padding: var(--s4) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  /*
   * 标题在上、控件在下。
   *
   * 那一列必须写成显式的 `minmax(0, 1fr)`：隐式的 auto 轨道只在
   * `justify-content` 是 normal 或 stretch 时才铺开，而这一行从 `.row` 继承来
   * 的是 space-between，于是整行会缩到内容的宽度上——尺被挤成一小段、展开一
   * 张明细表就把整行撑宽。0 那一头同样是必要的：没有它，一个长实例名照样能把
   * 行顶出去。
   */
  .row.stack {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    justify-items: stretch;
    gap: var(--s3);
  }

  /* 自己占一行的按钮不跟着拉满：铺满整行的按钮读起来是一块横幅。 */
  .row.stack > :global(.btn) {
    justify-self: start;
  }

  .row:last-child {
    box-shadow: none;
  }

  /*
   * 从命令面板落到这一行时亮一下。
   *
   * 只是一层会退掉的底色，不是边框也不是选中态——它回答的是「你要找的在这
   * 里」，而那句话说完就该消失。整行铺开而不是描一圈，因为要指的是位置。
   */
  .row.found {
    animation: found 2.4s var(--ease) forwards;
  }

  @keyframes found {
    0%,
    55% {
      background: color-mix(in srgb, var(--accent) 14%, transparent);
      box-shadow: inset 0 -1px 0 var(--hairline-2);
    }
    100% {
      background: transparent;
      box-shadow: inset 0 -1px 0 var(--hairline-2);
    }
  }

  .label {
    display: grid;
    gap: 4px;
    font-size: var(--t-body);
    color: var(--ink);
  }

  .label small {
    max-width: 46ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.55;
  }

  /*
   * 所有分段控件同宽。
   *
   * 选项字数各不相同（「自动/G1/ZGC」和「由游戏决定/指定尺寸」），跟着内容
   * 走的话每一行的控件左边都停在不同的位置上，整节读起来就是一排参差的边。
   * 这里认的是组件真正的类名 `segmented`——上一版写的 `.choice` 不存在，那条
   * 规则一直没生效。
   */
  .row :global(.segmented) {
    flex: none;
    width: 210px;
  }

  @media (max-width: 720px) {
    .row {
      flex-direction: column;
      align-items: stretch;
      gap: var(--s3);
    }

    .row :global(.segmented) {
      width: 100%;
    }
  }
</style>
