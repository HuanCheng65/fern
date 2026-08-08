<script lang="ts">
  /**
   * 外壳。
   *
   * 这里只做四件事：把背景铺上、把顶栏和当前场景排好、接住全局快捷键、
   * 把浮层挂上去。场景自己的内容全部在 scenes/ 下，数据全部在 lib/ 的
   * store 里——外壳不认识实例，也不认识下载。
   *
   * 导航状态全部在 lib/nav.svelte.ts：场景层横向平移，场景内最多向内推一级，
   * 浮层盖在两者之上。外壳只负责把这三层画出来。
   */
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { fly } from 'svelte/transition'
  import Backdrop from './components/Backdrop.svelte'
  import TopBar from './components/TopBar.svelte'
  import CommandPalette from './components/CommandPalette.svelte'
  import Notices from './components/Notices.svelte'
  import Mark from './components/Mark.svelte'
  import CrashReport from './components/CrashReport.svelte'
  import GameLog from './components/GameLog.svelte'
  import WindowFrame from './components/WindowFrame.svelte'
  import LaunchScene from './scenes/Launch.svelte'
  import InstancesScene from './scenes/Instances.svelte'
  import Placeholder from './scenes/Placeholder.svelte'
  import MultiplayerScene from './scenes/Multiplayer.svelte'
  import SupplyScene from './scenes/Supply.svelte'
  import Setup from './routes/Setup.svelte'
  import Settings from './routes/Settings.svelte'
  import { frame, frameless, selfRounded } from './lib/frame.svelte'
  import { flush, hydrate } from './lib/persist'
  import { accounts } from './lib/accounts.svelte'
  import { instances } from './lib/instances.svelte'
  import { launch } from './lib/launch.svelte'
  import { DURATION, scaled } from './lib/motion'
  import { nav } from './lib/nav.svelte'
  import { palette } from 'fern-kit/palette'
  import './lib/places.svelte'
  import { prefs } from './lib/prefs.svelte'
  import { supply } from './lib/supply.svelte'
  import { theme } from './lib/theme.svelte'
  import { session } from './lib/pearl-session.svelte'
  import './styles/tokens.css'

  let setupOpen = $state(false)
  /** 设置在磁盘上，读完才知道该不该出向导。读完之前只铺背景。 */
  let ready = $state(false)
  /** 背景用当前实例的封面当种子——首页的背景就是这个实例自己的封面。 */
  const seed = $derived(instances.current?.cover ?? 'Fern')
  /** 顶栏的面包屑只要一个词。哪个场景有纵深，就由那个场景说它叫什么。 */
  const detailLabel = $derived(
    !nav.detail
      ? ''
      : nav.scene === 'instances'
        ? nav.detail === 'new'
          ? '新建实例'
          : (instances.list.find((item) => item.id === nav.detail)?.name ?? '')
        : nav.scene === 'supply'
          ? supply.viewingTitle || nav.detail
          : '',
  )

  const createInstance = () => nav.enter('instances', 'new')
  const away = $derived(nav.overlay !== '')

  // 联机昵称沿用当前账户的名字，Pearl 的配置只作为跨启动的兜底。
  $effect(() => {
    if (accounts.playerName.trim()) session.name = accounts.playerName.trim()
  })


  function onKeydown(event: KeyboardEvent) {
    const mod = event.metaKey || event.ctrlKey
    if (setupOpen) return
    if (mod && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      if (nav.overlay !== 'palette') palette.open()
      nav.toggle('palette')
      return
    }
    if (mod && event.key === ',') {
      event.preventDefault()
      nav.toggle('settings')
      return
    }
    if (event.key === 'Escape') {
      // 由外向内关：浮层内部还有更浅的一层就先退那一层（设置里的二级页），
      // 再收浮层，最后才退出详情。
      if (nav.overlay) {
        if (!nav.popFocus()) nav.dismiss()
      } else nav.back()
      return
    }
    // 左右方向键就是镜头。输入框里除外——那时候方向键属于光标。
    const tag = (event.target as HTMLElement | null)?.tagName
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return
    if (nav.overlay) return
    if (event.key === 'ArrowRight') nav.step(1)
    if (event.key === 'ArrowLeft') nav.step(-1)
  }

  onMount(() => {
    // 无边框时顶栏要给右上角的窗口按钮让出位置，用一个变量统一控制。
    document.body.classList.toggle('frameless', frameless())
    const disconnectNav = nav.connect()
    void hydrate().then(async (doc) => {
      theme.hydrate()
      prefs.hydrate()
      setupOpen = !prefs.setupDone
      ready = true
      // 0.1.0 的玩家名住在 localStorage 里，hydrate 才刚把它搬进 settings.json
      // ——而账户名册的迁移在那之前就跑过了，只会看到一份空的。补上这一步，
      // 否则那些用户会带着「已完成设置」但一个账户都没有的状态进来。
      await accounts.load()
      if (accounts.list.length === 0 && doc.account.playerName.trim()) {
        await accounts.addOffline(doc.account.playerName.trim())
      }
    })
    void instances.load()
    void session.loadName()
    void launch.connect()
    // 写盘是防抖的：改完设置立刻切走或关窗，别把最后那一下丢了。
    const saveNow = () => void flush()
    // 回到前台时对一次「谁在跑」：游戏可能在启动器被收起来的这段时间里退出了，
    // 而那条事件如果没收到，界面上会留下一个永远运行中的按钮。
    const resync = () => {
      if (!document.hidden) void launch.sync()
    }
    window.addEventListener('keydown', onKeydown)
    window.addEventListener('blur', saveNow)
    window.addEventListener('pagehide', saveNow)
    document.addEventListener('visibilitychange', resync)
    return () => {
      disconnectNav()
      document.removeEventListener('visibilitychange', resync)
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('blur', saveNow)
      window.removeEventListener('pagehide', saveNow)
      launch.disconnect()
    }
  })

  const enter = $derived({ x: nav.direction * 26, duration: scaled(DURATION.base), opacity: 0 })
