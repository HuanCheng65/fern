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
  import CommandPalette, { type PaletteAction } from './components/CommandPalette.svelte'
  import CrashReport from './components/CrashReport.svelte'
  import GameLog from './components/GameLog.svelte'
  import WindowFrame from './components/WindowFrame.svelte'
  import LaunchScene from './scenes/Launch.svelte'
  import InstancesScene from './scenes/Instances.svelte'
  import Placeholder from './scenes/Placeholder.svelte'
  import SupplyScene from './scenes/Supply.svelte'
  import Setup from './routes/Setup.svelte'
  import Settings from './routes/Settings.svelte'
  import { frame, frameless, platform, selfRounded } from './lib/frame.svelte'
  import { flush, hydrate } from './lib/persist'
  import { instances } from './lib/instances.svelte'
  import { launch } from './lib/launch.svelte'
  import { nav, SCENES } from './lib/nav.svelte'
  import { prefs } from './lib/prefs.svelte'
  import { supply } from './lib/supply.svelte'
  import { theme } from './lib/theme.svelte'
  import './styles/tokens.css'

  let setupOpen = $state(false)
  /** 设置在磁盘上，读完才知道该不该出向导。读完之前只铺背景。 */
  let ready = $state(false)
  const isMac = platform === 'macos'
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

  async function openDirectory() {
    const current = instances.current
    if (!current) {
      createInstance()
      return
    }
    try {
      await invoke('open_instance_directory', { instanceId: current.id })
    } catch (error) {
      launch.error = String(error)
    }
  }

  /**
   * 命令面板是这套路由的键盘化身：每一条都是一次跳转或一个动作，和导航结构
   * 同构。
   */
  const actions = $derived<PaletteAction[]>([
    ...(instances.current
      ? [
          ...(launch.running
            ? []
            : [
                {
                  id: 'launch',
                  title: '启动当前实例',
                  hint: instances.current.name,
                  run: () => void launch.launch(instances.current!.id, prefs.playerName),
                },
              ]),
          { id: 'dir', title: '打开游戏目录', run: () => void openDirectory() },
          {
            id: 'repair',
            title: '校验游戏文件',
            run: () => void launch.repair(instances.current!.id),
          },
          {
            id: 'configure',
            title: '打开实例详情',
            hint: instances.current.name,
            run: () => nav.enter('instances', instances.current!.id),
          },
        ]
      : []),
    { id: 'create', title: '新建实例', run: createInstance },
    // 日志平时不该占地方，但出事的时候必须找得到——所以放在命令面板里，
    // 而且只在真的有内容时才列出来。
    ...(launch.log.length > 0
      ? [
          {
            id: 'log',
            title: '查看游戏日志',
            hint: `${launch.log.length} 行`,
            run: () => nav.show('log'),
          },
        ]
      : []),
    ...SCENES.filter((item) => item.id !== nav.scene).map((item) => ({
      id: `go-${item.id}`,
      title: `前往 ${item.label}`,
      run: () => nav.go(item.id),
    })),
    { id: 'settings', title: '打开设置', keys: isMac ? '⌘ ,' : 'Ctrl ,', run: () => nav.show('settings') },
  ])

  function onKeydown(event: KeyboardEvent) {
    const mod = event.metaKey || event.ctrlKey
    if (setupOpen) return
    if (mod && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      nav.toggle('palette')
      return
    }
    if (mod && event.key === ',') {
      event.preventDefault()
      nav.toggle('settings')
      return
    }
    if (event.key === 'Escape') {
      // 由外向内关：先收浮层，再退出详情。
      if (nav.overlay) nav.dismiss()
      else nav.back()
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
    void hydrate().then(() => {
      theme.hydrate()
      prefs.hydrate()
      setupOpen = !prefs.setupDone
      ready = true
    })
    void instances.load()
    void launch.connect()
    // 写盘是防抖的：改完设置立刻切走或关窗，别把最后那一下丢了。
    const saveNow = () => void flush()
    window.addEventListener('keydown', onKeydown)
    window.addEventListener('blur', saveNow)
    window.addEventListener('pagehide', saveNow)
    return () => {
      disconnectNav()
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('blur', saveNow)
      window.removeEventListener('pagehide', saveNow)
      launch.disconnect()
    }
  })

  const enter = $derived({
    x: nav.direction * 26,
    duration: Math.round(190 * theme.motionScale),
    opacity: 0,
  })
</script>

<svelte:head><title>Fern</title></svelte:head>

<div class="shell" class:rounded={selfRounded() && !frame.maximized}>
  <Backdrop {seed} particles={theme.particles} parallax={theme.parallax} {away} />

  {#if !ready}
    <!-- 背景已经在画了，这里只是等一次读盘，不额外放加载动画。 -->
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
            <LaunchScene onswitch={() => nav.show('palette')} oncreate={createInstance} />
          {:else if nav.scene === 'instances'}
            <InstancesScene />
          {:else if nav.scene === 'supply'}
            <SupplyScene />
          {:else if nav.scene === 'multiplayer'}
            <Placeholder
              seed="multiplayer"
              title="联机尚未开放"
              note="房间、好友与服务器列表将在此处提供。"
              onback={() => nav.go('launch')}
            />
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
        <Settings onback={() => nav.dismiss()} />
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

  {#if nav.overlay === 'palette'}
    <CommandPalette {actions} onclose={() => nav.dismiss()} />
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

  .stage {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    padding: var(--s2) var(--pad-x) var(--pad-b);
  }

  .scene {
    height: 100%;
    min-height: 0;
  }
</style>
