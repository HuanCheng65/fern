<script lang="ts" generics="T extends string">
  /**
   * 下拉选择。替掉原生 `<select>`。
   *
   * 换掉它的理由不是想画得好看，是**原生的那一层根本不归我们管**：展开的菜单
   * 由系统绘制，CSS 只能改到 `option` 的前景/背景两个颜色，别的一概碰不到。
   * 结果是这套向背景学色彩的深色界面里，唯一一处颜色写死的地方就是它——而且
   * 三个平台的 webview（WebView2 / WKWebView / WebKitGTK）画出来还各不相同，
   * 「一套设计」在这里是断的。`scenes/NewInstance.svelte` 里早就为版本选择器
   * 写下过同一条结论。
   *
   * ## 为什么用 top layer
   *
   * 三个调用点全都待在会裁剪的容器里（浏览布局的结果区、详情布局、对话框），
   * 而外壳上还有一条 `contain: paint`（自绘圆角要靠它裁住背景画布）——那条
   * 会把 `position: fixed` 的包含块从视口改成外壳本身。所以普通的绝对或固定
   * 定位都靠不住，菜单要么被裁掉，要么锚错。
   *
   * `popover` 进的是 top layer，不受祖先的 overflow、contain、z-index 影响，
   * 这正是这个场景要的。位置仍然要自己算：CSS 锚点定位在 WebKit 上还没有，
   * 而 WKWebView 是 macOS 上唯一的选择。
   *
   * 用 `manual` 而不是 `auto`：`auto` 自带的轻点关闭和 Esc 在三个 webview 上
   * 的行为差异正是我们要躲开的东西，宁可自己接这两件事，跨平台是确定的。
   * 万一某个旧 webview 不认 `popover`，属性会被忽略、元素退回普通固定定位——
   * 位置照样是对的，只是可能被裁，不会整个坏掉。
   *
   * ## 焦点
   *
   * 焦点始终留在触发按钮上，用 `aria-activedescendant` 指向当前项——这是
   * combobox 的托管焦点模式。比把焦点搬进选项里简单得多，也不用在关闭时
   * 把焦点还回来。
   */
  import { ChevronDown } from 'lucide-svelte'
  import { tick } from 'svelte'

  interface Option {
    value: T
    label: string
  }

  interface Props {
    options: Option[]
    value: T
    onchange?: (value: T) => void
    /** 没有可见 label 时给一个无障碍名字。 */
    label?: string
    id?: string
    disabled?: boolean
    /**
     * bare 去掉边框和内边距，用在「排序」这类不该压过内容的地方——
     * 一个带边框的下拉会盖过它下面的卡片。
     */
    variant?: 'field' | 'bare'
  }

  let {
    options,
    value = $bindable(),
    onchange,
    label,
    id,
    disabled = false,
    variant = 'field',
  }: Props = $props()

  let open = $state(false)
  let active = $state(0)
  let trigger = $state<HTMLButtonElement>()
  let list = $state<HTMLDivElement>()
  /** 打字定位用的缓冲。原生 select 有这个行为，选项一多就靠它。 */
  let typed = ''
  let typedAt = 0

  const selected = $derived(options.find((option) => option.value === value))
  const listId = $derived(`${id ?? 'select'}-listbox`)
  const optionId = (index: number) => `${listId}-${index}`

  /** 视口边上留出的余量，别让菜单贴着窗口边缘。 */
  const EDGE = 8

  /** 菜单贴着触发器画。放不下就翻到上方，再放不下就压缩高度。 */
  function place() {
    if (!trigger || !list) return
    const box = trigger.getBoundingClientRect()
    const gap = 4
    const below = window.innerHeight - box.bottom - gap
    const above = box.top - gap
    const wanted = list.scrollHeight
    const up = below < Math.min(wanted, 200) && above > below

    list.style.minWidth = `${box.width}px`
    // 先量宽再定左边：bare 那一档的菜单常常比触发器宽（触发器只有当前那一项，
    // 菜单要容下最长的一项），贴着右边的排序下拉就会顶出视口。
    const width = list.offsetWidth
    const left = Math.max(EDGE, Math.min(box.left, window.innerWidth - EDGE - width))
    list.style.left = `${left}px`
    list.style.maxHeight = `${Math.min(wanted, (up ? above : below) - 4)}px`
    if (up) {
      list.style.top = ''
      list.style.bottom = `${window.innerHeight - box.top + gap}px`
    } else {
      list.style.bottom = ''
      list.style.top = `${box.bottom + gap}px`
    }
  }

  async function show() {
    if (disabled || open) return
    open = true
    active = Math.max(0, options.findIndex((option) => option.value === value))
    await tick()
    list?.showPopover?.()
    place()
    list?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: 'nearest' })
  }

  function hide() {
    if (!open) return
    open = false
    list?.hidePopover?.()
  }

  function pick(next: T) {
    value = next
    onchange?.(next)
    hide()
    trigger?.focus()
  }

  function move(delta: number) {
    if (options.length === 0) return
    active = (active + delta + options.length) % options.length
    tick().then(() =>
      list?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: 'nearest' }),
    )
  }

  function onKeydown(event: KeyboardEvent) {
    if (!open) {
      if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        void show()
      }
      return
    }
    switch (event.key) {
      case 'Escape':
        event.preventDefault()
        hide()
        return
      case 'ArrowDown':
        event.preventDefault()
        move(1)
        return
      case 'ArrowUp':
        event.preventDefault()
        move(-1)
        return
      case 'Home':
        event.preventDefault()
        active = 0
        return
      case 'End':
        event.preventDefault()
        active = options.length - 1
        return
      case 'Enter':
      case ' ':
        event.preventDefault()
        if (options[active]) pick(options[active].value)
        return
      case 'Tab':
        hide()
        return
    }
    // 打字定位：一秒内连续敲的字算同一串。
    if (event.key.length === 1) {
      const now = event.timeStamp
      typed = now - typedAt > 1000 ? event.key : typed + event.key
      typedAt = now
      const hit = options.findIndex((option) =>
        option.label.toLowerCase().startsWith(typed.toLowerCase()),
      )
      if (hit >= 0) {
        active = hit
        void tick().then(() =>
          list
            ?.querySelector<HTMLElement>('[data-active="true"]')
            ?.scrollIntoView({ block: 'nearest' }),
        )
      }
    }
  }

  /**
   * 开着的时候盯住外面。滚动用捕获阶段——菜单在 top layer 里，它不跟着任何
   * 一个滚动容器走，页面一滚它就会留在原地，看上去像飘在空中。
   */
  $effect(() => {
    if (!open) return
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node
      if (!trigger?.contains(target) && !list?.contains(target)) hide()
    }
    const reposition = () => place()
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('scroll', reposition, true)
    window.addEventListener('resize', reposition)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true)
      window.removeEventListener('scroll', reposition, true)
      window.removeEventListener('resize', reposition)
    }
  })