</script>

<svelte:head><title>Fern</title></svelte:head>

<div class="shell" class:rounded={selfRounded() && !frame.maximized}>
  <Backdrop {seed} particles={theme.particles} parallax={theme.parallax} {away} />

  {#if !ready}
    <!--
      等一次读盘。背景层这时还没有色板可交，所以这一帧只有品牌自己的颜色：
      墨松底上的嫩芽。它同时是唯一一处「先于任何内容」的界面。
    -->
    <div class="boot">
      <Mark size={30} spinning />
    </div>
  {:else if setupOpen}
    <Setup
      ondone={(create) => {
        setupOpen = false
        nav.go('launch')
        if (create) createInstance()
      }}
    />
  {:else}
    <TopBar {detailLabel} />

    <main class="stage">
      {#key nav.scene}
        <div class="scene" in:fly={enter}>
          {#if nav.scene === 'launch'}
            <LaunchScene
              onswitch={() => {
                // 切换器就是面板，只是带着一枚锁定实例的 chip 进来。
                palette.open({ kind: 'subjects', type: 'instance', label: '实例' })
                nav.show('palette')
              }}
              oncreate={createInstance}
            />
          {:else if nav.scene === 'instances'}
            <InstancesScene />
          {:else if nav.scene === 'supply'}
            <SupplyScene />
          {:else if nav.scene === 'multiplayer'}
            <MultiplayerScene />
          {:else}
            <Placeholder
              seed="wardrobe"
              title="衣柜尚未开放"
              note="皮肤与披风的预览与切换将在此处提供。"
              onback={() => nav.go('launch')}
            />
          {/if}
        </div>
      {/key}

      <!-- 设置盖在舞台上，顶栏留在上面：它是浮层，不是第六个场景。 -->
      {#if nav.overlay === 'settings'}
        <Settings at={nav.focus} onback={() => nav.dismiss()} />
      {/if}
    </main>
  {/if}

  {#if frameless()}
    <WindowFrame />
    {#if selfRounded() && !frame.maximized}
      <!-- 自绘边框的那一道内描边。深色桌面上没有它，窗口边界会消失。 -->
      <div class="edge" aria-hidden="true"></div>
    {/if}
  {/if}

  <!--
    通知层挂在外壳上，不挂在任何一个场景里：它说的是「刚才那件事完成了」，
    而那件事完成的时候，用户很可能已经走到别的地方去了。
  -->
  <Notices />

  {#if nav.overlay === 'palette'}
    <!-- 只收面板自己：它执行的动作可能已经把人送到设置或日志去了。 -->
    <CommandPalette onclose={() => nav.dismiss('palette')} />
  {/if}

  <!-- 崩溃报告压在所有浮层之上：它是用户此刻唯一需要处理的事。 -->
  {#if launch.crash}
    <CrashReport
      report={launch.crash}
      onclose={() => launch.dismissCrash()}
      onopenLogs={() => void invoke('open_logs_directory')}
    />
  {/if}

  {#if nav.overlay === 'log'}
    <GameLog onclose={() => nav.dismiss()} />
  {/if}

</div>

<style>
  .shell {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100dvh;
    min-width: 320px;
    overflow: hidden;
    isolation: isolate;
  }

  /*
   * 自绘圆角。contain: paint 让 .shell 成为固定定位子元素的包含块，背景层
   * 那张 position: fixed 的画布才会被圆角裁掉——只靠 overflow: hidden 裁不住
   * 固定定位的东西。
   */
  .shell.rounded {
    border-radius: 10px;
    contain: paint;
  }

  .edge {
    position: absolute;
    inset: 0;
    z-index: 60;
    border-radius: inherit;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1);
    pointer-events: none;
  }

  /* 读盘那一帧。只有标志，没有别的。 */
  .boot {
    display: grid;
    place-items: center;
    flex: 1;
    color: var(--sprout);
  }

  .stage {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    padding: calc(var(--top) + var(--s2)) var(--pad-x) var(--pad-b);
  }

  .scene {
    height: 100%;
    min-height: 0;
  }
</style>
