<script lang="ts">
  /**
   * 顶栏（见 docs/UI_DESIGN.md 四）。
   *
   * 全应用唯一常驻的 UI，所以它的克制程度直接定义整个产品的气质。坚决单行：
   * 双行意味着导航自成一个区域、有自己的背景，那是网站页头的做法；桌面应用
   * 的顶栏该更接近菜单栏——一条安静的功能带。
   *
   * 三段：左边身份区（点回启动，等于一个永远存在的「回家」），中间五个场景词
   * **绝对居中于窗口**（居中于窗口而不是剩余空间，切场景时才纹丝不动），右边
   * 状态与工具区。
   *
   * 场景词纯文字不配图标：五个中文词本身就是最好的标识，加图标只会稀释排印。
   * 当前场景实色、其余淡出，不画下划线也不套胶囊——下划线太网页，胶囊太 SaaS。
   *
   * 背景不可控（启动场景整屏是封面艺术），所以顶栏默认完全透明，文字颜色跟着
   * 背景层提取的色板走；只有场景内容滚到它底下时才浮现毛玻璃。
   */
  import { ArrowLeft, Settings } from 'lucide-svelte'
  import Island from './Island.svelte'
  import Mark from '../ui/Mark.svelte'
  import Button from '../ui/Button.svelte'
  import type { Presence } from './island'

  interface Scene {
    id: string
    label: string
  }

  interface Props {
    /** 五个场景词。顺序就是它们的位置，位置不重排。 */
    scenes: Scene[]
    /** 当前在哪个场景。 */
    scene: string
    /** 推进详情的深度。大于 0 且有名字时，其余的词退到景深之外。 */
    depth?: number
    /** 当前详情的名字。顶栏不认识实例，只认识一个要显示的词。 */
    detailLabel?: string
    /** 内容滚到顶栏底下了没有——决定那层毛玻璃浮不浮现。 */
    scrolled?: boolean
    /** macOS 的交通灯浮在内容上，要给它让出安全区。 */
    mac?: boolean
    presences?: Presence[]
    islandPinned?: boolean
    onisland?: () => void
    /** 有新版本时在设置键上点一个点。 */
    updateAvailable?: boolean
    onbrand?: () => void
    onscene?: (id: string) => void
    onback?: () => void
    onsettings?: () => void
  }

  let {
    scenes,
    scene,
    depth = 0,
    detailLabel = '',
    scrolled = false,
    mac = false,
    presences = [],
    islandPinned = false,
    onisland,
    updateAvailable = false,
    onbrand,
    onscene,
    onback,
    onsettings,
  }: Props = $props()

  let buttons: HTMLButtonElement[] = $state([])
  /** 面包屑要长在当前那个词上，位置从真实布局量——字宽随字体和语言变。 */
  let anchor = $state({ left: 0, right: 0 })

  const inDetail = $derived(depth > 0 && detailLabel !== '')

  /**
   * 收起时名字要留在原地被卷走。
   *
   * 返回的那一刻 detailLabel 就空了，直接渲染它的话 clip-path 卷的是一段空
   * 文字——动画在跑，只是看不见。所以这里留住最后一个非空的名字，让它自己
   * 从右向左收回去。
   */
  let held = $state('')
  $effect(() => {
    if (detailLabel) held = detailLabel
  })
  const index = $derived(Math.max(0, scenes.findIndex((item) => item.id === scene)))
  $effect(() => {
    const el = buttons[index]
    if (el) anchor = { left: el.offsetLeft, right: el.offsetLeft + el.offsetWidth }
  })
</script>

<!--
  deep：顶栏里任何空白处都能拖动窗口。Tauri 的规则是可点击元素会自己挡住
  拖拽，所以不用逐块开洞。
-->
<header
  class="top"
  class:mac={mac}
  class:glass={scrolled}
  data-tauri-drag-region="deep"
