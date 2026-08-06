<script lang="ts">
  /**
   * 无边框窗口自己要补的两样东西：拖拽改变大小的边，和三个窗口按钮。
   * 只在 Windows 和 Linux 出现——macOS 用系统的那一套（见 lib/frame.svelte.ts）。
   *
   * 按钮遵守平台惯例（右上角、最小化-最大化-关闭的顺序、关闭悬停变红、贴到
   * 窗口角落好让指针一甩就能命中），但笔画和留白用我们自己的——这两件事
   * 不冲突：用户认的是位置、顺序和红色，不是微软的字形。
   *
   * 移动窗口不在这里：那是 data-tauri-drag-region 的事，Tauri 自己接了
   * 拖拽和双击最大化，重复实现只会双击两次互相抵消。
   */
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { frame, selfRounded } from '../lib/frame.svelte'

  // @tauri-apps/api 没把 ResizeDirection 导出来，照着它的定义写一份。
  type ResizeDirection =
    | 'North'
    | 'NorthEast'
    | 'East'
    | 'SouthEast'
    | 'South'
    | 'SouthWest'
    | 'West'
    | 'NorthWest'

  /** 边 4px、角 12px：够得着，又不至于在靠边的按钮上抢走点击。 */
  const GRIPS: { direction: ResizeDirection; css: string }[] = [
    { direction: 'North', css: 'top: 0; left: 12px; right: 12px; height: 4px; cursor: ns-resize;' },
    { direction: 'South', css: 'bottom: 0; left: 12px; right: 12px; height: 4px; cursor: ns-resize;' },
    { direction: 'West', css: 'left: 0; top: 12px; bottom: 12px; width: 4px; cursor: ew-resize;' },
    { direction: 'East', css: 'right: 0; top: 12px; bottom: 12px; width: 4px; cursor: ew-resize;' },
    { direction: 'NorthWest', css: 'top: 0; left: 0; width: 12px; height: 12px; cursor: nwse-resize;' },
    { direction: 'NorthEast', css: 'top: 0; right: 0; width: 12px; height: 12px; cursor: nesw-resize;' },
    { direction: 'SouthWest', css: 'bottom: 0; left: 0; width: 12px; height: 12px; cursor: nesw-resize;' },
    { direction: 'SouthEast', css: 'bottom: 0; right: 0; width: 12px; height: 12px; cursor: nwse-resize;' },
  ]

  function grab(event: PointerEvent, direction: ResizeDirection) {
    if (event.button !== 0) return
    event.preventDefault()
    void getCurrentWindow().startResizeDragging(direction)
  }

  onMount(() => {
    let stop: (() => void) | undefined
    void frame.connect().then((cleanup) => (stop = cleanup))
    return () => stop?.()
  })
</script>

<!-- 最大化之后没有边可以拖，收起来免得在贴边的位置误触。 -->
{#if !frame.maximized}
  {#each GRIPS as grip (grip.direction)}
    <div
      class="grip"
      style={grip.css}
      aria-hidden="true"
      onpointerdown={(event) => grab(event, grip.direction)}
    ></div>
  {/each}
{/if}

<div class="controls" class:rounded={selfRounded() && !frame.maximized}>
  <button class="ctl" aria-label="最小化" title="最小化" onclick={() => frame.minimize()}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
  </button>
  <button
    class="ctl"
    aria-label={frame.maximized ? '向下还原' : '最大化'}
    title={frame.maximized ? '向下还原' : '最大化'}
    onclick={() => frame.toggleMaximize()}
  >
    <svg viewBox="0 0 10 10" aria-hidden="true">
      {#if frame.maximized}
        <path d="M2.5 2.5V0.5h7v7h-2" />
        <rect x="0.5" y="2.5" width="7" height="7" />
      {:else}
        <rect x="0.5" y="0.5" width="9" height="9" />
      {/if}
    </svg>
  </button>
  <button class="ctl close" aria-label="关闭" title="关闭" onclick={() => frame.close()}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg>
  </button>
</div>

<style>
  .grip {
    position: fixed;
    z-index: 65;
    /* 比浮层还高：面板开着的时候窗口一样要能拉大拉小。 */
  }

  .controls {
    position: fixed;
    top: 0;
    right: 0;
    /* 高过浮层——任何时候都要能关掉这个窗口。 */
    z-index: 70;
    display: flex;
  }

  .ctl {
    display: grid;
    place-items: center;
    width: 46px;
    height: var(--top);
    color: var(--ink-3);
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .ctl:hover {
    color: var(--ink);
    background: var(--tint-2);
  }

  /* 关闭变红是三个平台共同的语言，这一条不按我们的色板走。 */
  .ctl.close:hover {
    color: #fff;
    background: #c42b1c;
  }

  .rounded .ctl.close {
    border-top-right-radius: 10px;
  }

  svg {
    width: 10px;
    height: 10px;
    overflow: visible;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }
</style>
