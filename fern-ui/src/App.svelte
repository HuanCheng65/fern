<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import {
    ArrowRight,
    ChevronDown,
    Command,
    Download,
    FolderOpen,
    Gamepad2,
    Keyboard,
    Maximize2,
    Minus,
    Package,
    Play,
    Plus,
    Search,
    Settings2,
    Sparkles,
    X,
  } from 'lucide-svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import Router, { push, replace } from 'svelte-spa-router'
  import Backdrop from './components/Backdrop.svelte'
  import LandingRoute from './routes/Landing.svelte'
  import SettingsRoute from './routes/Settings.svelte'
  import WorkspaceRoute from './routes/Workspace.svelte'
  import './styles/tokens.css'

  type Scene = 'launch' | 'instances' | 'supply' | 'multiplayer' | 'wardrobe'
  type Instance = {
    id: string
    name: string
    version: string
    loader: string
    hours: string
    mods: number
    color: string
  }
  type DownloadEvent =
    | { type: 'status'; message: string }
    | { type: 'task_started'; total_files: number; total_bytes: number }
    | { type: 'file_done'; path: string; bytes: number }
    | { type: 'progress'; done_bytes: number; speed_bps: number }
    | { type: 'task_finished'; failed: string[] }
  type VersionOption = { id: string; kind: string; releaseTime: string; url: string }

  const scenes: { id: Scene; label: string }[] = [
    { id: 'launch', label: '启动' },
    { id: 'instances', label: '实例' },
    { id: 'supply', label: '补给' },
    { id: 'multiplayer', label: '联机' },
    { id: 'wardrobe', label: '衣柜' },
  ]
  const routes = {
    '/': LandingRoute,
    '/landing': LandingRoute,
    '/workspace': WorkspaceRoute,
    '/workspace/:scene': WorkspaceRoute,
    '/settings/*': SettingsRoute,
  }

  let instances: Instance[] = []

  const packs = [
    { name: 'Create: Astral', author: 'The Astral Team', version: '1.18.2', downloads: '2.4M', color: '#b4a278' },
    { name: 'Fabulously Optimized', author: 'Fabulously Optimized', version: '1.21.1', downloads: '8.7M', color: '#8ba5c0' },
    { name: 'Distant Horizons', author: 'Distant Horizons Team', version: '1.21.1', downloads: '1.2M', color: '#9a907f' },
    { name: 'Ad Astra', author: 'Terrarium', version: '1.20.1', downloads: '642K', color: '#7b8b9d' },
  ]

  let appName = 'Fern'
  let scene: Scene = 'launch'
  let selected = 0
  let commandOpen = false
  let settingsPage = false
  let landing = true
  let query = ''
  let supplyQuery = ''
  let isLaunching = false
  let launchProgress = 0
  let launchStatus = ''
  let launchError = ''
  let downloadTotalBytes = 0
  let showWidgetPicker = false
  let widgets: string[] = []
  let reducedEffects = false
  let dataRoot = ''
  let loadingInstances = true
  let instanceError = ''
  let createOpen = false
  let createName = ''
  let createVersion = ''
  let versions: VersionOption[] = []
  let versionsLoading = false
  let createError = ''
  let accountName = 'FernPlayer'

  const selectedInstance = () => instances[selected] ?? {
    id: 'empty', name: '还没有实例', version: '选择一个版本开始', loader: 'Vanilla', hours: '0 h', mods: 0, color: '#49616b',
  }
  const filteredInstances = () => instances.filter((item) => `${item.name}${item.version}${item.loader}`.toLowerCase().includes(query.toLowerCase()))
  const filteredPacks = () => packs.filter((pack) => `${pack.name}${pack.author}${pack.version}`.toLowerCase().includes(supplyQuery.toLowerCase()))
  const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  onMount(() => {
    let unlistenDownload: UnlistenFn | undefined
    const syncRoute = () => {
      const route = window.location.hash.replace(/^#/, '') || '/'
      landing = route === '/' || route === '/landing'
      settingsPage = route.startsWith('/settings')
      const routeScene = route.match(/^\/workspace\/([^/]+)/)?.[1] as Scene | undefined
      if (routeScene && scenes.some((item) => item.id === routeScene)) scene = routeScene
    }
    const syncSettings = (event: Event) => {
      const detail = (event as CustomEvent<{ accountName?: string; reducedEffects?: boolean }>).detail
      if (detail.accountName !== undefined) accountName = detail.accountName
      if (detail.reducedEffects !== undefined) reducedEffects = detail.reducedEffects
    }
    if (!window.location.hash) {
      void replace(localStorage.getItem('fern.landing.seen') === '1' ? '/workspace' : '/')
    }
    accountName = localStorage.getItem('fern.account.name') ?? accountName
    reducedEffects = localStorage.getItem('fern.effects.reduced') === '1'
    void invoke<string>('app_name').then((value) => (appName = value)).catch(() => undefined)
    void invoke<{ root: string }>('data_paths').then((paths) => {
      dataRoot = paths.root
      localStorage.setItem('fern.data.root', paths.root)
    }).catch(() => undefined)
    void loadInstances()
    syncRoute()
    window.addEventListener('hashchange', syncRoute)
    window.addEventListener('fern-settings-change', syncSettings)
    if ('__TAURI_INTERNALS__' in window) {
      void listen<DownloadEvent>('download-event', ({ payload }) => {
        if (payload.type === 'status') {
          launchStatus = payload.message
          launchProgress = Math.max(launchProgress, 3)
        }
        if (payload.type === 'task_started') {
          downloadTotalBytes = payload.total_bytes
          launchProgress = Math.max(launchProgress, 3)
          launchStatus = `检查 ${payload.total_files} 个文件`
        }
        if (payload.type === 'progress') {
          launchProgress = downloadTotalBytes > 0
            ? Math.min(99, Math.round((payload.done_bytes / downloadTotalBytes) * 100))
            : 0
          launchStatus = `${formatBytes(payload.done_bytes)} / ${formatBytes(downloadTotalBytes)} · ${formatBytes(payload.speed_bps)}/s`
        }
        if (payload.type === 'task_finished' && payload.failed.length > 0) {
          launchStatus = `${payload.failed.length} 个文件需要重试`
        }
      }).then((unlisten) => (unlistenDownload = unlisten))
    }
    const onKeydown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        commandOpen = !commandOpen
      }
      if (event.key === 'Escape') {
        commandOpen = false
        if (settingsPage) void push(`/workspace/${scene}`)
        showWidgetPicker = false
      }
      if (!landing && !commandOpen && !settingsPage && event.key === 'ArrowRight') moveScene(1)
      if (!landing && !commandOpen && !settingsPage && event.key === 'ArrowLeft') moveScene(-1)
    }
    window.addEventListener('keydown', onKeydown)
    return () => {
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('hashchange', syncRoute)
      window.removeEventListener('fern-settings-change', syncSettings)
      unlistenDownload?.()
    }
  })

  type CoreInstance = {
    id: string
    name: string
    gameVersion: string
    loader: string
    cover?: { identity: string }
  }

  function mapInstance(profile: CoreInstance): Instance {
    const color = profile.cover?.identity ? colorFor(profile.cover.identity) : '#49616b'
    return {
      id: profile.id,
      name: profile.name,
      version: profile.gameVersion,
      loader: profile.loader === 'neo_forge' ? 'NeoForge' : profile.loader === 'vanilla' ? 'Vanilla' : profile.loader,
      hours: '0 h',
      mods: 0,
      color,
    }
  }

  async function loadInstances() {
    loadingInstances = true
    instanceError = ''
    try {
      const profiles = await invoke<CoreInstance[]>('list_instances')
      instances = profiles.map(mapInstance)
      selected = Math.min(selected, Math.max(0, instances.length - 1))
    } catch (error) {
      if (inTauri()) {
        instanceError = String(error)
      } else {
        try {
          const stored = localStorage.getItem('fern.instances')
          instances = stored ? JSON.parse(stored) as Instance[] : []
          selected = Math.min(selected, Math.max(0, instances.length - 1))
        } catch (storageError) {
          instanceError = String(storageError)
        }
      }
    } finally {
      loadingInstances = false
    }
  }

  async function openCreate() {
    createOpen = true
    createError = ''
    if (versions.length > 0 || versionsLoading) return
    versionsLoading = true
    try {
      versions = inTauri()
        ? await invoke<VersionOption[]>('list_versions')
        : ((await fetch('https://piston-meta.mojang.com/mc/game/version_manifest_v2.json').then((response) => response.json())).versions as Array<{ id: string; type: string; releaseTime: string; url: string }>).map((version) => ({ id: version.id, kind: version.type, releaseTime: version.releaseTime, url: version.url }))
      createVersion = versions.find((version) => version.kind === 'release')?.id ?? versions[0]?.id ?? ''
    } catch (error) {
      createError = String(error)
    } finally {
      versionsLoading = false
    }
  }

  async function createNewInstance() {
    if (!createName.trim() || !createVersion) {
      createError = '填写实例名称并选择 Minecraft 版本'
      return
    }
    try {
      const next = inTauri()
        ? mapInstance(await invoke<CoreInstance>('create_instance', { name: createName, gameVersion: createVersion }))
        : {
            id: `browser-${Date.now()}`,
            name: createName.trim(),
            version: createVersion,
            loader: 'Vanilla',
            hours: '0 h',
            mods: 0,
            color: colorFor(createName),
          }
      instances = [...instances, next]
      if (!inTauri()) localStorage.setItem('fern.instances', JSON.stringify(instances))
      selected = instances.length - 1
      createName = ''
      createOpen = false
      scene = 'launch'
      void push('/workspace/launch')
    } catch (error) {
      createError = String(error)
    }
  }

  function colorFor(seed: string) {
    let hash = 0
    for (const char of seed) hash = (hash * 31 + char.charCodeAt(0)) >>> 0
    return ['#d47c51', '#79a77d', '#6d93b5', '#a18b6f'][hash % 4]
  }

  function moveScene(delta: number) {
    const index = scenes.findIndex((item) => item.id === scene)
    scene = scenes[(index + delta + scenes.length) % scenes.length].id
    void push(`/workspace/${scene}`)
  }

  async function startLaunch() {
    if (isLaunching) return
    if (instances.length === 0) {
      await openCreate()
      return
    }
    isLaunching = true
    launchError = ''
    launchProgress = 2
    launchStatus = '读取版本信息'
    if ('__TAURI_INTERNALS__' in window) {
      try {
        const result = await invoke<{ processId: number }>('launch_instance', {
          instanceId: selectedInstance().id,
          playerName: accountName,
        })
        launchProgress = 100
        launchStatus = `游戏已启动 · PID ${result.processId}`
        window.setTimeout(() => {
          isLaunching = false
          launchProgress = 0
          launchStatus = ''
        }, 1200)
      } catch (error) {
        launchError = String(error)
        launchStatus = '文件补全失败'
        isLaunching = false
      }
      return
    }
    simulateLaunch()
  }

  async function repairFiles() {
    if (isLaunching || instances.length === 0) return
    isLaunching = true
    launchError = ''
    launchProgress = 2
    launchStatus = '读取版本信息'
    try {
      await invoke('prepare_instance', { instanceId: selectedInstance().id })
      launchProgress = 100
      launchStatus = '文件校验完成'
    } catch (error) {
      launchError = String(error)
      launchStatus = '文件修复失败'
    } finally {
      isLaunching = false
    }
  }

  async function minimizeWindow() {
    if (inTauri()) await getCurrentWindow().minimize()
  }

  async function toggleMaximizeWindow() {
    if (inTauri()) await getCurrentWindow().toggleMaximize()
  }

  async function closeWindow() {
    if (inTauri()) await getCurrentWindow().close()
  }

  async function openGameDirectory() {
    if (instances.length === 0) {
      await openCreate()
      return
    }
    if (!inTauri()) {
      launchStatus = '浏览器预览无法打开本地游戏目录'
      return
    }
    try {
      await invoke('open_instance_directory', { instanceId: selectedInstance().id })
    } catch (error) {
      launchError = String(error)
    }
  }

  function simulateLaunch() {
    launchProgress = 8
    launchStatus = '浏览器预览 · 模拟文件补全'
    const timer = window.setInterval(() => {
      launchProgress = Math.min(100, launchProgress + 13)
      if (launchProgress >= 100) {
        window.clearInterval(timer)
        window.setTimeout(() => {
          isLaunching = false
          launchProgress = 0
          launchStatus = ''
        }, 700)
      }
    }, 320)
  }

  function formatBytes(bytes: number) {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`
    if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
    return `${(bytes / 1024 ** 3).toFixed(2)} GB`
  }

  function addWidget(name: string) {
    if (!widgets.includes(name)) widgets = [...widgets, name]
    showWidgetPicker = false
  }
</script>

<svelte:head>
  <title>{appName} · Minecraft launcher</title>
</svelte:head>

<div class:effects-off={reducedEffects} class:landing-mode={landing} class:settings-mode={settingsPage} class="app-shell">
  <Backdrop seed={selectedInstance().id} hours={selected + 1} particles={!reducedEffects} parallax={!reducedEffects} />
  <div class="scrim" aria-hidden="true"></div>
  <Router {routes} />

  <header class="topbar">
    <div class="drag-region" data-tauri-drag-region aria-hidden="true"></div>
    <div class="brand-mark" aria-hidden="true"><span></span></div>
    <nav aria-label="主导航">
      {#each scenes as item (item.id)}
        <button class:active={scene === item.id} aria-current={scene === item.id ? 'page' : undefined} onclick={() => { scene = item.id; void push(`/workspace/${item.id}`) }}>{item.label}</button>
      {/each}
    </nav>
    <div class="top-actions">
      <button class="quiet-button" aria-label="打开命令面板" title="命令面板 · ⌘K" onclick={() => (commandOpen = true)}>
        <Search size={16} strokeWidth={1.7} />
        <span class="shortcut"><Command size={11} />K</span>
      </button>
      <button class="quiet-button" aria-label="打开设置" title="设置" onclick={() => void push('/settings/general')}><Settings2 size={17} strokeWidth={1.7} /></button>
      <button class="account-button" aria-label="当前账户">{accountName.slice(0, 2).toUpperCase()}</button>
      {#if inTauri()}
        <div class="window-controls" aria-label="窗口控制">
          <button aria-label="最小化" title="最小化" onclick={minimizeWindow}><Minus size={15} /></button>
          <button aria-label="最大化" title="最大化" onclick={toggleMaximizeWindow}><Maximize2 size={13} /></button>
          <button class="window-close" aria-label="关闭" title="关闭" onclick={closeWindow}><X size={14} /></button>
        </div>
      {/if}
    </div>
  </header>

  <main class="stage">
    {#if scene === 'launch'}
      <section class="launch-scene" aria-label="启动">
        {#if instances.length === 0}
          <div class="first-run">
            <p class="eyebrow">Fern · 第一次打开</p>
            <h1>先创建一个实例</h1>
            <p>选择一个 Minecraft 版本，Fern 会把实例配置保存到本地，再按需补全游戏文件。</p>
            <button class="launch-button" onclick={openCreate}><Plus size={17} />创建实例</button>
            {#if loadingInstances}<span class="first-run-note">正在读取本地实例…</span>{:else if instanceError}<span class="first-run-error">{instanceError}</span>{/if}
          </div>
        {:else}
        <div class="launch-copy">
          <p class="eyebrow">正在播放 · {selectedInstance().loader}</p>
          <div class="title-row">
            <button class="instance-title" onclick={() => (commandOpen = true)} title="切换实例">{selectedInstance().name}<ChevronDown size={25} strokeWidth={1.55} /></button>
          </div>
          <p class="version-line">Minecraft {selectedInstance().version} <span>·</span> {selectedInstance().hours} <span>·</span> {selectedInstance().mods} 个模组</p>
          <button class:running={isLaunching} class="launch-button" onclick={startLaunch} disabled={isLaunching}>
            <span class="launch-fill" style={`width:${launchProgress}%`}></span>
            <span class="launch-label">{isLaunching ? `补全文件 ${launchProgress}%` : '启动游戏'}</span>
            {#if !isLaunching}<Play size={17} fill="currentColor" strokeWidth={1.4} />{:else}<Sparkles size={17} strokeWidth={1.5} />{/if}
          </button>
          <div class="launch-meta"><span class="status-dot"></span>{launchStatus || '离线账户 · 本地实例'}</div>
          {#if launchError}<div class="launch-error" role="alert">{launchError}</div>{/if}
        </div>

        {#if widgets.length === 0}
          <div class="widget-empty">
            <p>你的画面，从这里开始</p>
            <span>添加一个 HUD 组件，让常用信息浮在群系里。</span>
            <button class="text-action" onclick={() => (showWidgetPicker = true)}>添加组件 <ArrowRight size={14} /></button>
          </div>
        {:else}
          <div class="widget-grid" aria-label="HUD 组件">
            {#each widgets as widget (widget)}
              <article class="widget">
                <div class="widget-kicker">{widget === 'download' ? '补给进度' : widget === 'playtime' ? '本周游玩' : '服务器状态'}</div>
                <strong>{widget === 'download' ? '准备就绪' : widget === 'playtime' ? '12.7 h' : '3 个世界'}</strong>
                <span>{widget === 'download' ? '没有待处理任务' : widget === 'playtime' ? '较上周 +2.1 h' : '最近连接 · 2 分钟前'}</span>
              </article>
            {/each}
            <button class="widget-add" aria-label="添加组件" onclick={() => (showWidgetPicker = true)}><Plus size={18} /></button>
          </div>
        {/if}
        {/if}
      </section>
    {:else if scene === 'instances'}
      <section class="content-scene">
        <div class="section-heading"><div><p class="eyebrow">你的世界</p><h1>实例</h1></div><button class="outline-button" onclick={openCreate}><Plus size={15} />新建实例</button></div>
        <div class="instance-layout">
          <div class="instance-list">
            {#each instances as item, index (item.id)}
              <button class:active={selected === index} class="instance-row" onclick={() => (selected = index)}>
                <span class="mini-biome" style={`--swatch:${item.color}`}></span>
                <span class="row-copy"><strong>{item.name}</strong><small>{item.version} · {item.loader}</small></span>
                <span class="row-hours">{item.hours}</span>
              </button>
            {/each}
          </div>
          <div class="instance-detail">
            {#if instances.length === 0}
              <div class="detail-empty"><Package size={24} /><strong>还没有本地实例</strong><span>创建后会在这里显示版本、文件状态和修复入口。</span><button class="outline-button" onclick={openCreate}><Plus size={15} />创建实例</button></div>
            {:else}
            <div class="detail-top"><div><p class="eyebrow">当前实例</p><h2>{selectedInstance().name}</h2><span>{selectedInstance().version} · {selectedInstance().loader}</span></div><button class="launch-mini" onclick={startLaunch}><Play size={14} fill="currentColor" />启动</button></div>
            <div class="detail-stats"><div><small>游玩时长</small><strong>{selectedInstance().hours}</strong></div><div><small>模组数量</small><strong>{selectedInstance().mods}</strong></div><div><small>运行状态</small><strong>就绪</strong></div></div>
            <div class="detail-links"><button onclick={openGameDirectory}><FolderOpen size={15} />打开游戏目录</button><button><Package size={15} />管理内容</button><button onclick={repairFiles}><Download size={15} />修复文件</button></div>
            {/if}
          </div>
        </div>
      </section>
    {:else if scene === 'supply'}
      <section class="content-scene">
        <div class="section-heading"><div><p class="eyebrow">社区资源</p><h1>补给</h1></div><span class="section-note">Modrinth · 精选内容</span></div>
        <label class="search-field"><Search size={17} /><input bind:value={supplyQuery} placeholder="搜索整合包、模组、资源包" aria-label="搜索补给资源" /></label>
        {#if filteredPacks().length > 0}
          <div class="pack-grid">
            {#each filteredPacks() as pack (pack.name)}
              <article class="pack-card"><div class="pack-cover" style={`--pack-color:${pack.color}`}><span>群系精选</span><strong>{pack.name.split(':')[0]}</strong></div><div class="pack-body"><strong>{pack.name}</strong><small>{pack.author}</small><div><span>{pack.version}</span><span>{pack.downloads} 下载</span></div></div></article>
            {/each}
          </div>
        {:else}
          <div class="empty-state"><Search size={24} /><strong>没有匹配的资源</strong><span>换一个关键词，继续探索补给站。</span></div>
        {/if}
      </section>
    {:else}
      <section class="empty-scene"><div class="empty-glyph"><Gamepad2 size={29} strokeWidth={1.4} /></div><p class="eyebrow">{scenes.find((item) => item.id === scene)?.label}</p><h1>这片区域正在生长</h1><p>基础启动链路已经就绪，更多内容会在后续里程碑中进入世界。</p><button class="outline-button" onclick={() => { scene = 'launch'; void push('/workspace/launch') }}>回到启动 <ArrowRight size={15} /></button></section>
    {/if}
  </main>

  {#if createOpen}
    <div class="modal-scrim" role="presentation" onclick={() => (createOpen = false)}></div>
    <div class="create-panel" role="dialog" aria-modal="true" aria-label="新建实例">
      <div class="panel-heading"><div><p class="eyebrow">新世界</p><h2>创建实例</h2></div><button class="icon-only" aria-label="关闭" onclick={() => (createOpen = false)}><X size={17} /></button></div>
      <label class="form-field"><span>实例名称</span><input bind:value={createName} placeholder="例如：余烬谷" maxlength="64" /></label>
      <label class="form-field"><span>Minecraft 版本</span>
        {#if versionsLoading}<div class="form-loading">正在读取 Mojang 版本列表…</div>{:else}<select bind:value={createVersion} aria-label="选择 Minecraft 版本"><option value="" disabled>选择版本</option>{#each versions.filter((version) => version.kind === 'release').slice(0, 80) as version (version.id)}<option value={version.id}>{version.id}</option>{/each}</select>{/if}
      </label>
      {#if createError}<div class="form-error" role="alert">{createError}</div>{/if}
      <button class="launch-button create-submit" onclick={createNewInstance} disabled={versionsLoading}><Plus size={17} />保存实例</button>
      <p class="form-note">配置保存到本机数据目录，下载在启动或修复时开始。</p>
    </div>
  {/if}

  {#if showWidgetPicker}
    <div class="modal-scrim" role="presentation" onclick={() => (showWidgetPicker = false)}></div>
    <div class="widget-picker" role="dialog" aria-modal="true" aria-label="添加组件">
      <div class="panel-heading"><div><p class="eyebrow">HUD</p><h2>添加组件</h2></div><button class="icon-only" aria-label="关闭" onclick={() => (showWidgetPicker = false)}><X size={17} /></button></div>
      <button class="picker-row" onclick={() => addWidget('server')}><span class="picker-icon"><Gamepad2 size={17} /></span><span><strong>服务器状态</strong><small>查看最近连接的世界</small></span><Plus size={15} /></button>
      <button class="picker-row" onclick={() => addWidget('playtime')}><span class="picker-icon"><Sparkles size={17} /></span><span><strong>本周游玩</strong><small>追踪你的游玩节奏</small></span><Plus size={15} /></button>
      <button class="picker-row" onclick={() => addWidget('download')}><span class="picker-icon"><Download size={17} /></span><span><strong>补给进度</strong><small>下载任务完成情况</small></span><Plus size={15} /></button>
    </div>
  {/if}

  {#if commandOpen}
    <div class="modal-scrim" role="presentation" onclick={() => (commandOpen = false)}></div>
    <div class="command-panel" role="dialog" aria-modal="true" aria-label="命令面板">
      <label class="command-input"><Search size={18} /><input bind:value={query} placeholder="搜索实例、动作、设置" aria-label="搜索命令" /><kbd>ESC</kbd></label>
      <div class="command-list">
        <p class="command-group">实例</p>
        {#each filteredInstances() as item, index (item.id)}
          <button class="command-row" onclick={() => { selected = instances.indexOf(item); scene = 'launch'; commandOpen = false; void push('/workspace/launch') }}><span class="mini-biome" style={`--swatch:${item.color}`}></span><span><strong>{item.name}</strong><small>{item.version} · {item.loader}</small></span><ArrowRight size={15} /></button>
        {/each}
        <p class="command-group">动作</p>
        <button class="command-row" onclick={() => { commandOpen = false; startLaunch() }}><span class="command-icon"><Play size={15} fill="currentColor" /></span><span><strong>启动当前实例</strong><small>{selectedInstance().name}</small></span><kbd>↵</kbd></button>
        <button class="command-row" onclick={() => { commandOpen = false; void push('/settings/general') }}><span class="command-icon"><Settings2 size={15} /></span><span><strong>打开设置</strong><small>外观与性能</small></span><kbd>⌘,</kbd></button>
      </div>
      <footer class="command-footer"><span><Keyboard size={13} />上下选择</span><span><kbd>ESC</kbd>关闭</span></footer>
    </div>
  {/if}

</div>

<style>
  :global(html, body, #app) { margin: 0; height: 100%; overflow: hidden; }
  :global(body) { background: #07090b; color: var(--ink); font-family: var(--sans); font-size: var(--t-body); -webkit-font-smoothing: antialiased; user-select: none; }
  :global(button), :global(input) { font: inherit; color: inherit; }
  :global(button) { border: 0; background: none; cursor: pointer; }
  :global(:focus-visible) { outline: 1.5px solid var(--c4); outline-offset: 3px; }

  .app-shell { position: relative; min-width: 320px; height: 100dvh; overflow: hidden; isolation: isolate; border: 1px solid rgba(255,255,255,.12); border-radius: 18px; background: rgba(7,9,11,.18); }
  .landing-mode .topbar, .landing-mode .stage, .settings-mode .stage { visibility: hidden; pointer-events: none; }
  .app-shell :global(.backdrop), .app-shell :global(.backdrop-gl) { position: fixed; inset: 0; z-index: -3; }
  .scrim { position: fixed; inset: 0; z-index: -1; background: linear-gradient(90deg, rgba(5, 8, 9, .72), rgba(5, 8, 9, .2) 60%, rgba(5, 8, 9, .4)); pointer-events: none; }
  .effects-off :global(canvas) { opacity: .86; }
  .drag-region { position: absolute; inset: 0; }
  .topbar { position: fixed; inset: 0 0 auto; z-index: 10; height: var(--top); padding: 0 var(--pad-x); display: flex; align-items: center; gap: var(--s6); }
  .brand-mark { width: 19px; height: 19px; border-radius: 6px; flex: none; background: var(--c4); box-shadow: 0 0 0 4px rgba(255,255,255,.08); position: relative; z-index: 1; }
  .brand-mark span { position: absolute; inset: 5px; background: var(--c0); border-radius: 2px; opacity: .6; }
  nav { position: relative; z-index: 1; display: flex; gap: var(--s6); }
  nav button { padding: 3px 0; color: var(--ink-3); font-size: var(--t-lead); letter-spacing: .2em; transition: color var(--pan), transform var(--pan); }
  nav button:hover, nav button.active { color: var(--ink); }
  nav button:active, .quiet-button:active, .launch-button:active, .outline-button:active { transform: translateY(1px) scale(.98); }
  .top-actions { position: relative; z-index: 1; margin-left: auto; display: flex; align-items: center; gap: var(--s2); }
  .window-controls { display: flex; align-items: center; margin-left: var(--s3); border-left: 1px solid var(--line-2); padding-left: var(--s3); }
  .window-controls button { display: grid; place-items: center; width: 32px; height: 28px; color: var(--ink-3); transition: color var(--pan), background var(--pan); }
  .window-controls button:hover { color: var(--ink); background: rgba(255,255,255,.1); }
  .window-controls .window-close:hover { color: #fff; background: #a9564b; }
  .quiet-button, .account-button, .icon-only { display: grid; place-items: center; border-radius: var(--r1); color: var(--ink-2); transition: background var(--pan), color var(--pan), transform var(--pan); }
  .quiet-button { width: 34px; height: 34px; gap: 3px; }
  .quiet-button:hover, .account-button:hover, .icon-only:hover { color: var(--ink); background: rgba(255,255,255,.1); }
  .shortcut { display: inline-flex; align-items: center; font: 10px var(--mono); color: var(--ink-3); }
  .account-button { width: 32px; height: 32px; margin-left: var(--s2); border-radius: 10px; font: 11px var(--mono); color: var(--on-accent); background: var(--c4); }
  .stage { position: relative; z-index: 1; height: 100%; padding: var(--top) var(--pad-x) var(--s8); }
  .launch-scene, .content-scene { height: 100%; min-height: 0; display: flex; }
  .launch-scene { align-items: center; justify-content: space-between; gap: var(--s8); padding: 4vh 4vw 4vh 4vw; }
  .launch-copy { max-width: 560px; }
  .first-run { width: min(560px, 80vw); }
  .first-run h1 { margin: 0; font-size: clamp(40px, 6vw, 70px); letter-spacing: -.05em; }
  .first-run > p:not(.eyebrow) { max-width: 44ch; margin: var(--s5) 0 var(--s6); color: var(--ink-2); font: 13px/21px var(--mono); }
  .first-run-note, .first-run-error { display: block; margin-top: var(--s4); color: var(--ink-3); font: 11px/17px var(--mono); }
  .first-run-error, .form-error { color: #f0b0a1; }
  .eyebrow { margin: 0 0 var(--s3); color: var(--ink-3); font: 11px/16px var(--mono); letter-spacing: .14em; text-transform: uppercase; }
  .title-row { display: flex; align-items: center; }
  .instance-title { display: inline-flex; align-items: center; gap: var(--s3); padding: 0; color: var(--ink); font-size: clamp(38px, 6vw, 74px); line-height: 1; letter-spacing: -.045em; font-weight: 700; text-align: left; transition: color var(--pan); }
  .instance-title:hover { color: var(--c4); }
  .version-line { margin: var(--s4) 0 var(--s6); color: var(--ink-2); font: var(--t-mono)/20px var(--mono); }
  .version-line span { color: var(--ink-3); padding: 0 6px; }
  .launch-button { position: relative; isolation: isolate; display: inline-flex; align-items: center; justify-content: center; gap: var(--s3); min-width: 190px; min-height: 52px; padding: 0 var(--s6); overflow: hidden; border-radius: 14px; background: var(--c4); color: var(--on-accent); font-weight: 650; box-shadow: var(--shadow-1); transition: transform var(--pan), filter var(--pan); }
  .launch-button:hover { filter: brightness(1.07); transform: translateY(-2px); }
  .launch-button.running { color: var(--on-accent); cursor: wait; }
  .launch-fill { position: absolute; inset: 0 auto 0 0; z-index: -1; background: var(--c3); opacity: .42; transition: width 300ms cubic-bezier(.22,1,.36,1); }
  .launch-label { position: relative; z-index: 1; }
  .launch-meta { display: flex; align-items: center; gap: var(--s2); margin-top: var(--s4); color: var(--ink-3); font: 11px/16px var(--mono); }
  .launch-error { max-width: 56ch; margin-top: var(--s3); padding: var(--s3); border: 1px solid rgba(226, 125, 104, .45); border-radius: 9px; color: #f0b0a1; background: rgba(105, 45, 39, .28); font: 11px/17px var(--mono); user-select: text; }
  .status-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--c4); box-shadow: 0 0 0 4px rgba(255,255,255,.06); }
  .widget-empty { align-self: flex-end; width: min(310px, 32vw); padding: var(--s5) 0; color: var(--ink-3); border-top: 1px solid var(--line); }
  .widget-empty p { margin: 0 0 var(--s1); color: var(--ink-2); font-size: var(--t-body); }
  .widget-empty span { display: block; font: 11px/17px var(--mono); }
  .text-action { display: inline-flex; align-items: center; gap: var(--s2); margin-top: var(--s4); padding: 0; color: var(--c4); font-size: 12px; }
  .widget-grid { align-self: flex-end; display: grid; grid-template-columns: repeat(2, minmax(150px, 1fr)); gap: var(--gut); width: min(370px, 38vw); }
  .widget, .widget-add { min-height: 112px; padding: var(--s4); border: 1px solid var(--line); border-radius: var(--r3); background: var(--glass); backdrop-filter: blur(18px); box-shadow: var(--shadow-1); }
  .widget { display: flex; flex-direction: column; }
  .widget-kicker { margin-bottom: auto; color: var(--ink-3); font: 10px var(--mono); }
  .widget strong { font-size: 22px; letter-spacing: -.02em; }
  .widget span { margin-top: 3px; color: var(--ink-3); font: 10px var(--mono); }
  .widget-add { display: grid; place-items: center; color: var(--ink-3); transition: color var(--pan), background var(--pan); }
  .widget-add:hover { color: var(--ink); background: var(--glass-2); }

  .content-scene { flex-direction: column; padding: 4vh 4vw 0; }
  .section-heading { display: flex; align-items: end; justify-content: space-between; margin-bottom: var(--s6); }
  h1, h2 { margin: 0; letter-spacing: -.04em; line-height: 1; }
  .section-heading h1 { font-size: clamp(34px, 5vw, 56px); }
  .section-note { color: var(--ink-3); font: 11px var(--mono); }
  .outline-button { display: inline-flex; align-items: center; gap: var(--s2); min-height: 38px; padding: 0 var(--s4); border: 1px solid var(--line); border-radius: 10px; color: var(--ink-2); background: rgba(255,255,255,.05); transition: background var(--pan), color var(--pan), transform var(--pan); }
  .outline-button:hover { color: var(--ink); background: rgba(255,255,255,.12); }
  .instance-layout { display: grid; grid-template-columns: minmax(250px, 31%) 1fr; gap: var(--gut); min-height: 0; flex: 1; }
  .instance-list, .instance-detail, .pack-card { border: 1px solid var(--line); border-radius: var(--r3); background: var(--glass); backdrop-filter: blur(18px); box-shadow: var(--shadow-1); }
  .instance-list { padding: var(--s2); overflow-y: auto; }
  .instance-row { display: flex; align-items: center; gap: var(--s3); width: 100%; padding: var(--s3); border-radius: var(--r2); text-align: left; color: var(--ink-2); transition: background var(--pan), color var(--pan), transform var(--pan); }
  .instance-row:hover, .instance-row.active { color: var(--ink); background: rgba(255,255,255,.1); }
  .instance-row:active { transform: scale(.99); }
  .mini-biome { display: block; width: 34px; height: 34px; flex: none; border-radius: 10px; background: var(--swatch); box-shadow: inset 0 1px 0 rgba(255,255,255,.2); }
  .row-copy { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .row-copy strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; }
  .row-copy small, .row-hours { color: var(--ink-3); font: 10px/16px var(--mono); }
  .row-hours { white-space: nowrap; }
  .instance-detail { padding: var(--s6); }
  .detail-empty { height: 100%; min-height: 300px; display: grid; place-items: center; align-content: center; gap: var(--s3); color: var(--ink-3); text-align: center; }
  .detail-empty strong { color: var(--ink); font-size: 20px; }
  .detail-empty span { max-width: 32ch; font: 11px/17px var(--mono); }
  .detail-top { display: flex; align-items: start; justify-content: space-between; padding-bottom: var(--s6); border-bottom: 1px solid var(--line); }
  .detail-top h2 { font-size: clamp(28px, 4vw, 46px); }
  .detail-top span { display: block; margin-top: var(--s2); color: var(--ink-2); font: 11px var(--mono); }
  .launch-mini { display: inline-flex; align-items: center; gap: var(--s2); padding: 9px 14px; border-radius: 10px; background: var(--c4); color: var(--on-accent); font-size: 12px; font-weight: 650; }
  .detail-stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--gut); padding: var(--s6) 0; }
  .detail-stats div { display: flex; flex-direction: column; gap: var(--s1); }
  .detail-stats small { color: var(--ink-3); font: 10px var(--mono); }
  .detail-stats strong { font-size: 22px; }
  .detail-links { display: flex; flex-wrap: wrap; gap: var(--s2); }
  .detail-links button { display: inline-flex; align-items: center; gap: var(--s2); padding: 8px 10px; border-radius: 9px; color: var(--ink-2); background: rgba(255,255,255,.05); font-size: 12px; }
  .detail-links button:hover { color: var(--ink); background: rgba(255,255,255,.11); }
  .search-field { display: flex; align-items: center; gap: var(--s3); margin-bottom: var(--s5); padding: 0 var(--s4); min-height: 48px; border: 1px solid var(--line); border-radius: 12px; background: var(--glass); color: var(--ink-3); }
  .search-field input { width: 100%; border: 0; outline: 0; background: transparent; color: var(--ink); }
  .search-field input::placeholder { color: var(--ink-3); }
  .pack-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--gut); overflow-y: auto; padding-bottom: var(--s6); }
  .pack-card { overflow: hidden; transition: transform var(--pan), border-color var(--pan); }
  .pack-card:hover { transform: translateY(-3px); border-color: rgba(255,255,255,.25); }
  .pack-cover { aspect-ratio: 1.4; padding: var(--s4); display: flex; flex-direction: column; justify-content: space-between; background: var(--pack-color); color: rgba(8,12,13,.82); }
  .pack-cover span { align-self: start; padding: 3px 7px; border-radius: 5px; background: rgba(255,255,255,.3); font: 10px var(--mono); }
  .pack-cover strong { max-width: 9ch; font-size: 25px; line-height: .95; letter-spacing: -.04em; }
  .pack-body { display: flex; flex-direction: column; gap: 4px; padding: var(--s4); }
  .pack-body strong { font-size: 14px; }
  .pack-body small, .pack-body div { color: var(--ink-3); font: 10px/16px var(--mono); }
  .pack-body div { display: flex; justify-content: space-between; margin-top: var(--s2); }
  .empty-state, .empty-scene { display: grid; place-items: center; align-content: center; text-align: center; color: var(--ink-2); }
  .empty-state { flex: 1; min-height: 250px; gap: var(--s2); border: 1px dashed var(--line); border-radius: var(--r3); }
  .empty-state strong, .empty-scene h1 { color: var(--ink); }
  .empty-state span, .empty-scene > p:last-of-type { max-width: 38ch; color: var(--ink-3); font: 11px/18px var(--mono); }
  .empty-scene { height: 100%; gap: var(--s4); }
  .empty-scene h1 { font-size: clamp(32px, 5vw, 58px); }
  .empty-glyph { display: grid; place-items: center; width: 64px; height: 64px; border: 1px solid var(--line); border-radius: 18px; color: var(--c4); background: var(--glass); }

  .modal-scrim { position: fixed; inset: 0; z-index: 20; background: rgba(4,7,8,.45); backdrop-filter: blur(5px); }
  .command-panel, .widget-picker { position: fixed; z-index: 21; border: 1px solid var(--line); background: rgba(12,16,18,.88); backdrop-filter: blur(28px) saturate(1.12); box-shadow: var(--shadow-2); }
  .command-panel { top: 15vh; left: 50%; width: min(630px, calc(100vw - 32px)); transform: translateX(-50%); border-radius: 16px; overflow: hidden; animation: enter 200ms var(--pan); }
  .command-input { display: flex; align-items: center; gap: var(--s3); padding: var(--s4) var(--s5); border-bottom: 1px solid var(--line); color: var(--ink-3); }
  .command-input input { flex: 1; border: 0; outline: 0; background: none; color: var(--ink); font-size: 16px; }
  .command-input input::placeholder { color: var(--ink-3); }
  kbd { padding: 2px 6px; border: 1px solid var(--line); border-radius: 5px; color: var(--ink-3); font: 10px var(--mono); }
  .command-list { max-height: 52vh; padding: var(--s2); overflow-y: auto; }
  .command-group { margin: var(--s3) var(--s3) var(--s2); color: var(--ink-3); font: 10px var(--mono); letter-spacing: .15em; text-transform: uppercase; }
  .command-row { display: flex; align-items: center; gap: var(--s3); width: 100%; padding: var(--s3); border-radius: 10px; color: var(--ink-2); text-align: left; }
  .command-row:hover { color: var(--ink); background: rgba(255,255,255,.1); }
  .command-row > span:nth-child(2) { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .command-row small { color: var(--ink-3); font: 10px var(--mono); }
  .command-icon, .picker-icon { display: grid; place-items: center; width: 32px; height: 32px; border-radius: 9px; color: var(--c4); background: rgba(255,255,255,.07); }
  .command-footer { display: flex; justify-content: flex-end; gap: var(--s4); padding: var(--s3) var(--s5); border-top: 1px solid var(--line); color: var(--ink-3); font: 10px var(--mono); }
  .command-footer span { display: inline-flex; align-items: center; gap: 5px; }
  .widget-picker { top: 50%; right: var(--pad-x); width: min(340px, calc(100vw - 32px)); transform: translateY(-50%); padding: var(--s5); border-radius: 16px; animation: enter 200ms var(--pan); }
  .create-panel { position: fixed; top: 50%; left: 50%; z-index: 21; width: min(420px, calc(100vw - 32px)); transform: translate(-50%, -50%); padding: var(--s5); border: 1px solid var(--line); border-radius: 16px; background: rgba(12,16,18,.92); backdrop-filter: blur(28px); box-shadow: var(--shadow-2); animation: enter 200ms var(--pan); }
  .form-field { display: grid; gap: var(--s2); margin-bottom: var(--s4); color: var(--ink-2); font-size: 12px; }
  .form-field > span { color: var(--ink-3); font: 10px var(--mono); letter-spacing: .12em; text-transform: uppercase; }
  .form-field input, .form-field select { width: 100%; min-height: 42px; padding: 0 var(--s3); border: 1px solid var(--line); border-radius: 9px; outline: 0; background: rgba(255,255,255,.06); color: var(--ink); }
  .form-field input:focus, .form-field select:focus { border-color: var(--c4); }
  .form-field input::placeholder { color: var(--ink-3); }
  .form-field select option { color: #172019; }
  .form-loading { min-height: 42px; display: flex; align-items: center; padding: 0 var(--s3); border: 1px solid var(--line); border-radius: 9px; color: var(--ink-3); font: 11px var(--mono); }
  .form-error { margin: -4px 0 var(--s4); padding: var(--s3); border: 1px solid rgba(226,125,104,.45); border-radius: 9px; background: rgba(105,45,39,.28); font: 11px/17px var(--mono); }
  .create-submit { width: 100%; }
  .form-note { margin: var(--s4) 0 0; color: var(--ink-3); font: 10px/16px var(--mono); }
  .panel-heading { display: flex; align-items: start; justify-content: space-between; margin-bottom: var(--s5); }
  .panel-heading h2 { font-size: 28px; }
  .icon-only { width: 32px; height: 32px; }
  .picker-row { display: flex; align-items: center; gap: var(--s3); width: 100%; padding: var(--s3); border-radius: 10px; color: var(--ink-2); text-align: left; }
  .picker-row:hover { color: var(--ink); background: rgba(255,255,255,.09); }
  .picker-row > span:nth-child(2) { display: flex; flex-direction: column; flex: 1; }
  .picker-row small { color: var(--ink-3); font: 10px var(--mono); }
  @keyframes enter { from { opacity: 0; transform: translate(-50%, -8px); } }

  @media (max-width: 760px) {
    .topbar { padding: 0 16px; gap: var(--s4); }
    nav { gap: var(--s4); overflow-x: auto; }
    nav button { font-size: 13px; letter-spacing: .12em; }
    .top-actions { gap: 0; }
    .shortcut { display: none; }
    .account-button { margin-left: var(--s1); }
    .window-controls { display: none; }
    .stage { padding: var(--top) 16px 24px; }
    .launch-scene { display: block; padding: 12vh 8px 20px; }
    .instance-title { font-size: clamp(38px, 13vw, 62px); }
    .widget-empty { width: 100%; margin-top: 24vh; }
    .widget-grid { width: 100%; margin-top: 20vh; grid-template-columns: repeat(2, 1fr); }
    .content-scene { padding: 8vh 8px 0; }
    .instance-layout { grid-template-columns: 1fr; overflow-y: auto; }
    .instance-detail { min-height: 260px; }
    .pack-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .section-heading { align-items: start; gap: var(--s3); }
    .section-note { display: none; }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(*) { animation-duration: .01ms !important; transition-duration: .01ms !important; }
  }
</style>