>
  <button class="brand" onclick={() => onbrand?.()} title="回到启动">
    <Mark size={18} />
    <!-- 字标：小写、650、字距 −1.5%（见 docs/fern-brand-system.html 04）。 -->
    <span class="word">fern</span>
  </button>

  <nav aria-label="主导航" class:deep={inDetail}>
    <!--
      返回箭头长在当前场景词的左边，不替换整条导航——移动端那种「返回 + 标题」
      会让全局导航消失，用户失去空间感。
    -->
    <button
      class="back"
      class:on={inDetail}
      style:left={`${anchor.left - 22}px`}
      tabindex={inDetail ? 0 : -1}
      aria-hidden={!inDetail}
      aria-label="返回"
      onclick={() => onback?.()}
    >
      <ArrowLeft size={14} strokeWidth={2} />
    </button>

    {#each scenes as item, i (item.id)}
      <button
        bind:this={buttons[i]}
        class="scene"
        class:on={scene === item.id}
        aria-current={scene === item.id ? 'page' : undefined}
        onclick={() => onscene?.(item.id)}
      >
        {item.label}
      </button>
    {/each}

    <!--
      详情名从当前场景词右侧揭示。五个词的位置死死钉住不重排——位置稳定比
      对称重要，任何时候场景词都在肌肉记忆的位置上。名字压在已经退到 14%
      的词上面，所以它不吃指针事件，底下的词照样可点、可悬停恢复。
    -->
    <span class="crumb" class:on={inDetail} style:left={`${anchor.right + 14}px`}>
      {held}
    </span>
  </nav>

  <div class="right">
    <!--
      呼吸状态区：默认完全空白，有事情在发生时才浮现。零状态零挂件。它的意义
      是——切到任何场景，你都知道游戏还开着、东西还在下。顶栏不认识作业也不
      认识游戏，那是岛的事。
    -->
    <Island {presences} pinned={islandPinned} ontoggle={() => onisland?.()} />

    <!--
      有新版本时在这里点一个点。不弹窗、不横幅、不加一行文字——更新是启动器
      自己的事，而玩家在意的是游戏。这一点也是「有更新」在界面上唯一的痕迹。
    -->
    <Button
      variant="icon"
      class={updateAvailable ? 'marked' : ''}
      aria-label={updateAvailable ? '设置（有新版本）' : '设置'}
      title={updateAvailable ? '设置（有新版本）' : '设置'}
      onclick={() => onsettings?.()}
    >
      <Settings size={16} strokeWidth={1.8} />
    </Button>
  </div>

</header>

<style>
  .top {
    position: absolute;
    inset: 0 0 auto;
    z-index: 10;
    display: flex;
    align-items: center;
    width: 100%;
    /* --top / --pad-x / --frame-controls 是**窗口**的度量，留在产品的 app.css 里
       （官网没有窗口）。这里给回落值，好让这条顶栏落在别处也站得住。 */
    height: var(--top, 48px);
    padding: 0 calc(var(--pad-x, 24px) + var(--frame-controls, 0px)) 0 var(--pad-x, 24px);
    transition:
      background var(--t-base) var(--ease),
      box-shadow var(--t-base) var(--ease);
  }

  /* macOS 的交通灯浮在内容上，给它让出安全区。 */
  .top.mac {
    padding-left: 84px;
  }

  /*
   * 内容滚到顶栏底下才浮现毛玻璃。启动场景永不滚动，所以那里的顶栏永远是
   * 悬在封面上的一行纯文字。
   *
   * `-webkit-` 前缀不能省：WebKitGTK 只认带前缀的那个，Linux 和 macOS 上的
   * Tauri 跑的就是它。缺了它 backdrop-filter 整条失效，只剩底色——于是「毛
   * 玻璃」变成一块实色板子。
   */
  .top.glass {
    background: var(--top-glass);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    -webkit-backdrop-filter: blur(20px) saturate(1.2);
    backdrop-filter: blur(20px) saturate(1.2);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: none;
    padding: 0;
  }

  .brand :global(svg) {
    color: var(--accent);
    transition: color var(--t-slow) var(--ease);
  }

  .word {
    color: var(--ink-2);
    font-size: var(--t-body);
    font-weight: 650;
    letter-spacing: -0.015em;
    transition: color var(--t-fast) var(--ease);
  }

  .brand:hover .word {
    color: var(--ink);
  }

  /*
   * 绝对居中于窗口。左段宽度会因为 macOS 安全区变化、右段宽度会因为状态块
   * 出现而变化——居中于剩余空间的话，游戏一启动五个词就会整体挪一下。
   */
  nav {
    position: absolute;
    left: 50%;
    top: 0;
    display: flex;
    align-items: center;
    gap: var(--s6);
    height: 100%;
    transform: translateX(-50%);
  }

  .scene {
    padding: 0;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    opacity: 0.4;
    transition:
      opacity var(--t-base) var(--ease),
      filter var(--t-base) var(--ease),
      color var(--t-base) var(--ease);
  }

  .scene:hover {
    opacity: 0.75;
  }

  .scene.on {
    opacity: 1;
  }

  /*
   * 推入详情：其余四个词位置一个像素不动，只是退到景深之外。
   *
   * 只压暗不够——实例名压在它们上面是叠字，两层都是清晰的文字就会互相争。
   * 加一点失焦才真的分出前后：模糊过的那层不再被当成文字读，名字才立得住。
   */
  nav.deep .scene:not(.on) {
    opacity: 0.09;
    filter: blur(2.5px);
  }

  /* 悬停就回到焦点上——横跳能力保留，它们仍然可点。 */
  nav.deep .scene:not(.on):hover {
    opacity: 0.7;
    filter: none;
  }

  .back {
    position: absolute;
    display: grid;
    place-items: center;
    padding: 0;
    color: var(--ink);
    opacity: 0;
    transform: translateX(4px);
    pointer-events: none;
    transition:
      opacity var(--t-base) var(--ease),
      transform var(--t-base) var(--ease);
  }

  .back.on {
    opacity: 0.7;
    transform: none;
    pointer-events: auto;
  }

  .back.on:hover {
    opacity: 1;
  }

  /*
   * 从左向右揭示。和封面展开成横幅是同一个动作的两半，所以走同一条时长——
   * 顶栏的形态变化和内容的纵深变化要被感知为一件事。
   */
  .crumb {
    position: absolute;
    max-width: 26ch;
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-body);
    white-space: nowrap;
    text-overflow: ellipsis;
    pointer-events: none;
    clip-path: inset(0 100% 0 0);
    transition: clip-path var(--t-slow) var(--ease);
  }

  .crumb.on {
    clip-path: inset(0 0 0 0);
  }

  .right {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--s1);
    margin-left: auto;
  }


  /* 窗口窄到五个词和两侧要打架时，先牺牲词距。 */
  @media (max-width: 860px) {
    nav {
      gap: var(--s4);
    }
  }

  /*
   * 一个点，不是一个数字：数量在这里没有意义，有没有才有。
   * 布局归调用方，但 Svelte 的作用域样式进不了组件，所以罩一层自己的祖先。
   *
   * 画在 `::before` 上，不是 `::after`——设计系统用 `::after` 给每颗按钮撑最小点击区
   * （elements.css 里那条 `min-width: var(--hit)`）。两样东西抢同一个伪元素时，点的
   * `width: 6px` 压得过，`min-width` 压不过，于是这颗六像素的点是按 24px 画出来的。
   */
  .right :global(.marked) {
    position: relative;
  }

  .right :global(.marked)::before {
    content: '';
    position: absolute;
    top: 6px;
    right: 6px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent, currentColor);
  }
</style>
