<script lang="ts">
  /**
   * 新建实例——实例场景向内的那一级。
   *
   * ## 这一屏只问必须问的
   *
   * 建一个实例，**必填的信息只有一个：游戏版本**。名字能默认，加载器版本能
   * 默认，加载器本身是派生决定——它取决于你要装什么模组。所以字段按这个顺序
   * 排，其余渐进展开，打开就能回车。
   *
   * 上一版把六个字段并列摆在两列里，八百条版本列表常驻在右半屏，而「创建」
   * 埋在右下角。它把六种完全不同的意图（玩最新原版 / 装某个模组 / 有个整合包
   * / 和朋友对版本 / 开老版本 / 测模组）压进了同一张表。
   *
   * ## 版本为什么这么摆
   *
   * 实测（见 `lib/versions.ts`）：905 个版本里正式版只有 102 个，分 24 代，
   * 最大一代 12 条。「版本太多」几乎全部来自快照。所以正式版按代折叠，一列
   * 整行；快照是**并列的另一类**，不是埋在某一代底下——`24w14a` 属于哪一代，
   * 名字上根本看不出来，埋起来等于要求用户先知道答案才能找到它。
   *
   * 常用路径都是零展开：最新正式版是打开就选中的那一个，最新快照是切过去的
   * 第一条。只有翻历史版本才展开一层。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { open } from '@tauri-apps/plugin-dialog'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { onMount } from 'svelte'
  import { Check, ChevronDown, Package, Plus } from 'lucide-svelte'
  import SegmentedControl from 'fern-kit/ui/SegmentedControl.svelte'
  import Loading from '../components/Loading.svelte'
  import { EXISTING, instances, inTauri, type LoaderOption } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { suggestName } from '../lib/naming'
  import { nav } from '../lib/nav.svelte'
  import { expand, pop, unfold } from '../lib/motion'
  import { ancient, generations, newestRelease, newestSnapshot, snapshots } from '../lib/versions'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

  interface LoaderVersion {
    version: string
    stable: boolean
  }

  const TITLE = '新建实例'
  nav.name(TITLE)

  let name = $state('')
  /** 用户自己动过名字之后就不再替他改。 */
  let named = $state(false)
  let picked = $state('')
  let loader = $state('vanilla')
  let loaders = $state<LoaderOption[]>([{ kind: 'vanilla', label: '原版' }])
  let addons = $state<LoaderOption[]>([])
  /** 勾上的附加层。 */
  let extras = $state<string[]>([])
  let loaderVersion = $state('')
  let loaderVersions = $state<LoaderVersion[]>([])
  let choosingLoaderVersion = $state(false)
  let busy = $state(false)
  let error = $state('')

  /** 版本选择器展开了没有，以及展开之后停在哪一档、哪一代。 */
  let choosing = $state(false)
  let channel = $state<'release' | 'snapshot'>('release')
  let openGeneration = $state('')
  let query = $state('')

  /** 拖进来的整合包。看过一眼之后才装——先说清里面是什么。 */
  interface PackSummary {
    name: string
    version: string
    summary: string
    gameVersion: string
    loader: string
    loaderVersion: string
    files: number
  }
  let pack = $state<{ path: string; summary: PackSummary } | null>(null)
  let dropping = $state(false)

  const all = $derived(instances.versions)
  const eras = $derived(generations(all))
  const oldest = $derived(ancient(all))
  const feed = $derived(snapshots(all))
  const latest = $derived(newestRelease(all))
  const newestFeed = $derived(newestSnapshot(all))
  /** 展开之后平铺的那一代——最新的那一代，通常只有一两条。 */
  const current = $derived(eras[0])
  const older = $derived(eras.slice(1))
  /*
   * 搜索之外**不截断**。上一版只铺前 60 条，于是「所有快照」实际上是
   * 「最近两个月的快照」，而界面上没有任何东西说明后面还有——一个看不见的
   * 上限比一个明说的分页更坏。
   *
   * 742 行就整份铺出来。实测（无头 Chromium，比真机慢）切到快照那一档要
   * 60ms，搜索时每次改词 11–42ms，都在可以接受的范围里。试过给行加
   * `content-visibility: auto`：渲染只快 10ms，却让滚动条的量程在拖到底的
   * 那一下自己变长——省下的那点远不够抵这个。
   */
  const matched = $derived(
    query.trim()
      ? feed.filter((item) => item.id.toLowerCase().includes(query.trim().toLowerCase()))
      : feed,
  )

  const loaderLabel = $derived(loaders.find((item) => item.kind === loader)?.label ?? '原版')
  /*
   * 稳定版优先，但一个也没有时就把全部摆出来——某些加载器在某些版本上只有
   * 测试版，那时候空着一片比给个测试版更糟。
   */
  const stableLoaderVersions = $derived(loaderVersions.filter((item) => item.stable))
  const shownLoaderVersions = $derived(
    stableLoaderVersions.length > 0 ? stableLoaderVersions : loaderVersions,
  )
  /** 主加载器那一排。附加层不在这里——它是另一个问题。 */
  const primaries = $derived(
    loaders.filter((option) => !option.stackable || option.kind === loader),
  )

  // 打开就选中最新正式版：绝大多数人要的就是它，一次点击都不用。
  $effect(() => {
    if (!picked && latest) picked = latest.id
  })

  // 名字跟着选择走，直到用户自己接手。
  $effect(() => {
    if (!named) {
      name = suggestName(
        picked,
        loaderLabel,
        instances.list.map((item) => item.name),
      )
    }
  })

  /** 换了加载器或版本，之前挑的加载器版本就不作数了。 */
  $effect(() => {
    loader
    picked
    loaderVersion = ''
    choosingLoaderVersion = false
    loaderVersions = []
  })

  async function loadLoaderVersions() {
    choosingLoaderVersion = true
    if (!inTauri() || loaderVersions.length > 0) return
    try {
      loaderVersions = await invoke<LoaderVersion[]>('list_loader_versions', {
        loader,
        gameVersion: picked,
      })
    } catch (cause) {
      error = String(cause)
    }
  }

  function choose(id: string) {
    picked = id
    choosing = false
    openGeneration = ''
    query = ''
  }

  function toggleGeneration(name: string) {
    openGeneration = openGeneration === name ? '' : name
  }

  function toggleExtra(kind: string) {
    extras = extras.includes(kind)
      ? extras.filter((item) => item !== kind)
      : [...extras, kind]
  }

  async function submit() {
    const trimmed = name.trim()
    if (!trimmed) return (error = '请输入实例名称')
    if (!picked) return (error = '请选择 Minecraft 版本')
    busy = true
    error = ''
    try {
      const created = await instances.create(trimmed, picked, loader, loaderVersion)
      // 附加层建完再叠：建实例那一步收的是一个加载器，而附加层是另一个问题。
      for (const kind of extras) {
        await invoke('add_instance_component', { instanceId: created.id, loader: kind })
      }
      // 建完直接落到它的详情页：刚建的东西该能立刻看见，而不是回到网格里自己找。
      // 用 replace 顶掉这一页：这张表单已经交出去了，后退回到它没有意义。
      instances.select(created.id)
      nav.replace(['instances', created.id])
      /*
       * 建完立刻开始准备，不等第一次点启动。
       *
       * 建实例本身只是把选择写进一个 json，瞬间完成；装加载器、补全文件才是
       * 花时间的部分。上一版把它们推迟到第一次启动——于是曲库里躺着一个看起来
       * 一切正常的 Forge 实例，直到你点「启动」的那一刻才开始跑 Forge 安装器，
       * 一等好几分钟，而用户以为自己只是点了启动。
       *
       * 不 await：这一页的活已经干完了，进度归实例页和岛。
       */
      void launch.repair(created.id, `准备 ${created.name}`, false)
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }

  /**
   * 整合包走的是另一条路：它自带游戏版本和加载器，装它就是建一个实例，
   * 所以这一页上面那些选择在它面前全部作废——直接换成一张说明卡。
   */
  async function inspect(paths: string[]) {
    const file = paths.find((path) => path.toLowerCase().endsWith('.mrpack'))
    if (!file) {
      error = '只能导入 .mrpack 整合包'
      return
    }
    try {
      pack = { path: file, summary: await invoke<PackSummary>('inspect_modpack', { path: file }) }
      error = ''
    } catch (cause) {
      error = String(cause)
    }
  }

  /** 拖进来是一条路，自己去选也得有一条——不是每个人都想拖。 */
  async function pickPack() {
    if (!inTauri()) return
    const chosen = await open({
      multiple: false,
      title: '选择整合包',
      filters: [{ name: '整合包', extensions: ['mrpack'] }],
    })
    if (typeof chosen === 'string') await inspect([chosen])
  }

  async function importPack() {
    if (!pack) return
    busy = true
    error = ''
    try {
      const created = await invoke<{ id: string }>('import_modpack', {
        path: pack.path,
        name: name.trim() || null,
        title: `导入 ${pack.summary.name}`,
        subjects: [],
      })
      await instances.load()
      instances.select(created.id)
      nav.replace(['instances', created.id])
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }

  // 整合包一读出来就把名字换成它的，除非用户已经自己写过。
  $effect(() => {
    if (pack && !named) name = pack.summary.name
  })

  onMount(() => {
    if (!inTauri()) return
    let stop: (() => void) | undefined
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === 'over') dropping = true
        else if (event.payload.type === 'drop') {
          dropping = false
          void inspect(event.payload.paths)
        } else dropping = false
      })
      .then((unlisten) => (stop = unlisten))
    return () => stop?.()
  })

  void instances.loadVersions()
  // 加载器与附加层都跟着选中的版本走：1.7.10 上没有 Fabric，1.21 上没有
  // LiteLoader。摆一个装不上的选项，等于让人走到一半才被拦住。
  $effect(() => {
    const version = picked
    if (!inTauri() || !version) return
    void instances.loadLoaders(version).then((list) => {
      if (list.length === 0) return
      loaders = list
      if (!list.some((option) => option.kind === loader)) {
        loader = 'vanilla'
        loaderVersion = ''
      }
    })
  })
  $effect(() => {
    const version = picked
    const primary = loader
    if (!inTauri() || !version) return
    void instances.loadAddons(version, primary).then((list) => {
      addons = list
      // 换了版本或主加载器之后，之前勾的那些可能已经叠不上去了。
      extras = extras.filter((kind) => list.some((option) => option.kind === kind))
    })
  })
