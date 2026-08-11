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
  import { open } from '@tauri-apps/plugin-dialog'
  import { Check, ChevronLeft, ChevronRight, Copy, FolderOpen, X } from 'lucide-svelte'
  import AccountList from '../components/AccountList.svelte'
  import AccountProfile from '../components/AccountProfile.svelte'
  import AddAccount from '../components/AddAccount.svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import AboutHero from '../components/AboutHero.svelte'
  import JavaRuntimeProfile from 'fern-kit/parts/JavaRuntimeProfile.svelte'
  import MemoryMeter from 'fern-kit/ui/MemoryMeter.svelte'
  import SettingRow from '../components/SettingRow.svelte'
  import SegmentedControl from 'fern-kit/ui/SegmentedControl.svelte'
  import { javaLabel, megabytes, type JavaGroup } from 'fern-kit/parts/java'
  import Form from '../layouts/Form.svelte'
  import { ACCENT_PRESETS, theme } from '../lib/theme.svelte'
  import { accounts, type AccountKind } from '../lib/accounts.svelte'
  import { SETTINGS_SECTIONS } from '../lib/settings-catalog'
  import { ui } from '../lib/i18n'
  import { expand } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'
  import { launch } from '../lib/launch.svelte'
  import { prefs, suggestedSource } from '../lib/prefs.svelte'
  import { updates } from '../lib/update.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { nameList } from '../lib/backup'
  import { backupUsage } from '../lib/backup'
  import {
    clearCache,
    clearLogs,
    instanceStorage,
    slimApply,
    slimBytes,
    slimPreview,
    storageReport,
    type SlimPlan,
    type StorageReport,
  } from '../lib/storage'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

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
   * 所以加了第二级：**一行可以是一个入口。** 语法在 `nav.settingsRoute` 里定义，
   * 第三段起就是这一级。它和场景的纵深不只是「同一套语法」——它就是同一套：
   * 前两段是锚点（分区、行）落在参数里，第三段起才占路径，所以 `nav.up()`
   * 一视同仁地去掉最后一段。上一版这里自己写了一份 `slice(0, 2)`，于是四段
   * 长的位置会被它一步退掉两级。
   */
  const location = $derived(at.split('/').filter(Boolean))
  /** 二级页属于哪一行。`分区/行`。 */
  const page = $derived(location.slice(0, 2).join('/'))
  const target = $derived(location.length >= 3 ? location[2] : '')
  /**
   * 目标自己还带的一段。
   *
   * 目前只有一处用得上：`account/list/new/microsoft` 说的是「添加账户，而且
   * 已经选好了哪一种」——那一步在名单末尾那颗按钮上就做完了。不带这一段的
   * `account/list/new` 仍然成立（⌘K 就落在那儿），只是要再问一次。
   */
  const detail = $derived(location.length >= 4 ? location[3]! : '')

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

  // ——— 迁移数据目录 ———
  //
  // 系统目录选择器挑目标（和「现有游戏目录」同一个选择器），后端把「挑了个
  // 非空目录」落到其中的 Fern 子目录；确认那一步展示的就是最终路径。同一
  // 磁盘瞬间完成；跨磁盘的复制走任务岛显示字节进度。

  /** 选好的最终目的地。空串表示还没在选择器里挑。 */
  let migrateTo = $state('')
  let migrating = $state(false)

  async function pickMigrateTarget() {
    pathError = ''
    const picked = await open({ directory: true, multiple: false, title: '选择新的数据目录' })
    if (typeof picked !== 'string') return
    try {
      migrateTo = await invoke('migration_target', { picked })
    } catch (error) {
      pathError = String(error)
    }
  }

  async function migrateData() {
    migrating = true
    pathError = ''
    try {
      await invoke('migrate_data', { destination: migrateTo, title: '迁移数据目录' })
      migrateTo = ''
      // 完成即生效：每条命令都现解析数据根。把这一屏的路径换成新的。
      paths = await invoke('data_paths')
    } catch (error) {
      pathError = String(error)
    } finally {
      migrating = false
    }
  }

  // ——— 存储 ———
  //
  // 这里是一张桶表：每个桶一行，「多大、可省多少、动手删」都长在自己的
  // 行上。可回收注解靠预检自动跑出来（只读）——没有那句注解，「清理」和
  // 「瘦身」就会退化成两块和数字对不上号的独立控件。

  /** 分区报告。undefined 表示还没量完。 */
  let storage = $state<StorageReport | undefined>()
  let storageError = $state('')
  let storageBusy = $state<'' | 'cache' | 'logs' | 'shared' | 'java'>('')
  /** 每实例的字节数，量出来一个填一个——几十 GB 的实例不挡住整张报告。 */
  let instanceBytes = $state<Record<string, number>>({})
  /** 快照里模组占多少。快照桶那句注解。 */
  let snapshotModsBytes = $state<number | undefined>()
  /** 瘦身预检。undefined 表示还在查。 */
  let slimPlan = $state<SlimPlan | undefined>()
  let slimPick = $state({ versions: true, libraries: true, assets: true })
  /** 展开的那一桶。一次只开一个：展开是回答追问，不是多摆几张表。 */
  let expanded = $state<'' | 'instances' | 'shared' | 'java'>('')
  /** 清完之后那一行说的话（「已释放 X」），按桶各记各的。 */
  let freed = $state<Record<string, string>>({})

  /** 大的排前面——来这一行的问题是「占用都在哪」。还没量完的排最后。 */
  const sizedInstances = $derived(
    [...instances.list].sort(
      (left, right) => (instanceBytes[right.id] ?? -1) - (instanceBytes[left.id] ?? -1),
    ),
  )

  /** 收起时的一行明细：前三名 + 总数。 */
  const instanceSummary = $derived.by(() => {
    const parts = sizedInstances
      .slice(0, 3)
      .map(
        (item) =>
          `${item.name} ${instanceBytes[item.id] !== undefined ? formatBytes(instanceBytes[item.id]!) : '…'}`,
      )
    const rest = sizedInstances.length - 3
    return parts.join(' · ') + (rest > 0 ? ` 等 ${sizedInstances.length} 个` : '')
  })

  const sharedReclaim = $derived(
    !slimPlan ? 0 : slimPlan.versionsBytes + slimPlan.librariesBytes + slimPlan.assetsBytes,
  )
  const sharedPicked = $derived.by(() => {
    if (!slimPlan) return 0
    return (
      (slimPick.versions ? slimPlan.versionsBytes : 0) +
      (slimPick.libraries ? slimPlan.librariesBytes : 0) +
      (slimPick.assets ? slimPlan.assetsBytes : 0)
    )
  })

  /** 桶的次序即比例条的分段次序。固定次序，颜色才稳定。 */
  const buckets = $derived.by(() => {
    if (!storage) return []
    return [
      storage.instances,
      storage.snapshots,
      storage.versions + storage.libraries + storage.assets,
      storage.runtimes,
      storage.cache,
      storage.logs,
      storage.other,
    ]
  })

  const toggle = (key: 'instances' | 'shared' | 'java') => {
    expanded = expanded === key ? '' : key
  }

  /** 量一遍。只在第一次进这一节时做：遍历整个数据根不是免费的。 */
  let measured = false
  $effect(() => {
    if (section !== 'data' || measured) return
    measured = true
    void loadStorage()
  })

  async function loadStorage() {
    if (!inTauri()) return
    storageError = ''
    try {
      storage = await storageReport()
    } catch (error) {
      storageError = String(error)
      return
    }
    // 注解各自到位，谁也不等谁。
    void backupUsage()
      .then((usage) => (snapshotModsBytes = usage.modsBytes))
      .catch(() => {})
    void previewSlim()
    // 设置可以在实例列表还没加载时打开（冷启动直奔 ⌘K）。
    if (instances.list.length === 0) await instances.load()
    // 实例挨个量，不并发——同时遍历十个目录只会让磁盘更慢。
    for (const item of instances.list) {
      try {
        instanceBytes[item.id] = await instanceStorage(item.id)
      } catch {
        // 单个实例量不出来不挡整页，那一行显示不出数字本身就是信息。
      }
    }
  }

  async function previewSlim() {
    try {
      slimPlan = await slimPreview()
      slimPick = { versions: true, libraries: true, assets: true }
    } catch (error) {
      storageError = String(error)
    }
  }

  async function clean(kind: 'cache' | 'logs') {
    storageBusy = kind
    storageError = ''
    try {
      const bytes = kind === 'cache' ? await clearCache() : await clearLogs()
      freed[kind] = `已释放 ${formatBytes(bytes)}`
      // 就地扣掉，不整棵重量：省下多少是后端刚数完的，账仍然是平的。
      if (storage) {
        storage = { ...storage, total: storage.total - bytes, [kind]: 0 }
      }
    } catch (error) {
      storageError = String(error)
    } finally {
      storageBusy = ''
    }
  }

  async function applySlim(kind: 'shared' | 'java') {
    storageBusy = kind
    storageError = ''
    try {
      const done = await slimApply(
        kind === 'shared'
          ? {
              versions: slimPick.versions,
              libraries: slimPick.libraries,
              assets: slimPick.assets,
              runtimes: false,
            }
          : { versions: false, libraries: false, assets: false, runtimes: true },
      )
      freed[kind] = `已释放 ${formatBytes(slimBytes(done))}`
      expanded = ''
      if (storage) {
        storage = {
          ...storage,
          total: storage.total - slimBytes(done),
          versions: storage.versions - done.versionsBytes,
          libraries: storage.libraries - done.librariesBytes,
          assets: storage.assets - done.assetsBytes,
          runtimes: storage.runtimes - done.runtimesBytes,
        }
      }
      // 注解按新的现实重算——预检与执行同一套判定，这里也不例外。
      slimPlan = undefined
      void previewSlim()
      // 删掉的可能包括 Java 运行时，那一节的清单要跟上。
      if (kind === 'java') void loadRuntimes()
    } catch (error) {
      storageError = String(error)
    } finally {
      storageBusy = ''
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
            <Button variant="link" tone="quiet" onclick={() => nav.up()}>
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
            ongone={() => nav.settings('java/runtimes')}
            remove={removeRuntime}
            forget={forgetJavaPath}
          />
        {:else if target === 'new'}
          <!-- 换一个种类就是换一屏：那一屏的第一步已经由上一层做完了。 -->
          {#key detail}
            <AddAccount
              initial={detail as AccountKind | ''}
              ondone={(id) => nav.settings(`account/list${id ? `/${id}` : ''}`)}
            />
          {/key}
        {:else}
          <AccountProfile
            accountId={target}
            ongone={() => nav.settings('account/list')}
          />
        {/if}
      </div>
    </div>
  {:else}
  <!--
    换一节也写进地址。它是横向的（`nav.settings` 对同深度的位置用 replace，
    不压栈），但它是「我在哪」的一部分——上一版只有被 ⌘K 送进来时才记，自己
    点的那些换节一律不留痕，于是刷新回到外观、后退也回不到刚才那一节。
  -->
  <Form {sections} {section} onsection={(id) => nav.settings(id)}>
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
              <Input mono class="selectable" aria-label="主题码" bind:value={themeCode} spellcheck="false" />
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
                <Input
                  class="size"
                  type="number"
                  aria-label="窗口宽度"
                  value={resolution.width}
                  oninput={(event) =>
                    setResolution(Number(event.currentTarget.value), resolution.height)}
                />
                <span class="t-quiet">×</span>
                <Input
                  class="size"
                  type="number"
                  aria-label="窗口高度"
                  value={resolution.height}
                  oninput={(event) =>
                    setResolution(resolution.width, Number(event.currentTarget.value))}
                />
              {/if}
            </div>
          </SettingRow>

          <SettingRow id="game/jvm" found={focused === 'game/jvm'}>
            <Input
              mono
              aria-label="JVM 参数"
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
                    <button class="rt" onclick={() => nav.settings(profileAt(item.home))}>
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
              <Input
                mono
                aria-label="Java 路径"
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
            <div class="paths">
              <div class="path-line">
                <span class="path-label t-quiet">数据目录</span>
                <span class="path t-mono selectable">{paths.root || '—'}</span>
              </div>
              {#if paths.portable}
                <p class="t-quiet hint">
                  数据目录随可执行文件所在位置。移动整个文件夹即可迁移全部数据。
                </p>
              {/if}
              <div class="path-line">
                <span class="path-label t-quiet">游戏目录</span>
                <span class="path t-mono selectable">{paths.game || '—'}</span>
              </div>
              <div class="path-line">
                <span class="path-label t-quiet">日志目录</span>
                <span class="path t-mono selectable">{paths.logs || '—'}</span>
                <Button
                  variant="icon"
                  tone="quiet"
                  aria-label="打开日志目录"
                  onclick={() => void openLogs()}
                >
                  <FolderOpen size={14} strokeWidth={1.8} />
                </Button>
              </div>

              <!-- 便携模式不迁移：拷走整个文件夹就是它的迁移方式，上面那句
                   提示已经说了。 -->
              {#if !paths.portable}
                {#if !migrateTo}
                  <div class="migrate-entry">
                    <Button variant="link" disabled={migrating} onclick={() => void pickMigrateTarget()}>
                      迁移到其他位置…
                    </Button>
                  </div>
                {:else}
                  <div class="expand migrate-confirm">
                    <p class="path t-mono selectable">{migrateTo}</p>
                    <p class="consequence">
                      会把整个数据目录移动到上面的位置{#if storage}（共 {formatBytes(
                          storage.total,
                        )}）{/if}。同一磁盘上瞬间完成；跨磁盘需要复制一段时间，进度显示在任务岛上，期间请不要关闭
                      Fern。完成后立即生效，原位置只留下一份指路的说明文件。
                    </p>
                    <div class="expand-actions">
                      <Button variant="ghost" disabled={migrating} onclick={() => (migrateTo = '')}>
                        取消
                      </Button>
                      <Button variant="primary" disabled={migrating} onclick={() => void migrateData()}>
                        {migrating ? '正在迁移……' : '迁移'}
                      </Button>
                    </div>
                  </div>
                {/if}
              {/if}
            </div>
          </SettingRow>

          <SettingRow id="data/usage" found={focused === 'data/usage'}>
            {#if !storage}
              <p class="t-quiet">{storageError || '正在测量……'}</p>
            {:else}
              {#if storage.total > 0}
                <!-- 比例条：数字是确认用的，大头在哪一眼要能看出来。 -->
                <div class="bar" role="img" aria-label="各部分占用比例">
                  {#each buckets as bytes, index (index)}
                    {#if bytes > 0}
                      <span class="seg tone-{index}" style:flex-grow={bytes}></span>
                    {/if}
                  {/each}
                </div>
              {/if}
              <p class="tally t-quiet">共占用 {formatBytes(storage.total)}</p>

              <ul class="bucket-table">
                <li>
                  <div class="bucket-head">
                    <span class="dot tone-0"></span>
                    <span class="bucket-name">实例</span>
                    <span class="bytes">{formatBytes(storage.instances)}</span>
                  </div>
                  {#if sizedInstances.length > 0}
                    <button class="sub sub-toggle" onclick={() => toggle('instances')}>
                      {expanded === 'instances' ? '收起' : instanceSummary}
                    </button>
                    {#if expanded === 'instances'}
                      <ul class="detail">
                        {#each sizedInstances as item (item.id)}
                          <li>
                            <span class="detail-name">
                              {item.name}
                              {#if item.external}
                                <small class="t-quiet">外部目录，游戏文件不计入</small>
                              {/if}
                            </span>
                            <span class="bytes">
                              {instanceBytes[item.id] !== undefined
                                ? formatBytes(instanceBytes[item.id]!)
                                : '…'}
                            </span>
                          </li>
                        {/each}
                      </ul>
                    {/if}
                  {/if}
                </li>

                <li>
                  <div class="bucket-head">
                    <span class="dot tone-1"></span>
                    <span class="bucket-name">快照</span>
                    <span class="bytes">{formatBytes(storage.snapshots)}</span>
                  </div>
                  {#if snapshotModsBytes !== undefined && snapshotModsBytes > 0}
                    <p class="sub t-quiet">其中模组 {formatBytes(snapshotModsBytes)}</p>
                  {/if}
                </li>

                <li>
                  <div class="bucket-head">
                    <span class="dot tone-2"></span>
                    <span class="bucket-name">共享游戏文件</span>
                    <span class="bytes">
                      {formatBytes(storage.versions + storage.libraries + storage.assets)}
                    </span>
                  </div>
                  {#if !slimPlan && !freed.shared}
                    <p class="sub t-quiet">正在检查引用……</p>
                  {:else if sharedReclaim > 0}
                    <div class="sub reclaim">
                      <span>{formatBytes(sharedReclaim)} 未被任何实例使用</span>
                      <Button variant="link" onclick={() => toggle('shared')}>
                        {expanded === 'shared' ? '收起' : '清除…'}
                      </Button>
                    </div>
                    {#if expanded === 'shared' && slimPlan}
                      <div class="expand">
                        {#if slimPlan.versions.length > 0}
                          <label>
                            <input type="checkbox" bind:checked={slimPick.versions} />
                            <span class="pick-name">
                              没有实例使用的版本：{nameList(slimPlan.versions)}
                            </span>
                            <span class="bytes">{formatBytes(slimPlan.versionsBytes)}</span>
                          </label>
                        {/if}
                        {#if slimPlan.librariesFiles > 0}
                          <label>
                            <input type="checkbox" bind:checked={slimPick.libraries} />
                            <span class="pick-name">
                              未被引用的依赖库（{slimPlan.librariesFiles} 个文件）
                            </span>
                            <span class="bytes">{formatBytes(slimPlan.librariesBytes)}</span>
                          </label>
                        {/if}
                        {#if slimPlan.assetsFiles > 0}
                          <label>
                            <input type="checkbox" bind:checked={slimPick.assets} />
                            <span class="pick-name">
                              未被引用的资源（{slimPlan.assetsFiles} 个文件）
                            </span>
                            <span class="bytes">{formatBytes(slimPlan.assetsBytes)}</span>
                          </label>
                        {/if}
                        <!-- 按下去会发生什么，先说清再给按钮——和恢复快照同一个规矩。 -->
                        <p class="consequence">
                          将释放 {formatBytes(sharedPicked)}。清除的内容需要时会重新下载。
                        </p>
                        <div class="expand-actions">
                          <Button variant="ghost" onclick={() => (expanded = '')}>取消</Button>
                          <Button
                            variant="primary"
                            disabled={sharedPicked === 0 || storageBusy !== ''}
                            onclick={() => void applySlim('shared')}
                          >
                            {storageBusy === 'shared' ? '正在清除……' : '清除'}
                          </Button>
                        </div>
                      </div>
                    {/if}
                  {:else if freed.shared}
                    <p class="sub t-quiet">{freed.shared}</p>
                  {/if}
                </li>

                <li>
                  <div class="bucket-head">
                    <span class="dot tone-3"></span>
                    <span class="bucket-name">Java 运行时</span>
                    <span class="bytes">{formatBytes(storage.runtimes)}</span>
                  </div>
                  {#if slimPlan && slimPlan.runtimes.length > 0}
                    <div class="sub reclaim">
                      <span>{nameList(slimPlan.runtimes)} 无实例需要</span>
                      <Button variant="link" onclick={() => toggle('java')}>
                        {expanded === 'java' ? '收起' : '清除…'}
                      </Button>
                    </div>
                    {#if expanded === 'java'}
                      <div class="expand">
                        <p class="consequence">
                          将删除 {nameList(slimPlan.runtimes)}，释放
                          {formatBytes(slimPlan.runtimesBytes)}。需要它的实例下次启动时会重新下载。
                        </p>
                        <div class="expand-actions">
                          <Button variant="ghost" onclick={() => (expanded = '')}>取消</Button>
                          <Button
                            variant="primary"
                            disabled={storageBusy !== ''}
                            onclick={() => void applySlim('java')}
                          >
                            {storageBusy === 'java' ? '正在清除……' : '清除'}
                          </Button>
                        </div>
                      </div>
                    {/if}
                  {:else if freed.java}
                    <p class="sub t-quiet">{freed.java}</p>
                  {/if}
                </li>

                <li>
                  <div class="bucket-head">
                    <span class="dot tone-4"></span>
                    <span class="bucket-name">元数据缓存</span>
                    {#if storage.cache > 0}
                      <Button
                        variant="link"
                        disabled={storageBusy !== ''}
                        onclick={() => void clean('cache')}
                      >
                        {storageBusy === 'cache' ? '正在清除……' : '清除'}
                      </Button>
                    {:else if freed.cache}
                      <span class="t-quiet freed-note">{freed.cache}</span>
                    {/if}
                    <span class="bytes">{formatBytes(storage.cache)}</span>
                  </div>
                </li>

                <li>
                  <div class="bucket-head">
                    <span class="dot tone-5"></span>
                    <span class="bucket-name">日志</span>
                    {#if storage.logs > 0}
                      <Button
                        variant="link"
                        disabled={storageBusy !== ''}
                        onclick={() => void clean('logs')}
                      >
                        {storageBusy === 'logs' ? '正在清除……' : '清除'}
                      </Button>
                    {:else if freed.logs}
                      <span class="t-quiet freed-note">{freed.logs}</span>
                    {/if}
                    <span class="bytes">{formatBytes(storage.logs)}</span>
                  </div>
                </li>

                {#if storage.other > 0}
                  <li>
                    <div class="bucket-head">
                      <span class="dot tone-6"></span>
                      <span class="bucket-name">其他</span>
                      <span class="bytes">{formatBytes(storage.other)}</span>
                    </div>
                    <p class="sub t-quiet">设置、来源记录等零散文件</p>
                  </li>
                {/if}
              </ul>
            {/if}
          </SettingRow>

          <SettingRow id="data/existing" found={focused === 'data/existing'}>
            <Button variant="ghost" onclick={() => nav.settings('data/existing/browse')}>
              选择目录…
            </Button>
          </SettingRow>

          {#if pathError}<div class="alert">{pathError}</div>{/if}
          {#if storageError && storage}<div class="alert">{storageError}</div>{/if}
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

  /* 目录：三条路径并成一行设置，小标签在左，路径等宽字体。 */
  .paths {
    display: grid;
    gap: var(--s2);
  }

  .path-line {
    display: flex;
    align-items: center;
    gap: var(--s3);
    min-width: 0;
  }

  .path-label {
    flex: none;
    width: 4em;
    font-size: var(--t-micro);
  }

  /* 比例条：大头在哪一眼看出来，数字是确认用的。 */
  .bar {
    display: flex;
    height: 6px;
    margin: 0 0 var(--s2);
    border-radius: 3px;
    overflow: hidden;
  }

  .seg {
    min-width: 1px;
  }

  /* 同一支色阶从强到弱：桶有固定次序，颜色跟着次序走，不各自抢戏。 */
  .tone-0 { background: color-mix(in srgb, var(--accent) 90%, transparent); }
  .tone-1 { background: color-mix(in srgb, var(--accent) 68%, transparent); }
  .tone-2 { background: color-mix(in srgb, var(--accent) 50%, transparent); }
  .tone-3 { background: color-mix(in srgb, var(--accent) 36%, transparent); }
  .tone-4 { background: color-mix(in srgb, var(--accent) 25%, transparent); }
  .tone-5 { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .tone-6 { background: color-mix(in srgb, var(--accent) 10%, transparent); }

  /* 桶表：每桶一行，形状完全一样——色点、名字、数字；注解和动作长在
     自己的行上，「看、可省多少、删」不再分家。 */
  .bucket-table {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .bucket-table > li {
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .bucket-table > li:last-child {
    box-shadow: none;
  }

  .bucket-head {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    color: var(--ink-2);
    font-size: var(--t-body);
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    align-self: center;
  }

  .bucket-name {
    flex: 1;
    min-width: 0;
  }

  .bytes {
    flex: none;
    color: var(--ink);
    font-variant-numeric: tabular-nums;
  }

  /* 桶的第二行：注解、明细摘要。缩进对齐到名字，不对齐到色点。 */
  .sub {
    margin: var(--s1) 0 0;
    padding-left: calc(8px + var(--s2));
    font-size: var(--t-micro);
  }

  .sub-toggle {
    display: block;
    color: var(--ink-4);
    text-align: left;
    cursor: pointer;
    transition: color var(--t-fast) var(--ease);
  }

  .sub-toggle:hover {
    color: var(--ink-2);
  }

  .reclaim {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s3);
    color: var(--ink-3);
  }

  .freed-note {
    flex: none;
    font-size: var(--t-micro);
  }

  .detail {
    display: grid;
    gap: var(--s1);
    margin: var(--s2) 0 0;
    padding: 0 0 0 calc(8px + var(--s2));
    list-style: none;
  }

  .detail li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s3);
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  .detail-name small {
    margin-left: var(--s2);
    font-size: var(--t-micro);
  }

  /* 就地展开的确认区：勾选、后果句、按钮。不弹窗——决定在它作用的那一行
     旁边做。 */
  .expand {
    display: grid;
    gap: var(--s2);
    margin-top: var(--s2);
    padding-left: calc(8px + var(--s2));
  }

  .expand label {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    color: var(--ink-2);
    font-size: var(--t-small);
    cursor: pointer;
  }

  .pick-name {
    flex: 1;
    min-width: 0;
  }

  .migrate-confirm {
    padding-left: 0;
  }

  /* 后果句。这一段是展开区最重要的一行：说清按下去会发生什么。 */
  .consequence {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    line-height: 1.7;
  }

  .expand-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
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

  /* 布局归调用方，作用域样式进不了组件，罩一层自己的祖先。 */
  .slider-row :global(.size) {
    width: 88px;
  }

  .code-row {
    display: flex;
    gap: var(--s2);
  }

  .code-row :global(.input) {
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
