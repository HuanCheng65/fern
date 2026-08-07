<script lang="ts">
  /**
   * 设置。
   *
   * 它是浮层不是场景（见 lib/nav.svelte.ts）：工具属性的东西不该占掉五个
   * 场景位之一——那五个词在概念上都是「玩」的组成部分。所以这里是一块盖在
   * 舞台上的覆盖面板，顶栏留在上面，随时可以点任意场景词离开。
   *
   * 只放真的接着东西的开关。上一版里「启动后保持在后台」「并发任务 64 个
   * 文件」这类项要么点了没反应，要么根本是写死的说明文字——设置页里的
   * 假开关比没有这一页更伤，因为它会让人以为自己已经配置过了。
   *
   * **什么该进这一页**，三条排除线（见 docs/UI_DESIGN.md 十三）：
   *
   *   属于某个实例的     → 实例设置
   *   属于某一次操作的   → 就地解决（岛、对话框）
   *   剩下的、跨实例跨会话的才在这里
   *
   * 「游戏」那一节是所有实例的**起点**，不是它们的替代品：实例设置回答
   * 「这一个要不要特别一点」，这里回答「一般情况下是什么样」。
   *
   * 外观这一节是文档里「个性化出口」的第一批：改动写进主题状态，立刻
   * 全局生效，序列化出来就是一份可以贴给别人的主题码。
   */
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { Check, Copy, FolderOpen, X } from 'lucide-svelte'
  import AccountList from '../components/AccountList.svelte'
  import Choice from '../components/Choice.svelte'
  import Form from '../layouts/Form.svelte'
  import { ACCENT_PRESETS, theme } from '../lib/theme.svelte'
  import { accounts } from '../lib/accounts.svelte'
  import { prefs, suggestedSource } from '../lib/prefs.svelte'
  import { inTauri } from '../lib/instances.svelte'

  type SectionId =
    | 'appearance'
    | 'account'
    | 'game'
    | 'java'
    | 'download'
    | 'data'
    | 'about'

  interface Props {
    /** 打开时落在哪一节。空串就是第一节。 */
    at?: string
    onback: () => void
  }

  let { at = '', onback }: Props = $props()

  const sections: { id: SectionId; label: string }[] = [
    { id: 'appearance', label: '外观' },
    { id: 'account', label: '账户' },
    { id: 'game', label: '游戏' },
    { id: 'java', label: 'Java' },
    { id: 'download', label: '下载' },
    { id: 'data', label: '数据' },
    { id: 'about', label: '关于' },
  ]

  let section = $state<SectionId>('appearance')
  // 外面指定了落点就跟着走。设置已经开着时也生效——命令面板搜到一个设置项，
  // 该把人直接带到那一节，而不是在第一屏放下就不管了。
  $effect(() => {
    if (sections.some((item) => item.id === at)) section = at as SectionId
  })
  let paths = $state({ root: '', logs: '' })
  let pathError = $state('')
  let runtimes = $state<
    { path: string; home: string; major: number; version: string; vendor: string; managed: boolean }[]
  >([])
  let runtimeError = $state('')

  /** 这台机器有多少内存，以及现在那条线在哪。 */
  let budget = $state({ physicalMb: 0, ceilingMb: 0 })
  const GIGABYTE = 1024
  /** 滑杆读的是 GB：内存这件事上没人以 MB 为单位思考。 */
  const gigabytes = (mb: number) => Math.round((mb / GIGABYTE) * 10) / 10
  const ceilingGb = $derived(gigabytes(prefs.game.memoryCeilingMb ?? budget.ceilingMb))
  const custom = $derived(prefs.game.memoryCeilingMb !== null)
  const resolution = $derived(prefs.game.resolution)

  function setCeiling(gb: number) {
    prefs.setGame({ memoryCeilingMb: Math.round(gb * GIGABYTE) })
    void refreshBudget()
  }

  async function refreshBudget() {
    if (!inTauri()) return
    try {
      budget = await invoke('memory_budget')
    } catch {
      // 读不到就把这一行留空，别编一个数出来。
    }
  }

  function setResolution(width: number, height: number) {
    if (!Number.isFinite(width) || !Number.isFinite(height)) return
    prefs.setGame({
      resolution: { width: Math.max(320, Math.round(width)), height: Math.max(240, Math.round(height)) },
    })
  }

  let themeCode = $state('')
  let copied = $state(false)
  let importError = $state('')

  const sourceName = { official: '官方源', bmclapi: 'BMCLAPI' } as const

  onMount(() => {
    themeCode = theme.export()
    if (!inTauri()) return
    void invoke<{ root: string; logs: string }>('data_paths')
      .then((value) => (paths = value))
      .catch((error) => (pathError = String(error)))
    void loadRuntimes()
    void accounts.load()
    void refreshBudget()
  })





  async function loadRuntimes() {
    if (!inTauri()) return
    try {
      runtimes = await invoke('list_java_runtimes')
      runtimeError = ''
    } catch (error) {
      runtimeError = String(error)
    }
  }

  async function removeRuntime(home: string) {
    try {
      await invoke('remove_java_runtime', { home })
      await loadRuntimes()
    } catch (error) {
      runtimeError = String(error)
    }
  }

  function change<T>(apply: (value: T) => void) {
    return (value: T) => {
      apply(value)
      themeCode = theme.export()
      importError = ''
    }
  }

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(themeCode)
      copied = true
      setTimeout(() => (copied = false), 1400)
    } catch {
      // 剪贴板被拒绝时字段本身是可选中的，手动复制照样能完成这件事。
    }
  }

  function applyCode() {
    importError = theme.import(themeCode) ? '' : '无法解析该主题码'
    if (!importError) themeCode = theme.export()
  }

  async function openLogs() {
    pathError = ''
    try {
      await invoke('open_logs_directory')
    } catch (error) {
      pathError = String(error)
    }
  }
