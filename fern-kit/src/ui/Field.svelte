<script lang="ts">
  /**
   * 一个带标签的控件。
   *
   * 它存在的理由不是那点栅格间距，是**把标签和控件对上号**这件事——今天这件事
   * 在二十处有四种写法：八处手写 `for`/`id`，六处用 `<label>` 直接包住控件，
   * 六处干脆没有标签，说明文字有时塞在 `<label>` 里的 `<small>` 里、有时是独立
   * 的一行灰字，错误文案则是两处各写各的 `.err`。
   *
   * 而 `<label>` 包住控件这条路**正在失效**：它对原生 `<input>` 有效，对我们
   * 自己的 `Select` 无效——那是一颗 `<button>`，`<label>` 跟 button 不建立关联。
   * 也就是说，那六处只要有一处把输入框换成下拉，标签就会静默失联。
   *
   * 所以 id 由这里生成、由这里绑。`$props.id()` 是框架给的，SSR 和 hydration
   * 两边一致——官网是预渲染的，自己搓计数器会在 hydration 时对不上号。
   *
   * 控件用 snippet 传进来，因为这二十处包的东西五花八门：输入框、下拉、分段
   * 选择器、一排按钮、纯自定义的块。Field 不该知道里面是什么，只负责把它需要
   * 的那几个属性递进去。
   */
  import type { Snippet } from 'svelte'
  import type { ControlProps } from './field'

  interface Props {
    /** 不给就不画标签。那时候控件自己要有 `aria-label`。 */
    label?: string
    /**
     * 说明。跟着标签走，画在控件**上方**——它是「填之前要知道的事」，
     * 放在下面就成了填完才看见的注解。
     */
    hint?: string
    /** 错误。画在控件下方：它是对你刚才输入的回应。 */
    error?: string
    control: Snippet<[ControlProps]>
  }

  let { label, hint, error, control }: Props = $props()

  const uid = $props.id()
  const hintId = `${uid}-hint`
  const errorId = `${uid}-error`

  const describedBy = $derived(
    [hint ? hintId : '', error ? errorId : ''].filter(Boolean).join(' ') || undefined,
  )
</script>

<div class="field">
  {#if label || hint}
    <label for={uid}>
      {label}
      {#if hint}<small id={hintId}>{hint}</small>{/if}
    </label>
  {/if}

  {@render control({
    id: uid,
    'aria-describedby': describedBy,
    'aria-invalid': error ? true : undefined,
  })}

  {#if error}<p id={errorId} class="error">{error}</p>{/if}
</div>

<style>
  .field {
    display: grid;
    gap: var(--s2);
  }

  label {
    display: grid;
    gap: 2px;
    font-size: var(--t-small);
    color: var(--ink-3);
  }

  small {
    font-size: var(--t-small);
    line-height: 1.6;
    color: var(--ink-4);
  }

  /* 错误要有颜色。灰色小字读起来像补充说明，而这一行是「刚才那下不行」。 */
  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
    line-height: 1.6;
  }
</style>
