<script lang="ts" generics="T extends string">
  /**
   * 分段选择器：一排等宽的选项，选中的那格底下有块滑块滑过去。
   *
   * 这是这个应用里用得第二多的控件，仅次于按钮——界面密度、圆角、动效、
   * 强调色来源、下载源、更新通道、垃圾回收器、游戏窗口、进程优先级、版本类型、
   * 恢复范围，全是「N 选一」这个形状。
   *
   * 名字用惯例的那个（Mantine / Radix Themes / Chakra 都叫 SegmentedControl，
   * Ant 叫 Segmented），不自己造词。语义上它是 radiogroup，但 `RadioGroup`
   * 在惯例里指的是竖排那种传统单选列表，形状不一样。
   *
   * 选中态是一个绝对定位的滑块加 transform 平移，切换时是滑过去的——不是
   * 换一格背景色。位移能被眼睛跟住，闪一下不能。
   */
  import Field from './Field.svelte'
  import type { ControlProps } from './field'
  import type { Snippet } from 'svelte'

  interface Props {
    options: { value: T; label: string }[]
    value: T
    onchange: (value: T) => void
    /** 标签。给了就自动画出来并绑好 `for`——和 Input / Select 是同一个 Field。 */
    label?: string
    hint?: string
    error?: string
    /** 可见标签在别处时（设置里那一列由 SettingRow 提供）用这个给无障碍名。 */
    'aria-label'?: string
  }

  let {
    options,
    value,
    onchange,
    label,
    hint,
    error,
    'aria-label': ariaLabel,
  }: Props = $props()

  const framed = $derived(Boolean(label || hint || error))
  const index = $derived(Math.max(0, options.findIndex((option) => option.value === value)))
</script>

{#snippet control(props: Partial<ControlProps>)}
  <div
    id={props.id}
    class="segmented"
    role="radiogroup"
    aria-label={framed ? undefined : ariaLabel}
    aria-describedby={props['aria-describedby']}
    style:--count={options.length}
  >
    <div class="thumb" style:transform={`translateX(${index * 100}%)`}></div>
    {#each options as option (option.value)}
      <button
        type="button"
        role="radio"
        aria-checked={option.value === value}
        class:on={option.value === value}
        onclick={() => onchange(option.value)}
      >
        {option.label}
      </button>
    {/each}
  </div>
{/snippet}

{#if framed}
  <Field {label} {hint} {error} control={control as Snippet<[ControlProps]>} />
{:else}
  {@render control({})}
{/if}

<style>
  .segmented {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--count), minmax(0, 1fr));
    padding: 3px;
    border-radius: calc(var(--r1) + 3px);
    background: var(--tint-1);
  }

  .thumb {
    position: absolute;
    top: 3px;
    left: 3px;
    width: calc((100% - 6px) / var(--count));
    height: calc(100% - 6px);
    border-radius: var(--r1);
    background: var(--tint-3);
    transition: transform var(--t-base) var(--spring);
  }

  button {
    position: relative;
    min-height: 30px;
    padding: 0 var(--s2);
    border: none;
    border-radius: var(--r1);
    background: none;
    color: var(--ink-3);
    font-family: var(--sans);
    font-size: var(--t-small);
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--t-fast) var(--ease);
  }

  button:hover {
    color: var(--ink-2);
  }

  button.on {
    color: var(--ink);
    font-weight: 550;
  }
</style>
