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
  import { Check, ChevronLeft, Copy, FolderOpen, X } from 'lucide-svelte'
  import AccountList from '../components/AccountList.svelte'
  import AccountProfile from '../components/AccountProfile.svelte'
  import AddAccount from '../components/AddAccount.svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import AboutHero from '../components/AboutHero.svelte'
  import SettingRow from '../components/SettingRow.svelte'
  import Choice from '../components/Choice.svelte'
  import Form from '../layouts/Form.svelte'
  import { ACCENT_PRESETS, theme } from '../lib/theme.svelte'
  import { accounts } from '../lib/accounts.svelte'
  import { SETTINGS_SECTIONS } from '../lib/settings-catalog'
  import { ui } from '../lib/i18n'
  import { expand } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'
  import { prefs, suggestedSource } from '../lib/prefs.svelte'
  import { updates } from '../lib/update.svelte'
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

  /**
   * 从命令面板直接落到某一行时，把它滚进视野并亮一下。
   *
   * 这一句说完就该消失，所以是一段会退掉的底色，而不是一个选中态。
   */
  let focused = $state('')

  const sections = SETTINGS_SECTIONS

  /**
   * 设置有两级。
   *
   * 根页是那张表单：七节，每节若干行，改一个开关就是改一个值。**但有些东西
   * 不是一个值**——一个账户有名字、UUID、类型、皮肤站、绑定的实例，还有
   * 「设为当前」「改名」「移除」三个动作。把它塞进表单的一行里，就只能塞成
   * 一段就地展开的东西：看一眼 UUID 要把那一行撑开，添加账户要在名单中间
   * 撑开一整张表单，而撑开的那一刻，下面所有的行都往下跳一截。
   *
   * 所以加了第二级：**一行可以是一个入口。** 语法在 `nav.focus` 里定义，
   * `分区/行/目标` 的第三段就是这一级。它和场景的纵深是同一套语法（一次
   * 返回回到上一层、就地展开而不是横移），只是发生在浮层内部。
   */
  const location = $derived(at.split('/').filter(Boolean))
  /** 二级页属于哪一行。`分区/行`。 */
  const page = $derived(location.slice(0, 2).join('/'))
  const target = $derived(location.length >= 3 ? location[2] : '')

  let section = $state<SectionId>('appearance')
  // 外面指定了落点就跟着走。设置已经开着时也生效——命令面板搜到一个设置项，
  // 该把人直接带到那一行，而不是在第一屏放下就不管了。
  $effect(() => {
    const [wanted, row] = location
    if (!wanted || !sections.some((item) => item.id === wanted)) return
    section = wanted as SectionId
    // 在二级页上时不闪那一行：人已经不在那一屏上了。
    if (!row || target) return
    const at = `${wanted}/${row}`
    focused = at
    // 等这一节渲染出来再找它。分区是刚刚才切过去的，这一帧里它还不在 DOM 里。
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-setting="${at}"]`)
        ?.scrollIntoView({ block: 'center', behavior: 'smooth' })
    })
  })

  /** 返回按钮上写的是它所属的那一节。 */
  const sectionLabel = $derived(
    sections.find((item) => item.id === location[0])?.label ?? '设置',
  )

  /** 二级页的标题。 */
  const subtitle = $derived.by(() => {
    if (page === 'data/existing') return '现有游戏目录'
    if (target === 'new') return '添加账户'
    return accounts.list.find((item) => item.id === target)?.playerName ?? '账户'
  })
  let paths = $state({ root: '', game: '', logs: '', portable: false })
  let pathError = $state('')
  interface JavaRuntime {
    path: string
    home: string
    major: number
    version: string
    vendor: string
    arch: string
    managed: boolean
    added: boolean
    image: 'jdk' | 'jre'
    native: boolean
    sizeBytes: number
  }

  /**
   * 按大版本分组，而不是平铺一串安装路径。
   *
   * 用户的问题是「我缺什么」，平铺的列表只回答得了「我装了什么」。缺的那些
   * 也占一组，组里没有运行时——那一行正是要让人看见的。
   */
  let groups = $state<{ major: number; requiredBy: string[]; runtimes: JavaRuntime[] }[]>([])
  let runtimeError = $state('')

  const megabytes = (bytes: number) =>
    bytes > 0 ? `${Math.round(bytes / (1024 * 1024))} MB` : ''

  const describe = (item: JavaRuntime) =>
    [
      item.vendor || '未知发行版',
      item.image === 'jdk' ? 'JDK' : 'JRE',
      item.managed ? '由 Fern 下载' : item.added ? '手动添加' : '系统自带',
      megabytes(item.sizeBytes),
    ]
      .filter(Boolean)
      .join(' · ')

  async function installJava(major: number) {
    runtimeError = ''
    try {
      await invoke('install_java', {
        major,
        title: `安装 Java ${major}`,
        subjects: [`java-${major}`],
      })
      await loadRuntimes()
    } catch (error) {
      runtimeError = String(error)
    }
  }

  /**
   * 手动登记一个位置。
   *
   * 用输入框而不是系统文件选择器：Java 装在哪里是一个用户能直接说出来的
   * 路径，而目录选择器要为此引入一个仅此一处使用的权限。
   */
  let manualPath = $state('')

  async function addJavaPath() {
    if (!manualPath.trim()) return
    runtimeError = ''
    try {
      await invoke('add_java_path', { path: manualPath.trim() })
      manualPath = ''
      await loadRuntimes()
    } catch (error) {
      runtimeError = String(error)
    }
  }

  async function forgetJavaPath(home: string) {
    try {
      await invoke('forget_java_path', { home })
      await loadRuntimes()
    } catch (error) {
      runtimeError = String(error)
    }
  }

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
  let copiedReport = $state(false)
  /** 关于页那几行。空串表示还没问到，或者不在 Tauri 里。 */
  let about = $state({ version: '', commit: '', built: '', platform: '', webview: '' })
  let importError = $state('')

  const sourceName = { official: '官方源', bmclapi: 'BMCLAPI' } as const

  const REPOSITORY = 'https://github.com/HuanCheng65/fern'

  /** 交给系统浏览器。后端只放行 https。 */
  const openInBrowser = (url: string) => void invoke('open_external', { url })

  /**
   * 一段可以直接贴进 issue 的话。
   *
   * 反馈问题时最费时间的是来回问三轮「什么系统、什么版本、日志在哪」。Java
   * 那一行由这一屏本来就加载着的清单拼出来，不额外查一次。
   */
  const report = $derived(
    [
      `Fern ${about.version}${about.commit ? ` (${about.commit}, ${about.built})` : ''}`,
      [about.platform, about.webview && `WebView ${about.webview}`].filter(Boolean).join(' · '),
      `数据目录 ${paths.root || '—'}`,
      `Java ${
        groups.flatMap((group) => group.runtimes).map((runtime) => runtime.version).join('、') ||
        '未检测到'
      }`,
    ].join('\n'),
  )

  async function copyReport() {
    await navigator.clipboard.writeText(report)
    copiedReport = true
    setTimeout(() => (copiedReport = false), 1400)
  }

  onMount(() => {
    themeCode = theme.export()
    if (!inTauri()) return
    void invoke<typeof about>('about').then((value) => (about = value))
    void invoke<{ root: string; game: string; logs: string; portable: boolean }>('data_paths')
      .then((value) => (paths = value))
      .catch((error) => (pathError = String(error)))
    void loadRuntimes()
    void accounts.load()
    void refreshBudget()
  })





  async function loadRuntimes() {
    if (!inTauri()) return
    try {
      groups = await invoke('java_overview')
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
  {#if target}
    <!--
      二级页。整块换掉而不是挤在右栏里：左侧那列锚点回答的是「这一长页我要看
      哪一段」，而这里已经不是那一页了——留着它只会让人以为自己还站在表单上。
    -->
    <div class="sub scroll" data-page-scroll in:expand>
      <header>
        <div class="crumbs">
          <!-- 返回到它所属的那一节，名字从目录里取——这一级是通用机制，不是
               账户专用的。 -->
          <button class="btn btn--link" onclick={() => nav.show('settings', location.slice(0, 2).join('/'))}>
            <ChevronLeft size={14} strokeWidth={2} />{sectionLabel}
          </button>
          <h1 class="t-h1">{subtitle}</h1>
        </div>
        <button class="btn btn--icon close" aria-label="关闭设置" onclick={onback}>
          <X size={16} />
        </button>
      </header>

      <div class="sub-body">
        {#if page === 'data/existing'}
          <AdoptDirectory />
        {:else if target === 'new'}
          <AddAccount ondone={(id) => nav.show('settings', `account/list${id ? `/${id}` : ''}`)} />
        {:else}
          <AccountProfile
            accountId={target}
            ongone={() => nav.show('settings', 'account/list')}
          />
        {/if}
      </div>
    </div>
  {:else}
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
          <SettingRow id="appearance/accent" found={focused === 'appearance/accent'}>
            <Choice
              label="强调色来源"
              value={theme.accentMode}
              onchange={change((value) => theme.set('accentMode', value))}
              options={[
                { value: 'biome', label: '跟随背景' },
                { value: 'locked', label: '锁定' },
              ]}
            />
          </SettingRow>

          {#if theme.accentMode === 'locked'}
            <SettingRow id="appearance/swatch" found={focused === 'appearance/swatch'}>
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
            </SettingRow>
          {/if}

          <SettingRow id="appearance/density" found={focused === 'appearance/density'}>
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
          </SettingRow>

          <SettingRow id="appearance/radius" found={focused === 'appearance/radius'}>
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
          </SettingRow>

          <SettingRow id="appearance/motion" found={focused === 'appearance/motion'}>
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
          </SettingRow>

          <SettingRow id="appearance/code" found={focused === 'appearance/code'}>
            <div class="code-row">
              <input class="input selectable t-mono" bind:value={themeCode} spellcheck="false" />
              <button class="btn btn--icon" aria-label="复制" title="复制" onclick={() => void copyCode()}>
                {#if copied}<Check size={15} />{:else}<Copy size={14} />{/if}
              </button>
              <button class="btn btn--ghost" onclick={applyCode}>应用</button>
            </div>
            {#if importError}<p class="err">{importError}</p>{/if}
          </SettingRow>

          <SettingRow id="appearance/reset" found={focused === 'appearance/reset'}>
            <button
              class="btn btn--ghost"
              onclick={() => {
                theme.reset()
                themeCode = theme.export()
              }}
            >
              恢复
            </button>
          </SettingRow>
        {:else if section === 'account'}
          <SettingRow id="account/list" found={focused === 'account/list'}>
            <AccountList />
          </SettingRow>
        {:else if section === 'game'}
          <!--
            这一节是所有实例的起点，不是它们的替代品。放在这里的判据只有一条：
            它是不是「一般情况下该是什么样」——只对某一个实例成立的东西属于
            实例设置。
          -->
          <SettingRow id="game/memory" found={focused === 'game/memory'}>
            {#snippet note()}自动分配的堆与实例中手动指定的值均以此为上限。 {#if budget.physicalMb} 本机内存共 {gigabytes(budget.physicalMb)} GB，默认上限为其一半。 {/if}{/snippet}
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
          </SettingRow>

          <SettingRow id="game/gc" found={focused === 'game/gc'}>
            <Choice
              label="垃圾回收器"
              value={prefs.game.garbageCollector ?? 'auto'}
              onchange={(value) => prefs.setGame({ garbageCollector: value as 'auto' | 'g1' | 'z' })}
              options={[
                { value: 'auto', label: '自动' },
                { value: 'g1', label: 'G1' },
                { value: 'z', label: 'ZGC' },
              ]}
            />
          </SettingRow>

          <SettingRow id="game/window" found={focused === 'game/window'}>
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
          </SettingRow>

          <SettingRow id="game/jvm" found={focused === 'game/jvm'}>
            <input
              class="input t-mono"
              value={prefs.game.jvmArguments}
              spellcheck="false"
              placeholder="-XX:+UseStringDeduplication"
              oninput={(event) => prefs.setGame({ jvmArguments: event.currentTarget.value })}
            />
          </SettingRow>

          <SettingRow id="game/minimize" found={focused === 'game/minimize'}>
            <Choice
              label="启动后最小化"
              value={prefs.minimizeOnLaunch ? 'on' : 'off'}
              onchange={(next) => prefs.setMinimizeOnLaunch(next === 'on')}
              options={[
                { value: 'off', label: '保持显示' },
                { value: 'on', label: '最小化' },
              ]}
            />
          </SettingRow>
        {:else if section === 'java'}
          <SettingRow id="java/runtimes" found={focused === 'java/runtimes'}>

            {#if groups.length === 0}
              <p class="t-quiet">尚未扫描到任何 Java，也没有实例需要它。</p>
            {/if}

            <ul class="runtimes">
              {#each groups as group (group.major)}
                <li class="group">
                  <div class="group-head">
                    <span class="rt-name">
                      Java {group.major}
                      <small class="t-quiet">
                        {#if group.requiredBy.length > 0}
                          {group.requiredBy.length} 个实例需要：{group.requiredBy.join('、')}
                        {:else}
                          当前没有实例需要
                        {/if}
                      </small>
                    </span>
                    {#if group.runtimes.length === 0}
                      <button class="btn btn--ghost" onclick={() => void installJava(group.major)}>
                        安装
                      </button>
                    {/if}
                  </div>

                  {#each group.runtimes as item (item.path)}
                    <div class="rt">
                      <span class="rt-name">
                        {item.version || `Java ${item.major}`}
                        <small class="t-quiet">{describe(item)}</small>
                        <!-- 非原生架构必须说明：能跑，但明显更慢，而这一点
                             在任何别的地方都看不出来。 -->
                        {#if !item.native}
                          <small class="warn">{item.arch} 版本，与本机架构不一致，性能会下降</small>
                        {/if}
                        <small class="t-mono path">{item.home}</small>
                      </span>
                      {#if item.managed}
                        <button class="btn btn--link" onclick={() => void removeRuntime(item.home)}>
                          删除
                        </button>
                      {:else if item.added}
                        <button class="btn btn--link" onclick={() => void forgetJavaPath(item.home)}>
                          移除登记
                        </button>
                      {/if}
                    </div>
                  {/each}
                </li>
              {/each}
            </ul>
          </SettingRow>

          <SettingRow id="java/add" found={focused === 'java/add'}>
            <div class="code-row">
              <input
                class="input t-mono"
                bind:value={manualPath}
                spellcheck="false"
                placeholder="/usr/lib/jvm/java-21-openjdk"
                onkeydown={(event) => event.key === 'Enter' && void addJavaPath()}
              />
              <button class="btn btn--ghost" onclick={() => void addJavaPath()}>添加</button>
            </div>
          </SettingRow>

          <SettingRow id="java/rescan" found={focused === 'java/rescan'}>
            <button class="btn btn--ghost" onclick={() => void loadRuntimes()}>扫描</button>
          </SettingRow>

          {#if runtimeError}<div class="alert">{runtimeError}</div>{/if}
        {:else if section === 'download'}
          <SettingRow id="download/source" found={focused === 'download/source'}>
            {#snippet note()}根据系统区域建议使用 {sourceName[suggestedSource()]}。当前源失败时将自动切换到另一个源。{/snippet}
            <Choice
              label="下载源"
              value={prefs.downloadSource}
              onchange={(value) => prefs.setDownloadSource(value)}
              options={[
                { value: 'official', label: '官方源' },
                { value: 'bmclapi', label: 'BMCLAPI' },
              ]}
            />
          </SettingRow>
        {:else if section === 'data'}
          <SettingRow id="data/root" found={focused === 'data/root'}>
            <p class="path t-mono selectable">{paths.root || '—'}</p>
            {#if paths.portable}
              <p class="t-quiet hint">
                数据目录随可执行文件所在位置。移动整个文件夹即可迁移全部数据。
              </p>
            {/if}
          </SettingRow>
          <SettingRow id="data/game" found={focused === 'data/game'}>
            <p class="path t-mono selectable">{paths.game || '—'}</p>
          </SettingRow>
          <SettingRow id="data/existing" found={focused === 'data/existing'}>
            <button class="btn btn--ghost" onclick={() => nav.show('settings', 'data/existing/browse')}>
              选择目录…
            </button>
          </SettingRow>
          <SettingRow id="data/logs" found={focused === 'data/logs'}>
            <p class="path t-mono selectable">{paths.logs || '—'}</p>
            <button class="btn btn--ghost open" onclick={() => void openLogs()}>
              <FolderOpen size={14} strokeWidth={1.8} />打开日志目录
            </button>
          </SettingRow>
          {#if pathError}<div class="alert">{pathError}</div>{/if}
        {:else}
          <!--
            这一块不走 SettingRow：那一行是「一个名字，一个控件」，而这里没有
            控件，讲的是这个产品是什么。`data-setting` 保留着，命令面板还能直接
            跳到它。
          -->
          <div class="hero-slot" data-setting="about/version" class:found={focused === 'about/version'}>
            <AboutHero version={about.version} commit={about.commit} built={about.built} />
          </div>

          <SettingRow id="about/diagnostics" found={focused === 'about/diagnostics'}>
            <pre class="t-mono report selectable">{report}</pre>
            <button class="btn btn--ghost" onclick={() => void copyReport()}>
              {#if copiedReport}<Check size={14} />{:else}<Copy size={13} strokeWidth={1.9} />{/if}
              {copiedReport ? ui.about.copied : ui.about.copy}
            </button>
          </SettingRow>

          <!--
            检查更新这一行的沉默规则：`held_back`（灰度还没轮到）什么都不显示，
            和「已是最新」走同一句话——「有更新但不给你」是最招人烦的一种提示。
            失败也只在这里说，因为只有这一行代表「用户自己问了」。
          -->
          <SettingRow id="about/update" found={focused === 'about/update'}>
            <p class="update-state">
              {#if updates.checking}
                {ui.about.update.checking}
              {:else if updates.failed}
                {ui.about.update.failed}
              {:else if updates.decision?.kind === 'available'}
                {ui.about.update.available}
                <strong>{updates.decision.version}</strong>
                {#if updates.decision.critical}<br /><span class="t-quiet">{ui.about.update.critical}</span>{/if}
              {:else if updates.decision?.kind === 'ahead_of_channel'}
                {ui.about.update.aheadOfChannel}
              {:else if updates.decision?.kind === 'needs_full_download'}
                {ui.about.update.needsFullDownload}
              {:else if updates.decision?.kind === 'no_build'}
                {ui.about.update.noBuild}
              {:else if updates.decision}
                {ui.about.update.upToDate}
              {/if}
            </p>
            <div class="links">
              <button
                class="btn btn--ghost"
                disabled={updates.checking}
                onclick={() => void updates.check()}
              >
                {ui.about.update.check}
              </button>
              {#if updates.decision && updates.decision.kind !== 'up_to_date' && updates.decision.kind !== 'held_back'}
                <button class="btn btn--link" onclick={() => openInBrowser(`${REPOSITORY}/releases`)}>
                  {ui.about.update.download}
                </button>
              {/if}
            </div>
            <Choice
              label={ui.about.update.automatic}
              value={updates.automatic ? 'on' : 'off'}
              onchange={(next) => updates.setAutomatic(next === 'on')}
              options={[
                { value: 'on', label: ui.about.update.automaticOn },
                { value: 'off', label: ui.about.update.automaticOff },
              ]}
            />
          </SettingRow>

          <SettingRow id="about/channel" found={focused === 'about/channel'}>
            <Choice
              label="更新通道"
              value={updates.channel}
              onchange={(next) => updates.setChannel(next === 'beta' ? 'beta' : 'stable')}
              options={[
                { value: 'stable', label: ui.about.update.channelStable },
                { value: 'beta', label: ui.about.update.channelBeta },
              ]}
            />
          </SettingRow>

          <SettingRow id="about/links" found={focused === 'about/links'}>
            <div class="links">
              <button class="btn btn--link" onclick={() => openInBrowser(REPOSITORY)}>
                {ui.about.repository}
              </button>
              <button class="btn btn--link" onclick={() => openInBrowser(`${REPOSITORY}/issues`)}>
                {ui.about.issues}
              </button>
            </div>
          </SettingRow>

          <SettingRow id="about/legal" found={focused === 'about/legal'}>
            <p class="legal">{ui.about.license}{ui.about.licenseFork}</p>
            <p class="legal t-quiet">{ui.about.notOfficial}</p>
          </SettingRow>
        {/if}
  </Form>
  {/if}
</div>

<style>
  /* 二级页和根页共用同一套头部与列宽，只是没有左边那列锚点。 */
  .sub {
    height: 100%;
    min-height: 0;
    padding-right: var(--s2);
  }

  .sub-body {
    max-width: 640px;
    padding-bottom: var(--s8);
  }

  .crumbs {
    display: grid;
    gap: 2px;
  }

  .crumbs .btn--link {
    gap: 2px;
    justify-self: start;
    padding-left: 0;
    color: var(--ink-3);
  }

  .crumbs .btn--link:hover {
    color: var(--ink);
  }

  .runtimes {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .group {
    display: grid;
    gap: var(--s2);
    padding: var(--s3) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .group:last-child {
    box-shadow: none;
  }

  .group-head,
  .rt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  /* 具体的安装缩进一级：它们从属于上面那个大版本。 */
  .rt {
    padding-left: var(--s4);
  }

  .warn {
    color: var(--danger);
    font-size: var(--t-micro);
  }

  .rt .path {
    color: var(--ink-4);
    font-size: var(--t-micro);
    overflow-wrap: anywhere;
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

  /* 检查更新那一行的状态句。行高对齐旁边的按钮，别让这一行比其它行矮一截。 */
  .update-state {
    margin: 0;
    align-self: center;
    line-height: 1.5;
  }

  .links {
    display: flex;
    gap: var(--s4);
  }

  /* 顶上那一块要整幅铺开，所以它不在行的栅格里。 */
  .hero-slot {
    margin: var(--s2) 0 var(--s5);
    border-radius: var(--r2);
  }

  .hero-slot.found {
    animation: found-block 2.4s var(--ease) forwards;
  }

  @keyframes found-block {
    0%,
    55% {
      box-shadow: 0 0 0 2px var(--accent);
    }
    100% {
      box-shadow: 0 0 0 2px transparent;
    }
  }

  /* 一段能整段选中、整段复制的文本。等宽是因为它要贴到 issue 里去。 */
  .report {
    margin: 0 0 var(--s3);
    padding: var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.7;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .legal {
    margin: 0 0 var(--s2);
    max-width: 56ch;
    color: var(--ink-2);
    font-size: var(--t-small);
    line-height: 1.7;
  }

  .legal:last-child {
    margin-bottom: 0;
  }

  .path {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .open {
    justify-self: start;
  }

  .err {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
  }

  @media (max-width: 720px) {
    .swatches {
      justify-content: flex-start;
    }
  }
</style>
