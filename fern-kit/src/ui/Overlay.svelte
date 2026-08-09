<script lang="ts">
  /**
   * 浮层的统一外壳：景深遮罩 + 一块面板（见 docs/UI_DESIGN.md 七）。
   *
   * 命令面板、新建实例、任何需要打断当前场景的东西都走这里，所以它们的
   * 圆角、玻璃、影子、进出方式只在这一个文件里定义过一次。
   *
   * 面板那身玻璃原来是基础样式里的 `.panel` 类，搬进来的时候一起搬了：整个
   * 项目里只有这里用它，而这个组件要能落在任何一页上，不该指望宿主先铺好
   * 一张样式表。
   */
  import { fade, scale } from 'svelte/transition'
  import { host } from '../host.svelte'

  interface Props {
    label: string
    /** 面板宽度。命令面板宽一些，表单窄一些。 */
    width?: string
    /** 命令面板贴上方（视线落点在上三分之一），表单居中。 */
    align?: 'top' | 'center'
    onclose: () => void
    children: import('svelte').Snippet
  }

  let { label, width = '440px', align = 'center', onclose, children }: Props = $props()

  const ms = (base: number) => Math.round(base * host.motionScale)
</script>

<div
  class="scrim"
  role="presentation"
  transition:fade={{ duration: ms(160) }}
  onclick={onclose}
></div>

<div class="dock" class:top={align === 'top'}>
  <div
    class="sheet"
    style:width
    role="dialog"
    aria-modal="true"
    aria-label={label}
    transition:scale={{ duration: ms(200), start: 0.97, opacity: 0 }}
  >
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(4, 6, 8, 0.5);
    -webkit-backdrop-filter: blur(6px);
    backdrop-filter: blur(6px);
  }

  /* 用一个不吃指针的容器做定位，面板自己只管大小——否则 transform 动画会和
     translate 定位打架。 */
  .dock {
    position: fixed;
    inset: 0;
    z-index: 41;
    display: grid;
    place-items: center;
    padding: var(--s6);
    pointer-events: none;
  }

  .dock.top {
    align-items: start;
    padding-top: 14vh;
  }

  .sheet {
    max-width: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    pointer-events: auto;
    border-radius: var(--r3);
    background: var(--panel);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      var(--shadow-lg);
    -webkit-backdrop-filter: blur(26px) saturate(1.3);
    backdrop-filter: blur(26px) saturate(1.3);
  }
</style>