</script>

<div class="settings">
  <Form
    {sections}
    {section}
    onsection={(id) => (section = id as SectionId)}
  >
    {#snippet head()}
      <header>
        <h1 class="t-h1">设置</h1>
        <button class="btn btn--icon close" aria-label="关闭设置" onclick={onback}>
          <X size={16} />
        </button>
      </header>
    {/snippet}

    {#if section === 'appearance'}
          <div class="row">
            <span class="label">强调色</span>
            <Choice
              label="强调色来源"
              value={theme.accentMode}
              onchange={change((value) => theme.set('accentMode', value))}
              options={[
                { value: 'biome', label: '跟随背景' },
                { value: 'locked', label: '锁定' },
              ]}
            />
          </div>

          {#if theme.accentMode === 'locked'}
            <div class="row swatch-row">
              <span class="label">颜色</span>
              <div class="swatches">
                {#each ACCENT_PRESETS as preset (preset.key)}
                  <button
                    class="swatch"
                    class:on={theme.accent.toLowerCase() === preset.value}
                    style:background={preset.value}
                    title={preset.name}
                    aria-label={preset.name}
                    onclick={() => change((v: string) => theme.set('accent', v))(preset.value)}
                  >
                    {#if theme.accent.toLowerCase() === preset.value}
                      <Check size={13} strokeWidth={3} />
                    {/if}
                  </button>
                {/each}
                <label class="swatch custom" title="自定义颜色" style:background={theme.accent}>
                  <input
                    type="color"
                    value={theme.accent}
                    oninput={(event) =>
                      change((v: string) => theme.set('accent', v))(event.currentTarget.value)}
                  />
                </label>
              </div>
            </div>
          {/if}

          <div class="row">
            <span class="label">界面密度</span>
            <Choice
              label="界面密度"
              value={theme.density}
              onchange={change((value) => theme.set('density', value))}
              options={[
                { value: 'compact', label: '紧凑' },
                { value: 'default', label: '标准' },
                { value: 'roomy', label: '宽松' },
              ]}
            />
          </div>

          <div class="row">
            <span class="label">圆角</span>
            <Choice
              label="圆角"
              value={theme.radius}
              onchange={change((value) => theme.set('radius', value))}
              options={[
                { value: 'sharp', label: '直角' },
                { value: 'default', label: '标准' },
                { value: 'round', label: '圆润' },
              ]}
            />
          </div>

          <div class="row">
            <span class="label">
              动效
              <small>关闭后同时停用背景粒子与指针视差。窗口失焦时始终暂停。</small>
            </span>
            <Choice
              label="动效"
              value={theme.motion}
              onchange={change((value) => theme.set('motion', value))}
              options={[
                { value: 'full', label: '完整' },
                { value: 'reduced', label: '减弱' },
                { value: 'off', label: '关闭' },
              ]}
            />
          </div>

          <div class="row stack">
            <span class="label">
              主题码
              <small>包含以上全部外观选择。他人粘贴后点击应用即可复现。</small>
            </span>
            <div class="code-row">
              <input class="input selectable t-mono" bind:value={themeCode} spellcheck="false" />
              <button class="btn btn--icon" aria-label="复制" title="复制" onclick={() => void copyCode()}>
                {#if copied}<Check size={15} />{:else}<Copy size={14} />{/if}
              </button>
              <button class="btn btn--ghost" onclick={applyCode}>应用</button>
            </div>
            {#if importError}<p class="err">{importError}</p>{/if}
          </div>

          <div class="row">
            <span class="label">恢复默认外观</span>
            <button
              class="btn btn--ghost"
              onclick={() => {
                theme.reset()
                themeCode = theme.export()
              }}
            >
              恢复
            </button>
          </div>
        {:else if section === 'account'}
          <div class="row stack">
            <span class="label">
              账户
              <small>可保存多个身份，点击名称切换。令牌存储于系统钥匙串，不写入任何文件。</small>
            </span>
            <AccountList />
          </div>
        {:else if section === 'game'}
          <!--
            这一节是所有实例的起点，不是它们的替代品。放在这里的判据只有一条：
            它是不是「一般情况下该是什么样」——只对某一个实例成立的东西属于
            实例设置。
          -->
          <div class="row stack">
            <span class="label">
              游戏内存上限
              <small>
                自动分配的堆与实例中手动指定的值均以此为上限。
                {#if budget.physicalMb}
                  本机内存共 {gigabytes(budget.physicalMb)} GB，默认上限为其一半。
                {/if}
              </small>
            </span>
            <div class="slider-row">
              <input
                class="slider"
                type="range"
                min="2"
                max={Math.max(4, gigabytes(budget.physicalMb || 8192))}
                step="0.5"
                value={ceilingGb}
                oninput={(event) => setCeiling(Number(event.currentTarget.value))}
              />
              <span class="t-mono amount">{ceilingGb} GB</span>
              <button
                class="btn btn--link"
                disabled={!custom}
                onclick={() => {
                  prefs.setGame({ memoryCeilingMb: null })
                  void refreshBudget()
                }}
              >
                恢复默认
              </button>
            </div>
          </div>

          <div class="row">
            <span class="label">
              垃圾回收器
              <small>ZGC 停顿更短，但占用更多内存与 CPU。实例可单独覆盖。</small>
            </span>
            <Choice
              label="垃圾回收器"
              value={prefs.game.garbageCollector ?? 'g1'}
              onchange={(value) => prefs.setGame({ garbageCollector: value as 'g1' | 'z' })}
              options={[
                { value: 'g1', label: 'G1' },
                { value: 'z', label: 'ZGC' },
              ]}
            />
          </div>

          <div class="row stack">
            <span class="label">
              游戏窗口
              <small>未指定时沿用游戏自身记录的尺寸。</small>
            </span>
            <div class="slider-row">
              <Choice
                label="游戏窗口"
                value={resolution ? 'custom' : 'default'}
                onchange={(value) =>
                  prefs.setGame({ resolution: value === 'custom' ? { width: 1280, height: 720 } : null })}
                options={[
                  { value: 'default', label: '由游戏决定' },
                  { value: 'custom', label: '指定尺寸' },
                ]}
              />
              {#if resolution}
                <input
                  class="input size"
                  type="number"
                  value={resolution.width}
                  oninput={(event) =>
                    setResolution(Number(event.currentTarget.value), resolution.height)}
                />
                <span class="t-quiet">×</span>
                <input
                  class="input size"
                  type="number"
                  value={resolution.height}
                  oninput={(event) =>
                    setResolution(resolution.width, Number(event.currentTarget.value))}
                />
              {/if}
            </div>
          </div>

          <div class="row stack">
            <span class="label">
              额外 JVM 参数
              <small>
                置于 Fern 内置参数之后，同名参数以此处为准。以空格分隔，不解析引号。
              </small>
            </span>
            <input
              class="input t-mono"
              value={prefs.game.jvmArguments}
              spellcheck="false"
              placeholder="-XX:+UseStringDeduplication"
              oninput={(event) => prefs.setGame({ jvmArguments: event.currentTarget.value })}
            />
          </div>

          <div class="row">
            <span class="label">
              启动后最小化
              <small>在游戏窗口出现后最小化 Fern，而非点击启动时。</small>
            </span>
            <Choice
              label="启动后最小化"
              value={prefs.minimizeOnLaunch ? 'on' : 'off'}
              onchange={(next) => prefs.setMinimizeOnLaunch(next === 'on')}
              options={[
                { value: 'off', label: '保持显示' },
                { value: 'on', label: '最小化' },
              ]}
            />
          </div>
        {:else if section === 'java'}
          <!-- Java 平时是隐形的；能看见的唯一理由是它占了地方，要能删。 -->
          <div class="row stack">
            <span class="label">
              已安装的运行时
              <small>缺失的版本将在首次启动相应实例时自动下载，无需手动维护。</small>
            </span>
            {#if runtimes.length === 0}
              <p class="t-quiet">未找到可用的 Java，首次启动游戏时将自动下载。</p>
            {:else}
              <ul class="runtimes">
                {#each runtimes as item (item.path)}
                  <li>
                    <span class="rt-name">
                      Java {item.major}
                      <small class="t-quiet">
                        {item.vendor || '未知发行版'} · {item.managed ? '由 Fern 下载' : '系统自带'}
                      </small>
                    </span>
                    {#if item.managed}
                      <button class="btn btn--link" onclick={() => void removeRuntime(item.home)}>
                        删除
                      </button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if runtimeError}<div class="alert">{runtimeError}</div>{/if}
          </div>
        {:else if section === 'download'}
          <div class="row">
            <span class="label">
              下载源
              <small
                >根据系统区域建议使用 {sourceName[suggestedSource()]}。当前源失败时将自动切换到另一个源。</small
              >
            </span>
            <Choice
              label="下载源"
              value={prefs.downloadSource}
              onchange={(value) => prefs.setDownloadSource(value)}
              options={[
                { value: 'official', label: '官方源' },
                { value: 'bmclapi', label: 'BMCLAPI' },
              ]}
            />
          </div>
        {:else if section === 'data'}
          <div class="row stack">
            <span class="label">数据目录</span>
            <p class="path t-mono selectable">{paths.root || '—'}</p>
          </div>
          <div class="row stack">
            <span class="label">日志目录</span>
            <p class="path t-mono selectable">{paths.logs || '—'}</p>
            <button class="btn btn--ghost open" onclick={() => void openLogs()}>
              <FolderOpen size={14} strokeWidth={1.8} />打开日志目录
            </button>
          </div>
          {#if pathError}<div class="alert">{pathError}</div>{/if}
        {:else}
          <div class="row">
            <span class="label">版本</span>
            <span class="t-mono value">Fern 0.1.0</span>
          </div>
        {/if}
  </Form>
</div>

<style>
  .runtimes {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .runtimes li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
  }

  .rt-name {
    display: grid;
    gap: 1px;
    color: var(--ink-2);
    font-size: var(--t-body);
  }

  /*
   * 盖在舞台上，不盖顶栏——场景词要一直在肌肉记忆的位置上。底色压暗到能读，
   * 但仍然透出背景的色彩，不做成一块不透明的板子。
   */
  .settings {
    position: absolute;
    inset: 0;
    z-index: 5;
    padding: calc(var(--top) + var(--s2)) var(--pad-x) 0;
    background: var(--panel);
    -webkit-backdrop-filter: blur(26px) saturate(1.3);
    backdrop-filter: blur(26px) saturate(1.3);
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s5) 0 var(--s6);
  }

  .close {
    flex: none;
    margin-top: 2px;
  }






  /* 每一行是「一个名字，一个控件」。说明文字只在没有它就会用错的地方出现。 */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s5);
    padding: var(--s4) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row.stack {
    display: grid;
    justify-items: stretch;
    gap: var(--s3);
  }

  .row:last-child {
    box-shadow: none;
  }

  .label {
    display: grid;
    gap: 4px;
    font-size: var(--t-body);
    color: var(--ink);
  }

  .label small {
    max-width: 46ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.55;
  }

  .row :global(.choice) {
    flex: none;
    width: 210px;
  }

  .swatch-row {
    align-items: center;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    justify-content: flex-end;
  }

  .swatch {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: #10171b;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.2);
    transition: transform var(--t-fast) var(--spring);
  }

  .swatch:hover {
    transform: scale(1.12);
  }

  .swatch.on {
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.2),
      0 0 0 2px var(--panel),
      0 0 0 3.5px var(--ink);
  }

  .swatch.custom {
    position: relative;
    overflow: hidden;
    cursor: pointer;
    background-image: conic-gradient(#e88, #ee8, #8e8, #8ee, #88e, #e8e, #e88);
  }

  .swatch.custom input {
    position: absolute;
    inset: -6px;
    opacity: 0;
    cursor: pointer;
  }

  .slider-row {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }

  .slider {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
  }

  .amount {
    flex: none;
    min-width: 5ch;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .size {
    width: 88px;
  }

  .code-row {
    display: flex;
    gap: var(--s2);
  }

  .code-row .input {
    font-size: var(--t-small);
  }

  .path {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .open {
    justify-self: start;
  }

  .value {
    color: var(--ink-3);
  }

  .err {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
  }

  @media (max-width: 720px) {
    .row {
      flex-direction: column;
      align-items: stretch;
      gap: var(--s3);
    }

    .row :global(.choice) {
      width: 100%;
    }

    .swatches {
      justify-content: flex-start;
    }
  }
</style>
