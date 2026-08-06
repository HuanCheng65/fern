<script lang="ts">
  /**
   * 顶栏（见 docs/UI_DESIGN.md 四）。
   *
   * 导航是一行纯文字，没有图标、没有侧栏——图标栏加列表-详情是 SaaS 的
   * 语言，文字导航是杂志的。当前场景实色，其余淡出，下面一条会滑动的
   * 短线告诉你镜头在哪一格。
   *
   * 设置和账户缩在右上角，不占场景位。
   */
  import { Search, Settings } from 'lucide-svelte'
  import { prefs } from '../lib/prefs.svelte'

  interface Props {
    scenes: { id: string; label: string }[]
    active: string
    isMac: boolean
    onselect: (id: string) => void
    oncommand: () => void
    onsettings: () => void
  }

  let { scenes, active, isMac, onselect, oncommand, onsettings }: Props = $props()

  let buttons: HTMLButtonElement[] = $state([])
  let marker = $state({ left: 0, width: 0 })

  // 短线跟着当前那个词走。位置从真实布局量，字宽随字体和语言变，算不出来。
  $effect(() => {
    const index = scenes.findIndex((item) => item.id === active)
    const el = buttons[index]
    if (el) marker = { left: el.offsetLeft, width: el.offsetWidth }
  })

  const initials = $derived((prefs.playerName || 'FERN').slice(0, 2).toUpperCase())
</script>

<header class="top" class:mac={isMac}>
  <div class="drag" data-tauri-drag-region aria-hidden="true"></div>

  <div class="mark" aria-hidden="true"></div>

  <nav aria-label="主导航">
    <span class="marker" style:transform={`translateX(${marker.left}px)`} style:width={`${marker.width}px`}></span>
    {#each scenes as item, index (item.id)}
      <button
        bind:this={buttons[index]}
        class:on={active === item.id}
        aria-current={active === item.id ? 'page' : undefined}
        onclick={() => onselect(item.id)}
      >
        {item.label}
      </button>
    {/each}
  </nav>

  <div class="right">
    <button class="btn command" onclick={oncommand} title="命令面板">
      <Search size={14} strokeWidth={1.9} />
      <kbd>{isMac ? '⌘' : 'Ctrl'} K</kbd>
    </button>
    <button class="btn btn--icon" aria-label="设置" title="设置" onclick={onsettings}>
      <Settings size={16} strokeWidth={1.8} />
    </button>
    <button class="avatar" onclick={onsettings} title={prefs.playerName || '设置账户'}>
      {initials}
    </button>
  </div>
</header>

<style>
  .top {
    position: relative;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: var(--s6);
    height: var(--top);
    padding: 0 var(--pad-x);
    flex: none;
  }

  /* macOS 的交通灯浮在内容上，给它让出位置。 */
  .top.mac {
    padding-left: 84px;
  }

  .drag {
    position: absolute;
    inset: 0;
  }

  .mark {
    position: relative;
    width: 12px;
    height: 12px;
    flex: none;
    border-radius: 3px;
    background: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-soft);
    transition: background var(--t-slow) var(--ease);
  }

  nav {
    position: relative;
    display: flex;
    gap: var(--s5);
  }

  nav button {
    position: relative;
    padding: 0;
    color: var(--ink-4);
    font-size: var(--t-body);
    font-weight: 500;
    transition: color var(--t-base) var(--ease);
  }

  nav button:hover {
    color: var(--ink-2);
  }

  nav button.on {
    color: var(--ink);
  }

  .marker {
    position: absolute;
    bottom: -8px;
    left: 0;
    height: 1.5px;
    border-radius: 2px;
    background: var(--accent);
    transition:
      transform var(--t-base) var(--spring),
      width var(--t-base) var(--spring);
  }

  .right {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--s1);
    margin-left: auto;
  }

  .command {
    min-height: 30px;
    padding: 0 var(--s2) 0 var(--s3);
    gap: var(--s2);
    color: var(--ink-4);
  }

  kbd {
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.02em;
  }

  .avatar {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin-left: var(--s2);
    border-radius: 50%;
    background: var(--tint-2);
    box-shadow: inset 0 0 0 1px var(--hairline);
    color: var(--ink-2);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.02em;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .avatar:hover {
    color: var(--ink);
    background: var(--tint-3);
  }

  @media (max-width: 720px) {
    .top {
      gap: var(--s4);
    }

    nav {
      gap: var(--s4);
      overflow: hidden;
    }

    .command kbd {
      display: none;
    }
  }
</style>