</script>

<section class="create" class:dropping>
  <header>
    <h1 class="t-h1">{TITLE}</h1>
  </header>

  {#if pack}
    <div class="pack" in:expand>
      <span class="badge"><Package size={16} strokeWidth={1.8} /></span>
      <div class="pack-text">
        <strong>{pack.summary.name}</strong>
        {#if pack.summary.version}<small class="t-mono">{pack.summary.version}</small>{/if}
        {#if pack.summary.summary}<p class="t-quiet">{pack.summary.summary}</p>{/if}
        <p class="t-mono t-quiet">
          Minecraft {pack.summary.gameVersion}
          {#if pack.summary.loaderVersion}· {pack.summary.loader} {pack.summary.loaderVersion}{/if}
          · {pack.summary.files} 个文件
        </p>
      </div>
      <Button variant="link" onclick={() => (pack = null)}>换一个</Button>
    </div>

    <div class="fields scroll">
      <Input label="名称" bind:value={name} maxlength={64} oninput={() => (named = true)} />
    </div>
  {:else}
    <div class="fields scroll">
      <div class="field">
        <span class="label" id="version-label">Minecraft 版本</span>
        <button
          class="value"
          aria-labelledby="version-label"
          aria-expanded={choosing}
          onclick={() => (choosing = !choosing)}
        >
          <span class="t-mono">{picked || '选择版本'}</span>
          <span class="caret" class:up={choosing}><ChevronDown size={14} strokeWidth={2} /></span>
        </button>

        {#if choosing}
          <div class="picker" transition:unfold>
            <!--
              档位和搜索框留在滚动区外面：翻到第三百条快照时还想换回正式版，
              不该先滚回顶上。
            -->
            <div class="picker-head">
              <SegmentedControl
                aria-label="版本类型"
                value={channel}
                onchange={(next) => {
                  channel = next as 'release' | 'snapshot'
                  openGeneration = ''
                }}
                options={[
                  { value: 'release', label: '正式版' },
                  { value: 'snapshot', label: '快照' },
                ]}
              />
              {#if channel === 'snapshot' && !instances.versionsLoading}
                <Input
                  bind:value={query}
                  spellcheck="false"
                  aria-label="搜索快照"
                  placeholder="搜索 {feed.length} 个快照"
                />
              {/if}
            </div>

            {#if instances.versionsLoading}
              <Loading note="读取版本清单" />
            {:else if channel === 'release'}
              <!--
                最新那一代平铺，别的按代折起来。整行可点——一行里横排七八个
                号码，既难点也难看。
              -->
              <div class="list scroll">
                {#if current}
                  {#each current.versions as version (version.id)}
                    <button
                      class="row"
                      class:on={picked === version.id}
                      onclick={() => choose(version.id)}
                    >
                      <span class="t-mono grow">{version.id}</span>
                      {#if version.id === latest?.id}<span class="t-quiet">最新正式版</span>{/if}
                      {#if picked === version.id}
                        <span class="tick-mark" in:pop><Check size={14} strokeWidth={2.4} /></span>
                      {/if}
                    </button>
                  {/each}
                  <hr />
                {/if}
                {#each older as era (era.name)}
                  <button
                    class="row"
                    aria-expanded={openGeneration === era.name}
                    onclick={() => toggleGeneration(era.name)}
                  >
                    <span class="t-mono grow">{era.name}</span>
                    <span class="t-quiet">{era.versions.length} 个版本</span>
                    <span class="caret" class:up={openGeneration === era.name}>
                      <ChevronDown size={13} strokeWidth={2} />
                    </span>
                  </button>
                  {#if openGeneration === era.name}
                    <div class="nest" transition:unfold>
                      {#each era.versions as version (version.id)}
                        <button
                          class="row nested"
                          class:on={picked === version.id}
                          onclick={() => choose(version.id)}
                        >
                          <span class="t-mono grow">{version.id}</span>
                          {#if picked === version.id}
                            <span class="tick-mark" in:pop>
                              <Check size={14} strokeWidth={2.4} />
                            </span>
                          {/if}
                        </button>
                      {/each}
                    </div>
                  {/if}
                {/each}
                {#if oldest.length > 0}
                  <button
                    class="row"
                    aria-expanded={openGeneration === 'ancient'}
                    onclick={() => toggleGeneration('ancient')}
                  >
                    <span class="grow">远古</span>
                    <span class="t-quiet">{oldest.length} 个</span>
                    <span class="caret" class:up={openGeneration === 'ancient'}>
                      <ChevronDown size={13} strokeWidth={2} />
                    </span>
                  </button>
                  {#if openGeneration === 'ancient'}
                    <div class="nest" transition:unfold>
                      {#each oldest as version (version.id)}
                        <button
                          class="row nested"
                          class:on={picked === version.id}
                          onclick={() => choose(version.id)}
                        >
                          <span class="t-mono grow">{version.id}</span>
                          {#if picked === version.id}
                            <span class="tick-mark" in:pop>
                              <Check size={14} strokeWidth={2.4} />
                            </span>
                          {/if}
                        </button>
                      {/each}
                    </div>
                  {/if}
                {/if}
              </div>
            {:else}
              <!--
                快照不分组：742 条按时间倒序，要最新的人看第一条，要某一条的
                人知道自己在找什么，他要的是搜索。
              -->
              <div class="list scroll">
                {#each matched as version (version.id)}
                  <button
                    class="row"
                    class:on={picked === version.id}
                    onclick={() => choose(version.id)}
                  >
                    <span class="t-mono grow">{version.id}</span>
                    {#if version.id === newestFeed?.id}<span class="t-quiet">最新快照</span>{/if}
                    {#if picked === version.id}
                      <span class="tick-mark" in:pop><Check size={14} strokeWidth={2.4} /></span>
                    {/if}
                  </button>
                {/each}
                {#if matched.length === 0}
                  <p class="hint">没有匹配的快照</p>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      {#if primaries.length > 1}
        <div class="field">
          <span class="label" id="loader-label">加载器</span>
          <div class="loaders" role="group" aria-labelledby="loader-label">
            {#each primaries as option (option.kind)}
              <button
                class="chip"
                class:on={loader === option.kind}
                onclick={() => (loader = option.kind)}
              >
                {option.label}
              </button>
            {/each}
          </div>

          {#if loader !== 'vanilla'}
            {#if !choosingLoaderVersion}
              <div class="sub" transition:unfold>
                <span class="t-quiet">将安装最新稳定版</span>
                <Button variant="link" onclick={() => void loadLoaderVersions()}>指定版本</Button>
              </div>
            {:else if loaderVersions.length === 0}
              <Loading note="读取 {loaderLabel} 的版本" size={18} />
            {:else}
              <div class="picker" transition:unfold>
                <div class="list short scroll">
                  {#each shownLoaderVersions as item (item.version)}
                    <button
                      class="row"
                      class:on={loaderVersion === item.version}
                      onclick={() => (loaderVersion = item.version)}
                    >
                      <span class="t-mono grow">{item.version}</span>
                      {#if !item.stable}<span class="t-quiet">测试版</span>{/if}
                      {#if loaderVersion === item.version}
                        <span class="tick-mark" in:pop><Check size={13} strokeWidth={2.4} /></span>
                      {/if}
                    </button>
                  {/each}
                </div>
              </div>
              <div class="sub">
                <span class="t-quiet">
                  {loaderVersion ? `已选 ${loaderVersion}` : '未选则使用最新稳定版'}
                </span>
                <Button variant="link" onclick={() => (choosingLoaderVersion = false)}>收起</Button>
              </div>
            {/if}
          {/if}

          <!--
            附加层只在真叠得上去时才有这一栏。绝大多数版本上它根本不存在，
            而不是灰着——摆一栏点不动的东西，等于让人以为自己漏了什么。
          -->
          {#if addons.length > 0}
            <div class="sub" transition:unfold>
              <span class="t-quiet">附加</span>
              {#each addons as option (option.kind)}
                <label class="tick">
                  <input
                    type="checkbox"
                    checked={extras.includes(option.kind)}
                    onchange={() => toggleExtra(option.kind)}
                  />
                  {option.label}
                </label>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <Input
        label="名称"
        bind:value={name}
        maxlength={64}
        oninput={() => (named = true)}
        onkeydown={(event: KeyboardEvent) => event.key === 'Enter' && void submit()}
      />
    </div>
  {/if}

  {#if error}<div class="alert" transition:unfold>{error}</div>{/if}

  <footer>
    <!-- 次要出口。层级明确降级：这一屏的主操作只有「创建」一个。 -->
    <div class="ways">
      {#if !pack}
        <Button variant="link" onclick={() => void pickPack()}>从整合包创建</Button>
        <span class="t-quiet">·</span>
        <Button variant="link" onclick={() => nav.enter('instances', EXISTING)}>
          添加现有游戏
        </Button>
      {/if}
    </div>
    <Button onclick={() => nav.up()}>取消</Button>
    {#if pack}
      <Button variant="primary" loading={busy} onclick={() => void importPack()}>
        <Plus size={15} />导入整合包
      </Button>
    {:else}
      <Button variant="primary" loading={busy} onclick={() => void submit()}>
        <Plus size={15} />创建
      </Button>
    {/if}
  </footer>
</section>

<style>
  .create {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    border-radius: var(--r2);
    transition: background var(--t-fast) var(--ease);
  }

  /* 拖到窗口上时整块亮一下，说明这里接得住。 */
  .create.dropping {
    background: var(--accent-soft);
  }

  header {
    padding-bottom: var(--s5);
  }

  /* 单列。这一屏只问三件事，摆成两列只会把「创建」推到看不见的角落。 */
  .fields {
    display: grid;
    gap: var(--s5);
    align-content: start;
    max-width: 48ch;
    min-height: 0;
    flex: 1;
    padding-bottom: var(--s4);
  }

  .field {
    display: grid;
    gap: var(--s2);
  }

  .label {
    font-size: var(--t-small);
    color: var(--ink-2);
  }

  /* 版本那一行：平时只显示当前值，点开才是列表。 */
  .value {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    width: 100%;
    padding: var(--s3);
    border: 0;
    border-radius: var(--r1);
    background: var(--well);
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .value:hover {
    background: var(--well-2);
  }

  /*
   * 展开的那一层。三段各自成块：档位、搜索、列表，**只有列表滚动**——搜索框
   * 跟着列表一起滚走，等于翻到第三百条之后就没法改条件了。
   */
  .picker {
    display: grid;
    gap: var(--s3);
    padding: var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
  }

  .picker-head {
    display: grid;
    gap: var(--s2);
  }

  /* 滚动交给 kit 的 .scroll——细滚动条那一套全应用只有一份。 */
  .list {
    display: grid;
    align-content: start;
    gap: 2px;
    max-height: 42vh;
  }

  .list.short {
    max-height: 28vh;
  }

  .list hr {
    width: 100%;
    border: 0;
    border-top: 1px solid var(--hairline-2);
    margin: var(--s2) 0;
  }

  .nest {
    display: grid;
    gap: 2px;
  }

  /* 整行可点。判定区是一整条，不是里面那个小号码。 */
  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2) var(--s3);
    border: 0;
    border-radius: var(--r1);
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
    min-height: 34px;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .grow {
    flex: 1;
  }

  .row:hover {
    background: var(--tint-1);
  }

  .row:active {
    background: var(--tint-2);
  }

  .row.on {
    background: var(--tint-2);
  }

  .row.nested {
    padding-left: var(--s6);
  }

  .tick-mark {
    display: grid;
    place-items: center;
    color: var(--accent);
  }

  /* 展开的那一层往下翻——箭头跟着转，省掉一句「已展开」。 */
  .caret {
    display: grid;
    place-items: center;
    color: var(--ink-3);
    transition: transform var(--t-base) var(--ease);
  }

  .caret.up {
    transform: rotate(180deg);
  }

  .loaders {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }

  .chip {
    padding: var(--s2) var(--s3);
    border: 0;
    border-radius: var(--r1);
    background: var(--tint-1);
    color: var(--ink-2);
    cursor: pointer;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .chip:hover {
    background: var(--tint-2);
    color: var(--ink);
  }

  /* 按下去要陷一点。这一排是这一屏上唯一按了不换页的东西，没有反馈就像没点着。 */
  .chip:active {
    transform: scale(0.97);
  }

  .chip.on {
    background: var(--tint-3);
    color: var(--ink);
  }

  .sub {
    display: flex;
    align-items: center;
    gap: var(--s3);
    flex-wrap: wrap;
    font-size: var(--t-small);
  }

  .tick {
    display: flex;
    align-items: center;
    gap: var(--s2);
    cursor: pointer;
  }

  .hint {
    padding: var(--s3);
    color: var(--ink-3);
  }

  .pack {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    padding: var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
    margin-bottom: var(--s5);
  }

  .badge {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--r1);
    background: var(--tint-2);
  }

  .pack-text {
    flex: 1;
    display: grid;
    gap: var(--s1);
    min-width: 0;
  }

  .alert {
    padding: var(--s3);
    border-radius: var(--r1);
    background: var(--danger-bg);
    color: var(--danger-ink);
  }

  footer {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding-top: var(--s4);
    border-top: 1px solid var(--hairline-2);
  }

  .ways {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: 1;
  }
</style>
