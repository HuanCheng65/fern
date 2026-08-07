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
  import { ArrowLeft, ScrollText, Settings } from 'lucide-svelte'
  import Mark from './Mark.svelte'
  import { platform } from '../lib/frame.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav, SCENES } from '../lib/nav.svelte'
  import { prefs } from '../lib/prefs.svelte'

  interface Props {
    /** 当前详情的名字。顶栏不认识实例，只认识一个要显示的词。 */
    detailLabel?: string
  }

  let { detailLabel = '' }: Props = $props()

  let buttons: HTMLButtonElement[] = $state([])
  /** 面包屑要长在当前那个词上，位置从真实布局量——字宽随字体和语言变。 */
  let anchor = $state({ left: 0, right: 0 })

  const inDetail = $derived(nav.depth > 0 && detailLabel !== '')

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
  const isMac = $derived(platform === 'macos')
  const initials = $derived((prefs.playerName || 'FERN').slice(0, 2).toUpperCase())
  const busy = $derived(launch.busy || launch.running)

  $effect(() => {
    const el = buttons[nav.index]
    if (el) anchor = { left: el.offsetLeft, right: el.offsetLeft + el.offsetWidth }
  })
</script>

<!--
  deep：顶栏里任何空白处都能拖动窗口。Tauri 的规则是可点击元素会自己挡住
  拖拽，所以不用逐块开洞。
-->
<header
  class="top"
  class:mac={isMac}
  class:glass={nav.scrolled}
  data-tauri-drag-region="deep"
>
  <button class="brand" onclick={() => nav.go('launch')} title="回到启动">
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
      onclick={() => nav.back()}
    >
      <ArrowLeft size={14} strokeWidth={2} />
    </button>

    {#each SCENES as item, index (item.id)}
      <button
        bind:this={buttons[index]}
        class="scene"
        class:on={nav.scene === item.id}
        aria-current={nav.scene === item.id ? 'page' : undefined}
        onclick={() => nav.go(item.id)}
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
      呼吸状态区：默认完全空白，有下载或游戏在跑时才浮现。零状态零挂件。
      它的意义是——切到任何场景，你都知道游戏还开着。
    -->
    {#if busy}
      <button
        class="status"
        class:live={launch.running}
        onclick={() => nav.toggle('tasks')}
        title={launch.running ? '游戏运行中' : launch.label || '准备中'}
      >
        {#if launch.running}
          <span class="dot"></span>运行中
        {:else}
          <!--
            进度长在标志上：螺线画完即启动完成（见 docs/fern-brand-system.html
            06）。进度未知时它沿走线自己跑，不假装知道到了百分之几。
          -->
          <Mark
            size={14}
            spinning={launch.progress < 0}
            progress={launch.progress >= 0 ? launch.progress / 100 : undefined}
          />
          {launch.progress >= 0 ? `${Math.round(launch.progress)}%` : launch.label || '准备中'}
        {/if}
      </button>
    {/if}

    <!--
      ⌘K 不给搜索框样式的入口：那会立刻成为顶栏最重的元素。一个纯文字角标，
      和状态块同级。
    -->
    <button class="hint t-mono" onclick={() => nav.toggle('palette')} title="命令面板">
      {isMac ? '⌘K' : 'Ctrl K'}
    </button>

    <button
      class="btn btn--icon"
      aria-label="设置"
      title="设置"
      onclick={() => nav.toggle('settings')}
    >
      <Settings size={16} strokeWidth={1.8} />
    </button>

    <button
      class="avatar"
      onclick={() => nav.show('settings')}
      title={prefs.playerName || '设置账户'}
    >
      {initials}
    </button>
  </div>

  {#if nav.overlay === 'tasks'}
    <!-- 状态块点开的小面板。它讲的是全局进程，不属于任何场景。 -->
    <div class="tasks panel">
      {#if launch.running}
        <p class="line"><span class="dot"></span>游戏运行中</p>
        <button class="btn btn--link" onclick={() => nav.show('log')}>
          <ScrollText size={12} strokeWidth={2} />查看日志
        </button>
      {:else}
        <p class="line">{launch.label || '准备中'}</p>
        {#if launch.detail}<p class="t-quiet sub t-mono">{launch.detail}</p>{/if}
        {#if launch.progress >= 0}
          <div class="bar"><span style:width={`${launch.progress}%`}></span></div>
        {/if}
      {/if}
    </div>
  {/if}
</header>

<style>
  .top {
    position: relative;
    z-index: 10;
    display: flex;
    align-items: center;
    height: var(--top);
    padding: 0 calc(var(--pad-x) + var(--frame-controls)) 0 var(--pad-x);
    flex: none;
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
   */
  .top.glass {
    background: var(--panel);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    backdrop-filter: blur(26px) saturate(1.3);
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

  .status {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: 26px;
    margin-right: var(--s2);
    padding: 0 var(--s3);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-2);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
    transition: background var(--t-fast) var(--ease);
  }

  .status:hover {
    background: var(--tint-2);
    color: var(--ink);
  }



  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .hint {
    padding: 0 var(--s2);
    color: var(--ink);
    font-size: var(--t-micro);
    letter-spacing: 0.03em;
    opacity: 0.3;
    transition: opacity var(--t-fast) var(--ease);
  }

  .hint:hover {
    opacity: 0.8;
  }

  .avatar {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
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

  .tasks {
    position: absolute;
    top: calc(var(--top) - var(--s2));
    right: calc(var(--pad-x) + var(--frame-controls));
    z-index: 20;
    display: grid;
    gap: var(--s2);
    justify-items: start;
    width: 260px;
    padding: var(--s3) var(--s4);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin: 0;
    color: var(--ink);
    font-size: var(--t-small);
  }

  .sub {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .bar {
    width: 100%;
    height: 2px;
    border-radius: 2px;
    background: var(--tint-2);
  }

  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width var(--t-base) var(--ease);
  }

  /* 窗口窄到五个词和两侧要打架时，先牺牲词距。 */
  @media (max-width: 860px) {
    nav {
      gap: var(--s4);
    }
  }
</style>
