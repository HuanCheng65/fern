<script lang="ts">
  /**
   * 外壳。
   *
   * 这里只做四件事：把背景铺上、把顶栏和当前场景排好、接住全局快捷键、
   * 管浮层的开关。场景自己的内容全部在 scenes/ 下，数据全部在 lib/ 的
   * store 里——外壳不认识实例，也不认识下载。
   *
   * 导航是横向舞台（见 docs/UI_DESIGN.md 四）：五个场景左右排开，切换时
   * 镜头横向平移。转场压在 200ms 以内是硬指标。
   */
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { fly } from 'svelte/transition'
  import Backdrop from './components/Backdrop.svelte'
  import TopBar from './components/TopBar.svelte'
  import CommandPalette, { type PaletteAction } from './components/CommandPalette.svelte'
  import CreateInstance from './components/CreateInstance.svelte'
  import CrashReport from './components/CrashReport.svelte'
  import InstanceSettings from './components/InstanceSettings.svelte'
  import GameLog from './components/GameLog.svelte'
  import WindowFrame from './components/WindowFrame.svelte'
  import LaunchScene from './scenes/Launch.svelte'
  import InstancesScene from './scenes/Instances.svelte'
  import Placeholder from './scenes/Placeholder.svelte'
  import Setup from './routes/Setup.svelte'
  import Settings from './routes/Settings.svelte'
  import { frame, frameless, selfRounded } from './lib/frame.svelte'
  import { flush, hydrate } from './lib/persist'
  import { instances } from './lib/instances.svelte'
  import { launch } from './lib/launch.svelte'
  import { prefs } from './lib/prefs.svelte'
  import { theme } from './lib/theme.svelte'
  import './styles/tokens.css'

  type SceneId = 'launch' | 'instances' | 'supply' | 'multiplayer' | 'wardrobe'

  const scenes: { id: SceneId; label: string }[] = [
    { id: 'launch', label: '启动' },
    { id: 'instances', label: '实例' },
    { id: 'supply', label: '补给' },
    { id: 'multiplayer', label: '联机' },
    { id: 'wardrobe', label: '衣柜' },
  ]

  let scene = $state<SceneId>('launch')
  let settingsOpen = $state(false)
  let paletteOpen = $state(false)
  let createOpen = $state(false)
  let instanceSettingsOpen = $state(false)
  let logOpen = $state(false)
  let setupOpen = $state(false)
  /** 设置在磁盘上，读完才知道该不该出向导。读完之前只铺背景。 */
  let ready = $state(false)
  let isMac = $state(false)
  /** 镜头往哪边走，决定新场景从哪一侧滑进来。 */
  let direction = $state(1)

  const overlayOpen = $derived(paletteOpen || createOpen || instanceSettingsOpen || logOpen)
  /** 背景用当前实例的名字当种子——首页的背景就是这个实例自己的封面。 */
  const seed = $derived(instances.current?.name ?? 'Fern')

  function goScene(id: SceneId) {
    const from = scenes.findIndex((item) => item.id === scene)
    const to = scenes.findIndex((item) => item.id === id)
    direction = to >= from ? 1 : -1
    scene = id
    settingsOpen = false
    location.hash = `#/${id}`
  }

  function step(delta: number) {
    const index = scenes.findIndex((item) => item.id === scene)
    goScene(scenes[(index + delta + scenes.length) % scenes.length]!.id)
  }

  function openSettings() {
    settingsOpen = true
    location.hash = '#/settings'
  }

  function closeSettings() {
    settingsOpen = false
    location.hash = `#/${scene}`
  }

  async function openDirectory() {
    const current = instances.current
    if (!current) {
      createOpen = true
      return
    }
    try {
      await invoke('open_instance_directory', { instanceId: current.id })
    } catch (error) {
      launch.error = String(error)
    }
  }

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
        ]
      : []),
    { id: 'create', title: '新建实例', run: () => (createOpen = true) },
    // 日志平时不该占地方，但出事的时候必须找得到——所以放在命令面板里，
    // 而且只在真的有内容时才列出来。
    ...(launch.log.length > 0
      ? [
          {
            id: 'log',
            title: '查看游戏日志',
            hint: `${launch.log.length} 行`,
            run: () => (logOpen = true),
          },
        ]
      : []),
    ...scenes
      .filter((item) => item.id !== scene)
      .map((item) => ({
        id: `go-${item.id}`,
        title: `前往 ${item.label}`,
        run: () => goScene(item.id),
      })),
    { id: 'settings', title: '打开设置', keys: isMac ? '⌘ ,' : 'Ctrl ,', run: openSettings },
  ])

  function readHash() {
    const raw = location.hash.replace(/^#\/?/, '')
    if (raw === 'settings') {
      settingsOpen = true
      return
    }
    settingsOpen = false
    if (scenes.some((item) => item.id === raw)) scene = raw as SceneId
  }

  function onKeydown(event: KeyboardEvent) {
    const mod = event.metaKey || event.ctrlKey
    if (setupOpen) return
    if (mod && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      paletteOpen = !paletteOpen
      return
    }
    if (mod && event.key === ',') {
      event.preventDefault()
      openSettings()
      return
    }
    if (event.key === 'Escape') {
      if (paletteOpen) paletteOpen = false
      else if (createOpen) createOpen = false
      else if (settingsOpen) closeSettings()
      return
    }
    // 左右方向键就是镜头。输入框里除外——那时候方向键属于光标。
    const tag = (event.target as HTMLElement | null)?.tagName
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return
    if (overlayOpen || settingsOpen) return
    if (event.key === 'ArrowRight') step(1)
    if (event.key === 'ArrowLeft') step(-1)
  }

  onMount(() => {
    isMac = /Mac/i.test(navigator.userAgent)
    // 无边框时顶栏要给右上角的窗口按钮让出位置，用一个变量统一控制。
    document.body.classList.toggle('frameless', frameless())
    readHash()
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
    window.addEventListener('hashchange', readHash)
    window.addEventListener('keydown', onKeydown)
    window.addEventListener('blur', saveNow)
    window.addEventListener('pagehide', saveNow)
    return () => {
      window.removeEventListener('hashchange', readHash)
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('blur', saveNow)
      window.removeEventListener('pagehide', saveNow)
      launch.disconnect()
    }
  })

  const enter = $derived({
    x: direction * 26,
    duration: Math.round(190 * theme.motionScale),
    opacity: 0,
  })
</script>

<svelte:head><title>Fern</title></svelte:head>

<div class="shell" class:rounded={selfRounded() && !frame.maximized}>
  <Backdrop
    {seed}
    particles={theme.particles}
    parallax={theme.parallax}
    away={overlayOpen || settingsOpen}
  />

  {#if !ready}
    <!-- 背景已经在画了，这里只是等一次读盘，不额外放加载动画。 -->
  {:else if setupOpen}
    <Setup
      ondone={(create) => {
        setupOpen = false
        goScene('launch')
        if (create) createOpen = true
      }}
    />
  {:else if settingsOpen}
    <Settings onback={closeSettings} />
  {:else}
    <TopBar
      {scenes}
      {isMac}
      active={scene}
      onselect={(id) => goScene(id as SceneId)}
      oncommand={() => (paletteOpen = true)}
      onsettings={openSettings}
    />

    <main class="stage">
      {#key scene}
        <div class="scene" in:fly={enter}>
          {#if scene === 'launch'}
            <LaunchScene
              onswitch={() => (paletteOpen = true)}
              oncreate={() => (createOpen = true)}
              onopenDirectory={() => void openDirectory()}
            />
          {:else if scene === 'instances'}
            <InstancesScene
              oncreate={() => (createOpen = true)}
              onopenDirectory={() => void openDirectory()}
              onconfigure={() => (instanceSettingsOpen = true)}
            />
          {:else if scene === 'supply'}
            <Placeholder
              seed="supply"
              title="资源站尚未接入"
              note="模组、整合包与资源包的搜索与安装将在此处提供。"
              onback={() => goScene('launch')}
            />
          {:else if scene === 'multiplayer'}
            <Placeholder
              seed="multiplayer"
              title="联机尚未开放"
              note="房间、好友与服务器列表将在此处提供。"
              onback={() => goScene('launch')}
            />
          {:else}
            <Placeholder
              seed="wardrobe"
              title="衣柜尚未开放"
              note="皮肤与披风的预览与切换将在此处提供。"
              onback={() => goScene('launch')}
            />
          {/if}
        </div>
      {/key}
    </main>
  {/if}

  {#if frameless()}
    <WindowFrame />
    {#if selfRounded() && !frame.maximized}
      <!-- 自绘边框的那一道内描边。深色桌面上没有它，窗口边界会消失。 -->
      <div class="edge" aria-hidden="true"></div>
    {/if}
  {/if}

  {#if paletteOpen}
    <CommandPalette {actions} onclose={() => (paletteOpen = false)} />
  {/if}

  <!-- 崩溃报告压在所有浮层之上：它是用户此刻唯一需要处理的事。 -->
  {#if launch.crash}
    <CrashReport
      report={launch.crash}
      onclose={() => launch.dismissCrash()}
      onopenLogs={() => void invoke('open_logs_directory')}
    />
  {/if}

  {#if instanceSettingsOpen && instances.current}
    <InstanceSettings
      instanceId={instances.current.id}
      instanceName={instances.current.name}
      onclose={() => (instanceSettingsOpen = false)}
    />
  {/if}

  {#if logOpen}
    <GameLog onclose={() => (logOpen = false)} />
  {/if}

  {#if createOpen}
    <CreateInstance onclose={() => (createOpen = false)} oncreated={() => goScene('launch')} />
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
