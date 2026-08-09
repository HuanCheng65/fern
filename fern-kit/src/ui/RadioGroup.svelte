<script lang="ts" generics="T extends string">
  /**
   * 竖排单选：一列选项，每项一个标题加一句说明，选中的那个打勾。
   *
   * 和 `SegmentedControl` 是两个控件，不是两个名字。那个是横排连体、有滑块，
   * 用在「密度/圆角/动效」这种短词之间；这个是每项都要解释一句的时候用的
   * ——下载源、登录方式、用哪个 Java。主流库都同时提供两者（Radix Themes 的
   * `SegmentedControl` 和 `RadioGroup`、Mantine 的 `SegmentedControl` 和
   * `Radio.Group`），因为它们回答的不是同一个问题。
   *
   * 收进来的另一半理由是**这几处原来全是裸 `<button>`**：没有 `role="radio"`，
   * 键盘按方向键切不了，读屏也不知道这几个是一组互斥的选项。那不是能靠每处
   * 自己补的东西——它需要焦点管理，而焦点管理写错一次比没写更糟。
   *
   * 焦点用漫游 tabindex（ARIA 的单选组规范）：整组只有一个可 Tab 到的项，
   * 就是当前选中的那个；进组之后方向键在组内走，Tab 直接离开整组。这和
   * 原生 radio 的手感一致——用户不该为了跳过五个选项按五次 Tab。
   *
   * 方向键**移动即选中**，也是规范要求的：单选组里「聚焦但没选中」这个中间
   * 态对用户没有意义，反而会让人以为按了没反应。
   */
  import { Check } from 'lucide-svelte'
  import type { Snippet } from 'svelte'
  import Field from './Field.svelte'
  import type { ControlProps } from './field'

  interface Option {
    value: T
    label: string
    /** 一句话说清它是什么。这正是竖排的理由——横排放不下解释。 */
    note?: string
    /**
     * 贴在标题后面的小标记：「推荐」「尚未接入」。
     *
     * 放在标题旁而不是右边，因为右边留给对勾——标记说的是「这一项是什么」，
     * 对勾说的是「你选了它」，两件事不该抢同一个位置。
     */
    badge?: string
    /** quiet 是一行灰字（不可用之类），accent 是一枚嫩芽色胶囊（推荐）。 */
    badgeTone?: 'quiet' | 'accent'
    disabled?: boolean
  }

  interface Props {
    options: Option[]
    value: T | ''
    onchange: (value: T) => void
    label?: string
    hint?: string
    error?: string
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
  const enabled = $derived(options.filter((option) => !option.disabled))

  /**
   * 能被 Tab 到的那一个。选中的优先；一个都没选中时是第一个可用的——
   * 整组必须恰好有一个入口，否则要么 Tab 不进来，要么每一项都要按一次 Tab。
   */
  const tabbable = $derived(
    options.some((option) => option.value === value && !option.disabled)
      ? value
      : (enabled[0]?.value ?? ''),
  )

  let host = $state<HTMLDivElement>()

  function step(from: T | '', delta: number) {
    if (enabled.length === 0) return
    const at = enabled.findIndex((option) => option.value === from)
    const next = enabled[(at + delta + enabled.length) % enabled.length]!
    onchange(next.value)
    focus(next.value)
  }

  function focus(which: T) {
    host?.querySelector<HTMLElement>(`[data-value="${CSS.escape(which)}"]`)?.focus()
  }

  function onKeydown(event: KeyboardEvent, own: T) {
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowRight':
        event.preventDefault()
        step(own, 1)
        return
      case 'ArrowUp':
      case 'ArrowLeft':
        event.preventDefault()
        step(own, -1)
        return
      case 'Home':
        event.preventDefault()
        if (enabled[0]) {
          onchange(enabled[0].value)
          focus(enabled[0].value)
        }
        return
      case 'End':
        event.preventDefault()
        if (enabled.at(-1)) {
          onchange(enabled.at(-1)!.value)
          focus(enabled.at(-1)!.value)
        }
        return
      case ' ':
        event.preventDefault()
        onchange(own)
    }
  }
</script>

{#snippet control(props: Partial<ControlProps>)}
  <div
    bind:this={host}
    id={props.id}
    class="group"
    role="radiogroup"
    aria-label={framed ? undefined : ariaLabel}
    aria-describedby={props['aria-describedby']}
  >
    {#each options as option (option.value)}
      <button
        type="button"
        class="option"
        class:on={option.value === value}
        role="radio"
        aria-checked={option.value === value}
        data-value={option.value}
        disabled={option.disabled}
        tabindex={option.value === tabbable ? 0 : -1}
        onclick={() => onchange(option.value)}
        onkeydown={(event) => onKeydown(event, option.value)}
      >
        <span class="text">
          <strong>
            {option.label}
            {#if option.badge}
              <span class="badge" class:accent={option.badgeTone === 'accent'}>{option.badge}</span>
            {/if}
          </strong>
          {#if option.note}<small>{option.note}</small>{/if}
        </span>
        {#if option.value === value}
          <Check size={16} strokeWidth={2.4} />
        {/if}
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
  .group {
    display: grid;
    gap: var(--s2);
  }

  .option {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3) var(--s4);
    border: none;
    border-radius: var(--r2);
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px transparent;
    font-family: var(--sans);
    text-align: left;
    cursor: pointer;
    transition:
      background var(--t-fast) var(--ease),
      box-shadow var(--t-fast) var(--ease);
  }

  .option:hover:not(:disabled) {
    background: var(--tint-2);
  }

  /* 选中不是靠一个勾，是靠整块变亮加一道描边——勾只是确认。 */
  .option.on {
    background: var(--tint-2);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  .option:disabled {
    opacity: 0.42;
    cursor: default;
  }

  .option :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .text strong {
    display: flex;
    align-items: center;
    gap: var(--s2);
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 550;
  }

  .text small {
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.5;
  }

  .badge {
    flex: none;
    color: var(--ink-3);
    font-size: var(--t-micro);
    font-weight: 400;
    white-space: nowrap;
  }

  .badge.accent {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 10px;
    font-weight: 600;
  }
</style>
