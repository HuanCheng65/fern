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
  import { Check, ChevronLeft, ChevronRight, Copy, FolderOpen, X } from 'lucide-svelte'
  import AccountList from '../components/AccountList.svelte'
  import AccountProfile from '../components/AccountProfile.svelte'
  import AddAccount from '../components/AddAccount.svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import AboutHero from '../components/AboutHero.svelte'
  import JavaRuntimeProfile from '../components/JavaRuntimeProfile.svelte'
  import MemoryMeter from '../components/MemoryMeter.svelte'
  import SettingRow from '../components/SettingRow.svelte'
  import SegmentedControl from 'fern-kit/ui/SegmentedControl.svelte'
  import { javaLabel, megabytes, type JavaGroup } from '../lib/java'
  import Form from '../layouts/Form.svelte'
  import { ACCENT_PRESETS, theme } from '../lib/theme.svelte'
  import { accounts } from '../lib/accounts.svelte'
  import { SETTINGS_SECTIONS } from '../lib/settings-catalog'
  import { ui } from '../lib/i18n'
  import { expand } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'
  import { launch } from '../lib/launch.svelte'
  import { prefs, suggestedSource } from '../lib/prefs.svelte'
  import { updates } from '../lib/update.svelte'
  import { inTauri } from '../lib/instances.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

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
  /**
   * 上一次落在哪儿。
   *
   * 「亮一下」回答的是「你要找的在这里」，那是**被送过来**才需要的一句话。从
   * 二级页返回时人本来就是从这一行进去的，用不着有人再指一次——退回来看见它
   * 闪，读起来像是「这一行出事了」。两种情形在 `at` 上分得开：返回是从
   * `分区/行/目标` 退到 `分区/行`，前一个位置在后一个的里面。
   */
  let came = ''
  // 外面指定了落点就跟着走。设置已经开着时也生效——命令面板搜到一个设置项，
  // 该把人直接带到那一行，而不是在第一屏放下就不管了。
  $effect(() => {
    const [wanted, row] = location
    const from = came
    came = location.join('/')
    if (!wanted || !sections.some((item) => item.id === wanted)) return
    section = wanted as SectionId
    // 在二级页上时不闪那一行：人已经不在那一屏上了。
    if (!row || target) return
    const at = `${wanted}/${row}`
    // 从这一行的二级页退回来：把它滚回视野里，但不指它。
    focused = from.startsWith(`${at}/`) ? '' : at
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
    if (page === 'java/runtimes') {
      const home = decodeURIComponent(target)
      return installed.find((item) => item.home === home)?.version ?? 'Java 运行时'
    }
    if (target === 'new') return '添加账户'
    return accounts.list.find((item) => item.id === target)?.playerName ?? '账户'
  })
  let paths = $state({ root: '', game: '', logs: '', portable: false })
  let pathError = $state('')

  /**
   * 按大版本分组，而不是平铺一串安装路径。
   *
   * 用户的问题是「我缺什么」，平铺的列表只回答得了「我装了什么」。缺的那些
   * 也占一组，组里没有运行时——那一行正是要让人看见的。
   */
  let groups = $state<JavaGroup[]>([])
  let runtimeError = $state('')

  const installed = $derived(groups.flatMap((group) => group.runtimes))
  /** 「能回收多少」是来这一页的理由之一，所以它写在段头上。 */
  const managedBytes = $derived(
    installed.reduce((total, item) => total + (item.managed ? item.sizeBytes : 0), 0),
  )

  /** 档案是设置里的二级页。home 是一条路径，要转义才塞得进 `分区/行/目标`。 */
  const profileAt = (home: string) => `java/runtimes/${encodeURIComponent(home)}`

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
  let budget = $state<{ physicalMb: number; ceilingMb: number; usedMb: number | null }>({
    physicalMb: 0,
    ceilingMb: 0,
    usedMb: null,
  })
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
  /** 需要人自己挑一个包时才用得上。平常的落点是清单里那个确切的文件。 */
  const DOWNLOADS = 'https://dl.fern.huanchengfly.top'

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
          <div class="crumb-back">
            <Button variant="link" tone="quiet" onclick={() => nav.show('settings', location.slice(0, 2).join('/'))}>
              <ChevronLeft size={14} strokeWidth={2} />{sectionLabel}
            </Button>
          </div>
          <h1 class="t-h1">{subtitle}</h1>
        </div>
        <div class="close">
          <Button variant="icon" tone="quiet" aria-label="关闭设置" onclick={onback}>
            <X size={16} />
          </Button>
        </div>
      </header>

      <div class="sub-body">
        {#if page === 'data/existing'}
          <AdoptDirectory />
        {:else if page === 'java/runtimes'}
          <JavaRuntimeProfile
            home={decodeURIComponent(target)}
            {groups}
            onchanged={() => void loadRuntimes()}
            ongone={() => nav.show('settings', 'java/runtimes')}
            remove={removeRuntime}
            forget={forgetJavaPath}
          />
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
        <div class="close">
          <Button variant="icon" tone="quiet" aria-label="关闭设置" onclick={onback}>
            <X size={16} />
          </Button>
        </div>
      </header>
    {/snippet}

    {#if section === 'appearance'}
          <SettingRow id="appearance/accent" found={focused === 'appearance/accent'}>
            <SegmentedControl
              aria-label="强调色来源"
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
            <SegmentedControl
              aria-label="界面密度"
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
            <SegmentedControl
              aria-label="圆角"
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
            <SegmentedControl
              aria-label="动效"
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
              <Button variant="icon" aria-label="复制" title="复制" onclick={() => void copyCode()}>
                {#if copied}<Check size={15} />{:else}<Copy size={14} />{/if}
              </Button>
              <Button variant="ghost" onclick={applyCode}>应用</Button>
            </div>
            {#if importError}<p class="err">{importError}</p>{/if}
          </SettingRow>

          <SettingRow id="appearance/reset" found={focused === 'appearance/reset'}>
            <Button
              variant="ghost"
              onclick={() => {
                theme.reset()
                themeCode = theme.export()
              }}>
              恢复
            </Button>
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
            <!--
              「这台机器上还跑着别的什么」这句话删掉了：尺上那道暗色就是它，
              而一段几何比一句话快。说明只留控件本身说不出来的那一件事。
            -->
            {#snippet note()}自动分配的堆与实例中手动指定的值均以此为上限。{/snippet}
            <!--
              和实例设置里那一节共用同一根尺。两屏说同一种视觉语言，这条线在
              哪、离满还有多远，两处读起来是一回事。

              这里拖的**就是**上限本身，右端只是物理内存，所以不画那堵墙。
            -->
            <div class="ceiling-row">
              <span class="t-mono amount">{ceilingGb} GB</span>
              <Button
                variant="link"
                disabled={!custom}
                onclick={() => {
                  prefs.setGame({ memoryCeilingMb: null })
                  void refreshBudget()
                }}>
                恢复默认
              </Button>
            </div>
            <MemoryMeter
              label="游戏内存上限"
              physicalMb={budget.physicalMb}
              usedMb={budget.usedMb ?? undefined}
              ceilingMb={budget.physicalMb || 8192}
              valueMb={Math.round(ceilingGb * GIGABYTE)}
              minMb={2 * GIGABYTE}
              stepMb={512}
              showCeiling={false}
              marks={budget.physicalMb
                ? [{ at: Math.round(budget.physicalMb / 2), label: '默认：本机的一半' }]
                : []}
              onchange={(mb) => setCeiling(mb / GIGABYTE)}
            />
          </SettingRow>

          <SettingRow id="game/gc" found={focused === 'game/gc'}>
            <SegmentedControl
              aria-label="垃圾回收器"
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
              <SegmentedControl
                aria-label="游戏窗口"
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
            <SegmentedControl
              aria-label="启动后最小化"
              value={prefs.minimizeOnLaunch ? 'on' : 'off'}
              onchange={(next) => prefs.setMinimizeOnLaunch(next === 'on')}
              options={[
                { value: 'off', label: '保持显示' },
                { value: 'on', label: '最小化' },
              ]}
            />
          </SettingRow>
        {:else if section === 'java'}
          <!--
            这一节回答的是「我缺什么、我能删什么」，不是「我装了什么」——所以
            组头写状态，段头写总占用，具体那几行安静下来，细节进档案页。
          -->
          <SettingRow id="java/runtimes" found={focused === 'java/runtimes'}>
            {#if groups.length === 0}
              <p class="t-quiet">尚未扫描到任何 Java，也没有实例需要它。</p>
            {:else}
              <p class="tally t-quiet">
                共 {installed.length} 份{#if managedBytes > 0}
                  ，其中 {megabytes(managedBytes)} 由 Fern 下载、可回收{/if}
              </p>
            {/if}

            <ul class="runtimes">
              {#each groups as group (group.major)}
                <li class="group">
                  <div class="group-head">
                    <span class="major">Java {group.major}</span>
                    <span class="state" class:missing={group.runtimes.length === 0}>
                      {#if group.runtimes.length === 0}
                        缺
                      {:else if group.requiredBy.length === 0}
                        无实例需要
                      {:else}
                        {group.requiredBy.length} 个实例需要
                      {/if}
                    </span>
                    {#if group.runtimes.length === 0}
                      <Button variant="ghost" onclick={() => void installJava(group.major)}>
                        安装
                      </Button>
                    {/if}
                  </div>
                  {#if group.requiredBy.length > 0}
                    <p class="who t-quiet">{group.requiredBy.join('、')}</p>
                  {/if}

                  {#each group.runtimes as item (item.path)}
                    <button class="rt" onclick={() => nav.show('settings', profileAt(item.home))}>
                      <span class="rt-name">
                        {item.version || `Java ${item.major}`}
                        <small class="t-quiet">{javaLabel(item)}</small>
                      </span>
                      <!--
                        实例那一屏只说得出「会用 Java 21」。装了不止一份的时候，
                        不指出是哪一份，两屏就对不上号。

                        只有一份时不说：那时「哪一份」根本不是个问题，标出来只是
                        给每一行都挂上一句废话。
                      -->
                      {#if group.runtimes.length > 1 && group.preferred === item.home}
                        <span class="badge">默认选用</span>
                      {/if}
                      <ChevronRight size={15} strokeWidth={2} />
                    </button>
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
              <Button variant="ghost" onclick={() => void addJavaPath()}>添加</Button>
            </div>
          </SettingRow>

          <SettingRow id="java/rescan" found={focused === 'java/rescan'}>
            <Button variant="ghost" onclick={() => void loadRuntimes()}>扫描</Button>
          </SettingRow>

          {#if runtimeError}<div class="alert">{runtimeError}</div>{/if}
        {:else if section === 'download'}
          <SettingRow id="download/source" found={focused === 'download/source'}>
            {#snippet note()}根据系统区域建议使用 {sourceName[suggestedSource()]}。当前源失败时将自动切换到另一个源。{/snippet}
            <SegmentedControl
              aria-label="下载源"
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
            <Button variant="ghost" onclick={() => nav.show('settings', 'data/existing/browse')}>
              选择目录…
            </Button>
          </SettingRow>
          <SettingRow id="data/logs" found={focused === 'data/logs'}>
            <p class="path t-mono selectable">{paths.logs || '—'}</p>
            <div class="open">
              <Button variant="ghost" onclick={() => void openLogs()}>
                <FolderOpen size={14} strokeWidth={1.8} />打开日志目录
              </Button>
            </div>
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
            <Button variant="ghost" onclick={() => void copyReport()}>
              {#if copiedReport}<Check size={14} />{:else}<Copy size={13} strokeWidth={1.9} />{/if}
              {copiedReport ? ui.about.copied : ui.about.copy}
            </Button>
          </SettingRow>

          <!--
            检查更新这一行的沉默规则：`held_back`（灰度还没轮到）什么都不显示，
            和「已是最新」走同一句话——「有更新但不给你」是最招人烦的一种提示。
            失败也只在这里说，因为只有这一行代表「用户自己问了」。
          -->
          <SettingRow id="about/update" found={focused === 'about/update'}>
            <p class="update-state">
              {#if updates.installed}
                {ui.about.update.installed}
              {:else if updates.applying}
                {ui.about.update.updating}{#if updates.progress !== undefined}
                  · {Math.round(updates.progress * 100)}%{/if}
              {:else if updates.error}
                {updates.error}
              {:else if updates.checking}
                {ui.about.update.checking}
              {:else if updates.failed}
                {ui.about.update.failed}
              {:else if updates.decision?.kind === 'available'}
                {ui.about.update.available}
                <strong>{updates.decision.version}</strong>
                {#if updates.decision.critical}<br /><span class="t-quiet">{ui.about.update.critical}</span>{/if}
                {#if !updates.selfUpdate}<br /><span class="t-quiet">{ui.about.update.managed}</span>{/if}
              {:else if updates.decision?.kind === 'ahead_of_channel'}
                {ui.about.update.aheadOfChannel}
              {:else if updates.decision?.kind === 'needs_full_download'}
                {ui.about.update.needsFullDownload}
              {:else if updates.decision?.kind === 'no_build'}
                {ui.about.update.noBuild}
              {:else if updates.decision?.kind === 'no_release'}
                {ui.about.update.noRelease}
              {:else if updates.decision}
                {ui.about.update.upToDate}
              {/if}
            </p>
            {#if updates.decision?.kind === 'available' && updates.decision.notes}
              <!-- 更新日志。来自清单的 notes，由 CHANGELOG.md 那一节生成。 -->
              <pre class="notes selectable">{updates.decision.notes}</pre>
            {/if}
            <div class="links">
              {#if updates.installed}
                <!--
                  重启由用户按，不自动。装好的那一刻他可能正在游戏里——
                  Fern 退出会不会带走游戏进程取决于 process.rs，赌不起。
                -->
                {#if Object.keys(launch.games).length > 0}
                  <span class="t-quiet">{ui.about.update.restartBlocked}</span>
                {:else}
                  <Button variant="ghost" onclick={() => updates.restart()}>
                    {ui.about.update.restart}
                  </Button>
                {/if}
              {:else}
                <Button
                  variant="ghost"
                  disabled={updates.checking || updates.applying}
                  onclick={() => void updates.check()}>
                  {ui.about.update.check}
                </Button>
                {#if updates.decision?.kind === 'available'}
                  {@const url = updates.decision.url}
                  {#if updates.selfUpdate}
                    <Button
                      variant="link"
                      disabled={updates.applying}
                      onclick={() => void updates.apply()}>
                      {ui.about.update.apply}
                    </Button>
                  {:else}
                    <!--
                      包管理器装的那一份不自更新。落点是清单里的那个地址——
                      本平台的那一个文件，不是一个要人自己找的页面。
                    -->
                    <Button variant="link" onclick={() => openInBrowser(url)}>
                      {ui.about.update.download}
                    </Button>
                  {/if}
                {:else if updates.decision?.kind === 'needs_full_download'}
                  <Button variant="link" onclick={() => openInBrowser(DOWNLOADS)}>
                    {ui.about.update.download}
                  </Button>
                {/if}
              {/if}
            </div>
            <SegmentedControl
              aria-label={ui.about.update.automatic}
              value={updates.automatic ? 'on' : 'off'}
              onchange={(next) => updates.setAutomatic(next === 'on')}
              options={[
                { value: 'on', label: ui.about.update.automaticOn },
                { value: 'off', label: ui.about.update.automaticOff },
              ]}
            />
          </SettingRow>

          <SettingRow id="about/channel" found={focused === 'about/channel'}>
            <SegmentedControl
              aria-label={ui.about.update.channel}
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
              <Button variant="link" onclick={() => openInBrowser(REPOSITORY)}>
                {ui.about.repository}
              </Button>
              <Button variant="link" onclick={() => openInBrowser(`${REPOSITORY}/issues`)}>
                {ui.about.issues}
              </Button>
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

  .crumbs .crumb-back {
    justify-self: start;
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

  .tally {
    margin: 0 0 var(--s3);
    font-size: var(--t-small);
  }

  .group-head,
  .rt {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }

  .major {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  /* 组头承担状态：来这一页的问题是「我缺什么」，不是「我装了什么」。 */
  .state {
    margin-right: auto;
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  .state.missing {
    color: var(--danger);
  }

  .who {
    margin: 0;
    padding-left: var(--s4);
    font-size: var(--t-micro);
  }

  /* 具体的安装缩进一级：它们从属于上面那个大版本。整行是进档案的入口。 */
  .rt {
    width: 100%;
    padding: var(--s1) var(--s2) var(--s1) var(--s4);
    border-radius: var(--r1);
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .rt:hover {
    background: var(--tint-1);
  }

  .rt :global(svg) {
    flex: none;
    color: var(--ink-4);
  }

  .rt-name {
    display: grid;
    gap: 1px;
    flex: 1;
    min-width: 0;
    color: var(--ink-2);
    font-size: var(--t-body);
  }

  .rt-name small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .badge {
    flex: none;
    color: var(--accent);
    font-size: var(--t-micro);
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

  /* 结论在上、尺在下：读到的第一件事是那个数，不是一根线。 */
  .ceiling-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s3);
  }

  .amount {
    flex: none;
    color: var(--ink);
    font-size: var(--t-h2);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
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
  /* 更新日志。等宽 + 保留换行：它是一份列表，不是一段话。 */
  .notes {
    margin: 0;
    max-height: 12em;
    overflow-y: auto;
    white-space: pre-wrap;
    font-size: 0.85em;
    line-height: 1.6;
    color: var(--ink-quiet, inherit);
  }

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
