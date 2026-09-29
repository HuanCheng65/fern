<script lang="ts">
  /**
   * 顶栏（见 docs/frond-design-system.md）。
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
    /** 0–1，按外部进度展开那座岛。见 Island 的 `unfold`。 */
    islandUnfold?: number
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
    islandUnfold,
    onisland,
    updateAvailable = false,
    onbrand,
    onscene,
    onback,
    onsettings,
  }: Props = $props()

  let header = $state<HTMLElement>()
  let brand = $state<HTMLElement>()
  let nav = $state<HTMLElement>()
  /**
   * 名字最多能占多宽：从品牌右边到五个词的左边。从真实布局量——字宽随字体
   * 和语言变，窗口宽度也在变。量不到时给一个保守值。
   */
  let room = $state(220)
  $effect(() => {
    if (!header) return
    const measure = () => {
      if (!brand || !nav) return
      const navLeft = nav.offsetLeft - nav.offsetWidth / 2
      const brandRight = brand.offsetLeft + brand.offsetWidth
      room = Math.max(0, navLeft - brandRight - 48)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(header)
    measure()
    return () => observer.disconnect()
  })

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
</script>

<!--
  deep：顶栏里任何空白处都能拖动窗口。Tauri 的规则是可点击元素会自己挡住
  拖拽，所以不用逐块开洞。
-->
<header
  bind:this={header}
  class="top"
  class:mac={mac}
  class:glass={scrolled}
  data-tauri-drag-region="deep"
>
  <button class="brand" bind:this={brand} onclick={() => onbrand?.()} aria-label="回到启动">
    <Mark size={18} />
    <!-- 字标：小写、650、字距 −1.5%（见 docs/fern-brand-system.html 04）。 -->
    <span class="word">fern</span>
  </button>

  <!--
    推入详情时，返回和名字长在左段，品牌的右边。整条是一颗返回按钮：箭头加
    名字，点哪里都是回去。

    它曾经长在当前场景词的两侧，名字压在其余四个词上面，于是那四个词只能退到
    9% 再模糊掉——进详情等于主导航消失了。放到左段之后五个词原样不动，当前那
    个照常亮着，说的正是「你在这个场景里面」。
  -->
  <button
    class="trail"
    class:on={inDetail}
    style:max-width={`${room}px`}
    tabindex={inDetail ? 0 : -1}
    aria-hidden={!inDetail}
    aria-label={inDetail ? `返回，当前：${held}` : undefined}
    onclick={() => onback?.()}
  >
    <ArrowLeft size={16} strokeWidth={2} />
    <span>{held}</span>
  </button>

  <nav aria-label="主导航" bind:this={nav}>
    {#each scenes as item (item.id)}
      <button
        class="scene"
        class:on={scene === item.id}
        aria-current={scene === item.id ? 'page' : undefined}
        onclick={() => onscene?.(item.id)}
      >
        {item.label}
      </button>
    {/each}

  </nav>

  <div class="right">
    <!--
      呼吸状态区：默认完全空白，有事情在发生时才浮现。零状态零挂件。它的意义
      是——切到任何场景，你都知道游戏还开着、东西还在下。顶栏不认识作业也不
      认识游戏，那是岛的事。
    -->
    <Island
      {presences}
      pinned={islandPinned}
      unfold={islandUnfold}
      ontoggle={() => onisland?.()}
    />

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
    /* 未选中也得读得出来：0.4 压在渐变上量出来只有 3 : 1。 */
    opacity: 0.6;
    transition:
      opacity var(--t-base) var(--ease),
      color var(--t-base) var(--ease);
  }

  .scene:hover {
    opacity: 0.85;
  }

  .scene.on {
    opacity: 1;
  }

  /*
   * 从左向右揭示，和详情页内容进场走同一条时长——顶栏的形态变化和内容的纵深
   * 变化要被感知为一件事。收起时名字留在原地被卷回去（见 `held`）。
   */
  .trail {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-width: 0;
    height: 32px;
    margin-left: var(--s5);
    padding: 0 var(--s2);
    border-radius: var(--r1);
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    white-space: nowrap;
    pointer-events: none;
    clip-path: inset(0 100% 0 0);
    transition:
      clip-path var(--t-slow) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .trail.on {
    pointer-events: auto;
    clip-path: inset(0 0 0 0);
  }

  .trail:hover {
    background: var(--tint-1);
  }

  .trail :global(svg) {
    flex: none;
    color: var(--ink-2);
  }

  .trail span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
