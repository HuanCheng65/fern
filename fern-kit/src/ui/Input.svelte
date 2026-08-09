<script lang="ts">
  /**
   * 单行输入框。
   *
   * 大多数地方是「一个标签配一个输入框」，所以标签、说明、错误直接收成 prop，
   * 不必每处都写一层 `Field` 和一个 snippet。实现仍然只有一份：这里内部就是
   * `Field`，跟那些包着分段选择器、包着自定义块的地方用的是同一段逻辑。
   *
   * 三个 prop 都不给的时候（补给站的搜索框那种），它就是一个光秃秃的输入框，
   * 不会凭空多出一层栅格——但那时候要自己给 `aria-label`。
   */
  import type { HTMLInputAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'
  import Field from './Field.svelte'
  import type { ControlProps } from './field'

  interface Props extends Omit<HTMLInputAttributes, 'value'> {
    label?: string
    hint?: string
    error?: string
    /** 机器数据：版本号、路径、内存大小。等宽 + 表格数字，别拿它当装饰。 */
    mono?: boolean
    value?: string | number
    /** 布局用。和 Button 一样，调用方要配 `:global`，见那边的说明。 */
    class?: string
  }

  let {
    label,
    hint,
    error,
    mono = false,
    value = $bindable('' as string | number),
    // 单独接出来，不然 `{...rest}` 里的 class 会把 .input 顶掉。
    class: extra = '',
    ...rest
  }: Props = $props()

  const framed = $derived(Boolean(label || hint || error))
</script>

{#snippet field(props: Partial<ControlProps>)}
  <input class="input {extra}" class:mono bind:value {...props} {...rest} />
{/snippet}

{#if framed}
  <Field {label} {hint} {error} control={field as Snippet<[ControlProps]>} />
{:else}
  {@render field({})}
{/if}

<style>
  .input {
    width: 100%;
    min-height: 40px;
    padding: 0 var(--s3);
    border-radius: var(--r1);
    background: var(--well);
    box-shadow: inset 0 0 0 1px var(--hairline);
    color: var(--ink);
    font-family: var(--sans);
    font-size: var(--t-body);
    outline: none;
    cursor: text;
    transition:
      box-shadow var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .input::placeholder {
    color: var(--ink-4);
  }

  .input:hover {
    background: var(--well-2);
  }

  .input:focus {
    background: var(--well-3);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  /* 出了错就让边框自己说，不用再加一个图标。 */
  .input[aria-invalid='true'] {
    box-shadow: inset 0 0 0 1.5px var(--danger);
  }

  .mono {
    font-family: var(--mono);
    font-size: var(--t-small);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
</style>
