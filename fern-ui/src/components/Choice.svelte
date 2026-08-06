<script lang="ts" generics="T extends string">
  /**
   * 分段选择器。设置里的每一项个性化都是「三选一」这个形状——密度、圆角、
   * 动效——所以给它一个元件，而不是让每一项自己画一排按钮。
   *
   * 选中的滑块用一个绝对定位的块 + transform 平移，切换时是滑过去的。
   */
  interface Props {
    options: { value: T; label: string }[]
    value: T
    onchange: (value: T) => void
    label: string
  }

  let { options, value, onchange, label }: Props = $props()

  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)))
</script>

<div class="choice" role="radiogroup" aria-label={label} style:--count={options.length}>
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

<style>
  .choice {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--count), minmax(0, 1fr));
    padding: 3px;
    border-radius: calc(var(--r1) + 3px);
    background: rgba(255, 255, 255, 0.055);
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
    border-radius: var(--r1);
    color: var(--ink-3);
    font-size: var(--t-small);
    white-space: nowrap;
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
