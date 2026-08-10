<script lang="ts" generics="T extends string">
  /**
   * 动作菜单。一个按钮，按下去展开几条互斥的去路。
   *
   * 和 `Select` 的区别不是长相，是**它没有当前值**：选择器回答「现在是哪一个」，
   * 菜单回答「接下来做哪一件」。所以这里没有选中态、没有打字定位，选完就走。
   * 两者共用同一套 top layer 的定位机制（那部分的理由写在 `Select` 里，
   * 一句话：外壳上的 `contain: paint` 让普通的固定定位锚不住）。
   *
   * 用它替掉的是「点进去先看见一屏三选一」——那一屏没有内容，只有分岔，而分岔
   * 本来可以长在按钮上。
   *
   * 触发按钮的长相由调用方给（默认插槽），因为菜单不知道自己该多重：名单末尾
   * 那个「添加」是轻的，工具栏上的可能是实心的。
   */
  import { tick, type Snippet } from 'svelte'

  interface Item {
    value: T
    label: string
    /** 一句解释。选错的代价不小的时候写上——比如三种登录方式。 */
    note?: string
  }

  interface Props {
    items: Item[]
    onpick: (value: T) => void
    'aria-label': string
    disabled?: boolean
    /** 触发按钮的内容。 */
    children: Snippet
  }

  let { items, onpick, 'aria-label': ariaLabel, disabled = false, children }: Props = $props()

  let open = $state(false)
  let active = $state(0)
  let trigger = $state<HTMLButtonElement>()
  let list = $state<HTMLDivElement>()

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
    active = 0
    await tick()
    list?.showPopover?.()
    place()
  }

  function hide() {
    if (!open) return
    open = false
    list?.hidePopover?.()
  }

  function pick(value: T) {
    hide()
    trigger?.focus()
    onpick(value)
  }

  function move(delta: number) {
    if (items.length === 0) return
    active = (active + delta + items.length) % items.length
  }

  function onKeydown(event: KeyboardEvent) {
    if (!open) {
      if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
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
        active = items.length - 1
        return
      case 'Enter':
      case ' ': {
        event.preventDefault()
        const item = items[active]
        if (item) pick(item.value)
        return
      }
      case 'Tab':
        hide()
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

  const menuId = $props.id()
</script>

<button
  bind:this={trigger}
  type="button"
  class="trigger"
  {disabled}
  aria-label={ariaLabel}
  aria-haspopup="menu"
  aria-expanded={open}
  aria-controls={menuId}
  onclick={() => (open ? hide() : void show())}
  onkeydown={onKeydown}
>
  {@render children()}
</button>

<div
  bind:this={list}
  id={menuId}
  class="list"
  class:shown={open}
  popover="manual"
  role="menu"
  aria-label={ariaLabel}
  tabindex="-1"
>
  {#each items as item, index (item.value)}
    <!--
      键盘不在这一层：焦点始终留在触发按钮上，方向键和回车都由它接。所以这里
      的点击处理没有对应的键盘处理，不是漏了。
    -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="item"
      role="menuitem"
      tabindex="-1"
      data-active={index === active}
      onclick={() => pick(item.value)}
      onmousemove={() => (active = index)}
    >
      <strong>{item.label}</strong>
      {#if item.note}<small>{item.note}</small>{/if}
    </div>
  {/each}
</div>

<style>
  /*
   * 触发器默认是最轻的那一档实体按钮，和 Button 的 ghost 对齐——菜单常常挂在
   * 一份名单的末尾，那里不该出现第二颗主按钮。
   */
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    min-height: var(--control);
    padding: 0 var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
    color: var(--ink-2);
    font-family: var(--sans);
    font-size: var(--t-small);
    cursor: pointer;
    transition:
      color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .trigger:hover:not(:disabled) {
    background: var(--tint-2);
    color: var(--ink);
  }

  .trigger[aria-expanded='true'] {
    background: var(--tint-2);
    color: var(--ink);
  }

  .trigger:disabled {
    opacity: 0.4;
    pointer-events: none;
  }

  .trigger :global(svg) {
    flex: none;
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
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
    scrollbar-color: var(--tint-3) transparent;
  }

  /* 不认 popover 的 webview 上，:popover-open 不会生效，靠这个类兜底。 */
  .list:not(.shown) {
    display: none;
  }

  .item {
    display: grid;
    gap: 2px;
    max-width: 44ch;
    padding: var(--s2) var(--s3);
    border-radius: var(--r1);
    color: var(--ink-2);
    cursor: pointer;
  }

  /* 键盘和鼠标共用同一条高亮——两套光标会让人不知道回车会选中哪一个。 */
  .item[data-active='true'] {
    background: var(--tint-2);
    color: var(--ink);
  }

  .item strong {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .item small {
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.5;
  }
</style>