</script>

<button
  bind:this={trigger}
  {id}
  type="button"
  class="select"
  class:bare={variant === 'bare'}
  {disabled}
  role="combobox"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={listId}
  aria-activedescendant={open ? optionId(active) : undefined}
  aria-label={label}
  onclick={() => (open ? hide() : void show())}
  onkeydown={onKeydown}
>
  <span class="text">{selected?.label ?? ''}</span>
  <ChevronDown size={variant === 'bare' ? 13 : 15} strokeWidth={1.9} />
</button>

<div
  bind:this={list}
  id={listId}
  class="list"
  class:shown={open}
  popover="manual"
  role="listbox"
  aria-label={label}
  tabindex="-1"
>
  {#each options as option, index (option.value)}
    <!--
      键盘不在这一层：焦点始终留在触发按钮上，方向键和回车都由它接
      （combobox 的托管焦点模式）。所以这里的点击处理没有对应的键盘处理，
      不是漏了。tabindex 是 -1——选项不该进 Tab 序列，Tab 应该离开整个控件。
    -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={optionId(index)}
      class="option"
      role="option"
      tabindex="-1"
      aria-selected={option.value === value}
      data-active={index === active}
      onclick={() => pick(option.value)}
      onmousemove={() => (active = index)}
    >
      <span class="text">{option.label}</span>
    </div>
  {/each}
</div>

<style>
  .select {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
    width: 100%;
    min-height: 40px;
    padding: 0 var(--s3);
    border-radius: var(--r1);
    background: var(--well);
    box-shadow: inset 0 0 0 1px var(--hairline);
    color: var(--ink);
    font-family: var(--sans);
    font-size: var(--t-body);
    text-align: left;
    cursor: pointer;
    transition:
      box-shadow var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .select:hover {
    background: var(--well-2);
  }

  .select[aria-expanded='true'] {
    background: var(--well-3);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  .select:disabled {
    opacity: 0.4;
    pointer-events: none;
  }

  .select :global(svg) {
    flex: none;
    color: var(--ink-3);
    transition: transform var(--t-fast) var(--ease);
  }

  .select[aria-expanded='true'] :global(svg) {
    transform: rotate(180deg);
  }

  /* 不该压过内容的那一档：只剩字和一个箭头。 */
  .bare {
    width: auto;
    min-height: 0;
    padding: 0;
    border-radius: 0;
    background: none;
    box-shadow: none;
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  .bare:hover {
    background: none;
    color: var(--ink);
  }

  .bare[aria-expanded='true'] {
    background: none;
    box-shadow: none;
    color: var(--ink);
  }

  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /*
   * top layer 里的元素默认带 UA 的边框和内边距，且 `inset: auto` 之外的定位
   * 全部由上面的 place() 用行内样式给出——这里只负责长相。
   */
  .list {
    position: fixed;
    inset: auto;
    margin: 0;
    padding: var(--s1);
    border: none;
    border-radius: var(--r2);
    background: var(--panel);
    -webkit-backdrop-filter: blur(20px) saturate(1.3);
    backdrop-filter: blur(20px) saturate(1.3);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      var(--shadow);
    color: var(--ink);
    font-family: var(--sans);
    font-size: var(--t-body);
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
    scrollbar-color: var(--tint-3) transparent;
  }

  /* 不认 popover 的 webview 上，:popover-open 不会生效，靠这个类兜底。 */
  .list:not(.shown) {
    display: none;
  }

  .option {
    display: flex;
    align-items: center;
    min-height: var(--control);
    padding: 0 var(--s3);
    border-radius: var(--r1);
    color: var(--ink-2);
    cursor: pointer;
  }

  /* 键盘和鼠标共用同一条高亮——两套光标会让人不知道回车会选中哪一个。 */
  .option[data-active='true'] {
    background: var(--tint-2);
    color: var(--ink);
  }

  .option[aria-selected='true'] {
    color: var(--ink);
    font-weight: 500;
  }
</style>
