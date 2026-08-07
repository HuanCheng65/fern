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

  .row.stack {
    display: grid;
    justify-items: stretch;
    gap: var(--s3);
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

  .row :global(.choice) {
    flex: none;
    width: 210px;
  }

  @media (max-width: 720px) {
    .row {
      flex-direction: column;
      align-items: stretch;
      gap: var(--s3);
    }

    .row :global(.choice) {
      width: 100%;
    }
  }
</style>
