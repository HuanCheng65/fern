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
    Package,
    Play,
    Plus,
    Search,
    Settings2,
    Sparkles,
    X,
  } from 'lucide-svelte'
  import Backdrop from './components/Backdrop.svelte'
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
    | { type: 'task_started'; total_files: number; total_bytes: number }
    | { type: 'file_done'; path: string; bytes: number }
    | { type: 'progress'; done_bytes: number; speed_bps: number }
    | { type: 'task_finished'; failed: string[] }

  const scenes: { id: Scene; label: string }[] = [
    { id: 'launch', label: '启动' },
    { id: 'instances', label: '实例' },
    { id: 'supply', label: '补给' },
    { id: 'multiplayer', label: '联机' },
    { id: 'wardrobe', label: '衣柜' },
  ]

  let instances: Instance[] = [
    { id: 'cinder-valley', name: '余烬谷', version: '1.21.1', loader: 'Fabric', hours: '82.4 h', mods: 38, color: '#d47c51' },
    { id: 'moss-archive', name: '苔痕档案', version: '1.20.4', loader: 'NeoForge', hours: '21.8 h', mods: 71, color: '#79a77d' },
    { id: 'quiet-lands', name: '静默群岛', version: '1.19.2', loader: 'Vanilla', hours: '8.6 h', mods: 0, color: '#6d93b5' },
  ]

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
  let settingsOpen = false
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

  const selectedInstance = () => instances[selected]
  const filteredInstances = () => instances.filter((item) => `${item.name}${item.version}${item.loader}`.toLowerCase().includes(query.toLowerCase()))
  const filteredPacks = () => packs.filter((pack) => `${pack.name}${pack.author}${pack.version}`.toLowerCase().includes(supplyQuery.toLowerCase()))

  onMount(() => {
    let unlistenDownload: UnlistenFn | undefined
    void invoke<string>('app_name').then((value) => (appName = value)).catch(() => undefined)
    void invoke<{ root: string }>('data_paths').then((paths) => (dataRoot = paths.root)).catch(() => undefined)
    void invoke<CoreInstance[]>('default_instances').then((profiles) => {
      if (profiles.length === 0) return
      instances = profiles.map((profile, index) => ({
        id: profile.id,
        name: profile.name,
        version: profile.gameVersion,
        loader: profile.loader === 'neo_forge' ? 'NeoForge' : profile.loader === 'vanilla' ? 'Vanilla' : profile.loader,
        hours: index === 0 ? '82.4 h' : '21.8 h',
        mods: index === 0 ? 38 : 71,
        color: index === 0 ? '#d47c51' : '#79a77d',
      }))
      selected = Math.min(selected, instances.length - 1)
    }).catch(() => undefined)
    if ('__TAURI_INTERNALS__' in window) {
      void listen<DownloadEvent>('download-event', ({ payload }) => {
        if (payload.type === 'task_started') {
          downloadTotalBytes = payload.total_bytes
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
        settingsOpen = false
        showWidgetPicker = false
      }
      if (!commandOpen && !settingsOpen && event.key === 'ArrowRight') moveScene(1)
      if (!commandOpen && !settingsOpen && event.key === 'ArrowLeft') moveScene(-1)
    }
    window.addEventListener('keydown', onKeydown)
    return () => {
      window.removeEventListener('keydown', onKeydown)
      unlistenDownload?.()
    }
  })

  type CoreInstance = {
    id: string
    name: string
    gameVersion: string
    loader: string
  }

  function moveScene(delta: number) {
    const index = scenes.findIndex((item) => item.id === scene)
    scene = scenes[(index + delta + scenes.length) % scenes.length].id
  }

  async function startLaunch() {
    if (isLaunching) return
    isLaunching = true
    launchError = ''
    launchProgress = 2
    launchStatus = '读取版本信息'
    if ('__TAURI_INTERNALS__' in window) {
      try {
        await invoke('prepare_instance', { versionId: selectedInstance().version })
        launchProgress = 100
        launchStatus = '文件准备完成'
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

<div class:effects-off={reducedEffects} class="app-shell">
  <Backdrop seed={selectedInstance().id} hours={selected + 1} particles={!reducedEffects} parallax={!reducedEffects} />
  <div class="scrim" aria-hidden="true"></div>

  <header class="topbar">
    <div class="drag-region" data-tauri-drag-region aria-hidden="true"></div>
    <div class="brand-mark" aria-hidden="true"><span></span></div>
    <nav aria-label="主导航">
      {#each scenes as item (item.id)}
        <button class:active={scene === item.id} aria-current={scene === item.id ? 'page' : undefined} onclick={() => (scene = item.id)}>{item.label}</button>
      {/each}
    </nav>
    <div class="top-actions">
      <button class="quiet-button" aria-label="打开命令面板" title="命令面板 · ⌘K" onclick={() => (commandOpen = true)}>
        <Search size={16} strokeWidth={1.7} />
        <span class="shortcut"><Command size={11} />K</span>
      </button>
      <button class="quiet-button" aria-label="打开设置" title="设置" onclick={() => (settingsOpen = true)}><Settings2 size={17} strokeWidth={1.7} /></button>
      <button class="account-button" aria-label="当前账户">EC</button>
    </div>
  </header>

  <main class="stage">
    {#if scene === 'launch'}
      <section class="launch-scene" aria-label="启动">
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
      </section>
    {:else if scene === 'instances'}
      <section class="content-scene">
        <div class="section-heading"><div><p class="eyebrow">你的世界</p><h1>实例</h1></div><button class="outline-button" onclick={() => (selected = (selected + 1) % instances.length)}><Plus size={15} />新建实例</button></div>
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
            <div class="detail-top"><div><p class="eyebrow">当前实例</p><h2>{selectedInstance().name}</h2><span>{selectedInstance().version} · {selectedInstance().loader}</span></div><button class="launch-mini" onclick={startLaunch}><Play size={14} fill="currentColor" />启动</button></div>
            <div class="detail-stats"><div><small>游玩时长</small><strong>{selectedInstance().hours}</strong></div><div><small>模组数量</small><strong>{selectedInstance().mods}</strong></div><div><small>运行状态</small><strong>就绪</strong></div></div>
            <div class="detail-links"><button><FolderOpen size={15} />打开游戏目录</button><button><Package size={15} />管理内容</button><button><Download size={15} />修复文件</button></div>
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
      <section class="empty-scene"><div class="empty-glyph"><Gamepad2 size={29} strokeWidth={1.4} /></div><p class="eyebrow">{scenes.find((item) => item.id === scene)?.label}</p><h1>这片区域正在生长</h1><p>基础启动链路已经就绪，更多内容会在后续里程碑中进入世界。</p><button class="outline-button" onclick={() => (scene = 'launch')}>回到启动 <ArrowRight size={15} /></button></section>
    {/if}
  </main>

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
          <button class="command-row" onclick={() => { selected = instances.indexOf(item); scene = 'launch'; commandOpen = false }}><span class="mini-biome" style={`--swatch:${item.color}`}></span><span><strong>{item.name}</strong><small>{item.version} · {item.loader}</small></span><ArrowRight size={15} /></button>
        {/each}
        <p class="command-group">动作</p>
        <button class="command-row" onclick={() => { commandOpen = false; startLaunch() }}><span class="command-icon"><Play size={15} fill="currentColor" /></span><span><strong>启动当前实例</strong><small>{selectedInstance().name}</small></span><kbd>↵</kbd></button>
        <button class="command-row" onclick={() => { commandOpen = false; settingsOpen = true }}><span class="command-icon"><Settings2 size={15} /></span><span><strong>打开设置</strong><small>外观与性能</small></span><kbd>⌘,</kbd></button>
      </div>
      <footer class="command-footer"><span><Keyboard size={13} />上下选择</span><span><kbd>ESC</kbd>关闭</span></footer>
    </div>
  {/if}

  {#if settingsOpen}
    <div class="modal-scrim" role="presentation" onclick={() => (settingsOpen = false)}></div>
    <div class="settings-panel" role="dialog" aria-modal="true" aria-label="设置">
      <div class="panel-heading"><div><p class="eyebrow">偏好</p><h2>设置</h2></div><button class="icon-only" aria-label="关闭" onclick={() => (settingsOpen = false)}><X size={17} /></button></div>
      <div class="setting-group"><span class="setting-label">外观</span><label class="setting-row"><span>环境粒子</span><input type="checkbox" checked={!reducedEffects} onchange={(event) => (reducedEffects = !(event.currentTarget as HTMLInputElement).checked)} /></label><label class="setting-row"><span>指针视差</span><input type="checkbox" checked={!reducedEffects} onchange={(event) => (reducedEffects = !(event.currentTarget as HTMLInputElement).checked)} /></label></div>
      <div class="setting-group"><span class="setting-label">账户</span><div class="account-card"><span class="account-avatar">EC</span><div><strong>离线账户</strong><small>本地身份 · Emily Chen</small></div></div></div>
      <div class="settings-foot"><span title={dataRoot}>{appName} 0.1.0</span><span>基础功能预览</span></div>
    </div>
  {/if}
</div>

<style>
  :global(html, body, #app) { margin: 0; height: 100%; overflow: hidden; }
  :global(body) { background: #07090b; color: var(--ink); font-family: var(--sans); font-size: var(--t-body); -webkit-font-smoothing: antialiased; user-select: none; }
  :global(button), :global(input) { font: inherit; color: inherit; }
  :global(button) { border: 0; background: none; cursor: pointer; }
  :global(:focus-visible) { outline: 1.5px solid var(--c4); outline-offset: 3px; }

  .app-shell { position: relative; min-width: 320px; height: 100dvh; overflow: hidden; isolation: isolate; }
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
  .quiet-button, .account-button, .icon-only { display: grid; place-items: center; border-radius: var(--r1); color: var(--ink-2); transition: background var(--pan), color var(--pan), transform var(--pan); }
  .quiet-button { width: 34px; height: 34px; gap: 3px; }
  .quiet-button:hover, .account-button:hover, .icon-only:hover { color: var(--ink); background: rgba(255,255,255,.1); }
  .shortcut { display: inline-flex; align-items: center; font: 10px var(--mono); color: var(--ink-3); }
  .account-button { width: 32px; height: 32px; margin-left: var(--s2); border-radius: 10px; font: 11px var(--mono); color: var(--on-accent); background: var(--c4); }
  .stage { position: relative; z-index: 1; height: 100%; padding: var(--top) var(--pad-x) var(--s8); }
  .launch-scene, .content-scene { height: 100%; min-height: 0; display: flex; }
  .launch-scene { align-items: center; justify-content: space-between; gap: var(--s8); padding: 4vh 4vw 4vh 4vw; }
  .launch-copy { max-width: 560px; }
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
  .command-panel, .widget-picker, .settings-panel { position: fixed; z-index: 21; border: 1px solid var(--line); background: rgba(12,16,18,.88); backdrop-filter: blur(28px) saturate(1.12); box-shadow: var(--shadow-2); }
  .command-panel { top: 15vh; left: 50%; width: min(630px, calc(100vw - 32px)); transform: translateX(-50%); border-radius: 16px; overflow: hidden; animation: enter 200ms var(--pan); }
  .command-input { display: flex; align-items: center; gap: var(--s3); padding: var(--s4) var(--s5); border-bottom: 1px solid var(--line); color: var(--ink-3); }
  .command-input input { flex: 1; border: 0; outline: 0; background: none; color: var(--ink); font-size: 16px; }
  .command-input input::placeholder { color: var(--ink-3); }
  kbd { padding: 2px 6px; border: 1px solid var(--line); border-radius: 5px; color: var(--ink-3); font: 10px var(--mono); }
  .command-list { max-height: 52vh; padding: var(--s2); overflow-y: auto; }
  .command-group, .setting-label { margin: var(--s3) var(--s3) var(--s2); color: var(--ink-3); font: 10px var(--mono); letter-spacing: .15em; text-transform: uppercase; }
  .command-row { display: flex; align-items: center; gap: var(--s3); width: 100%; padding: var(--s3); border-radius: 10px; color: var(--ink-2); text-align: left; }
  .command-row:hover { color: var(--ink); background: rgba(255,255,255,.1); }
  .command-row > span:nth-child(2) { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .command-row small { color: var(--ink-3); font: 10px var(--mono); }
  .command-icon, .picker-icon { display: grid; place-items: center; width: 32px; height: 32px; border-radius: 9px; color: var(--c4); background: rgba(255,255,255,.07); }
  .command-footer { display: flex; justify-content: flex-end; gap: var(--s4); padding: var(--s3) var(--s5); border-top: 1px solid var(--line); color: var(--ink-3); font: 10px var(--mono); }
  .command-footer span { display: inline-flex; align-items: center; gap: 5px; }
  .widget-picker, .settings-panel { top: 50%; right: var(--pad-x); width: min(340px, calc(100vw - 32px)); transform: translateY(-50%); padding: var(--s5); border-radius: 16px; animation: enter 200ms var(--pan); }
  .settings-panel { top: 0; right: 0; bottom: 0; width: min(360px, calc(100vw - 20px)); transform: none; border-radius: 0; border-width: 0 0 0 1px; animation: slide-in 240ms var(--pan); display: flex; flex-direction: column; }
  .panel-heading { display: flex; align-items: start; justify-content: space-between; margin-bottom: var(--s5); }
  .panel-heading h2 { font-size: 28px; }
  .icon-only { width: 32px; height: 32px; }
  .picker-row { display: flex; align-items: center; gap: var(--s3); width: 100%; padding: var(--s3); border-radius: 10px; color: var(--ink-2); text-align: left; }
  .picker-row:hover { color: var(--ink); background: rgba(255,255,255,.09); }
  .picker-row > span:nth-child(2) { display: flex; flex-direction: column; flex: 1; }
  .picker-row small { color: var(--ink-3); font: 10px var(--mono); }
  .setting-group { padding: var(--s4) 0; border-top: 1px solid var(--line); }
  .setting-label { display: block; margin: 0 0 var(--s2); }
  .setting-row { display: flex; justify-content: space-between; align-items: center; padding: var(--s3) 0; color: var(--ink-2); }
  .setting-row input { appearance: none; width: 32px; height: 18px; border-radius: 999px; background: rgba(255,255,255,.14); position: relative; transition: background var(--pan); }
  .setting-row input::before { content: ''; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 999px; background: var(--ink); transition: transform var(--pan); }
  .setting-row input:checked { background: var(--c3); }
  .setting-row input:checked::before { transform: translateX(14px); }
  .account-card { display: flex; align-items: center; gap: var(--s3); padding: var(--s3); border-radius: 10px; background: rgba(255,255,255,.05); }
  .account-avatar { display: grid; place-items: center; width: 34px; height: 34px; border-radius: 10px; color: var(--on-accent); background: var(--c4); font: 11px var(--mono); }
  .account-card div { display: flex; flex-direction: column; }
  .account-card small { color: var(--ink-3); font: 10px var(--mono); }
  .settings-foot { display: flex; justify-content: space-between; margin-top: auto; padding-top: var(--s5); color: var(--ink-3); font: 10px var(--mono); }
  @keyframes enter { from { opacity: 0; transform: translate(-50%, -8px); } }
  @keyframes slide-in { from { opacity: 0; transform: translateX(24px); } }

  @media (max-width: 760px) {
    .topbar { padding: 0 16px; gap: var(--s4); }
    nav { gap: var(--s4); overflow-x: auto; }
    nav button { font-size: 13px; letter-spacing: .12em; }
    .top-actions { gap: 0; }
    .shortcut { display: none; }
    .account-button { margin-left: var(--s1); }
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
