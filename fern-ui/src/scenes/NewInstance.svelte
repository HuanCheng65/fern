<script lang="ts">
  /**
   * 新建实例——实例场景向内的那一级。
   *
   * 以前是个 460px 的对话框。承载不住：版本清单有八百多条，加载器还要再选一个
   * 版本，全塞进一个浮层就只能靠滚动，而浮层的边界又在提示「这是一件小事，
   * 随手填完就走」。建实例不是小事，它决定了这个实例之后的一切。
   *
   * 名字默认填好（版本号，有加载器就跟在后面）。绝大多数人对叫什么没有意见，
   * 逼他先想一个名字，只是在真正的问题前面加了一道无谓的门槛。一旦动过输入框
   * 就不再自动改——那时候名字是用户的了。
   *
   * 版本不用原生 `<select>`：八百多个版本用下拉框找是灾难，而且原生控件在深色
   * 界面里长得和别处完全不是一套。做成两档 + 搜索 + 一列可滚动的版本，行为和
   * 命令面板一致——同一个交互模型在启动器里只学一次。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { onMount } from 'svelte'
  import { Check, Package, Plus } from 'lucide-svelte'
  import Choice from '../components/Choice.svelte'
  import Loading from '../components/Loading.svelte'
  import { instances, inTauri, type LoaderOption } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { suggestName } from '../lib/naming'
  import { nav } from '../lib/nav.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface LoaderVersion {
    version: string
    stable: boolean
  }

  type Kind = 'release' | 'snapshot'

  let name = $state('')
  /** 用户自己动过名字之后就不再替他改。 */
  let named = $state(false)
  let kind = $state<Kind>('release')
  let query = $state('')
  let picked = $state('')
  let loader = $state('vanilla')
  let loaders = $state<LoaderOption[]>([{ kind: 'vanilla', label: '原版' }])
  let loaderVersion = $state('')
  let loaderVersions = $state<LoaderVersion[]>([])
  let choosingLoaderVersion = $state(false)
  let busy = $state(false)
  let error = $state('')

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

  const shown = $derived(
    instances.versions
      .filter((item) => (kind === 'release' ? item.kind === 'release' : item.kind !== 'release'))
      .filter((item) => item.id.toLowerCase().includes(query.trim().toLowerCase()))
      .slice(0, 400),
  )

  const loaderLabel = $derived(
    loaders.find((item) => item.kind === loader)?.label ?? '原版',
  )
  const stableLoaderVersions = $derived(loaderVersions.filter((item) => item.stable))

  // 打开就默认选中最新正式版：绝大多数人要的就是它。
  $effect(() => {
    if (!picked && shown.length > 0) picked = shown[0]!.id
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

  const day = (iso: string) => iso.slice(0, 10)

  async function submit() {
    const trimmed = name.trim()
    if (!trimmed) return (error = '请输入实例名称')
    if (!picked) return (error = '请选择 Minecraft 版本')
    busy = true
    error = ''
    try {
      const created = await instances.create(trimmed, picked, loader, loaderVersion)
      // 建完直接落到它的详情页：刚建的东西该能立刻看见，而不是回到网格里自己找。
      instances.select(created.id)
      nav.open(created.id)
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
      void launch.repair(created.id, `准备 ${created.name}`)
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
      nav.open(created.id)
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
  if (inTauri()) {
    void instances.loadLoaders().then((list) => {
      if (list.length > 0) loaders = list
    })
  }
</script>

<section class="create" class:dropping>
  <header>
    <h1 class="t-h1">新建实例</h1>
    <p class="t-quiet">
      {pack ? '整合包自带版本与加载器。' : '选择版本与加载器，或把 .mrpack 拖进窗口。'}
    </p>
  </header>

  {#if pack}
    <div class="pack">
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

    <div class="field pack-name">
      <label for="pack-name">名称</label>
      <input id="pack-name" class="input" bind:value={name} maxlength="64" oninput={() => (named = true)} />
    </div>
  {:else}
  <div class="columns">
    <div class="side">
      <div class="field">
        <label for="new-instance-name">名称</label>
        <input
          id="new-instance-name"
          class="input"
          bind:value={name}
          maxlength="64"
          oninput={() => (named = true)}
          onkeydown={(event) => event.key === 'Enter' && void submit()}
        />
      </div>

      {#if loaders.length > 1}
        <div class="field">
          <span class="label" id="loader-label">加载器</span>
          <div class="loaders" role="group" aria-labelledby="loader-label">
            {#each loaders as option (option.kind)}
              <button
                class="loader"
                class:on={loader === option.kind}
                onclick={() => (loader = option.kind)}
              >
                {option.label}
              </button>
            {/each}
          </div>

          {#if loader !== 'vanilla'}
            {#if !choosingLoaderVersion}
              <div class="row">
                <span class="t-quiet">将安装最新稳定版</span>
                <Button variant="link" onclick={() => void loadLoaderVersions()}>
                  指定版本
                </Button>
              </div>
            {:else if stableLoaderVersions.length === 0}
              <Loading note="读取 {loaderLabel} 的版本" size={18} />
            {:else}
              <div class="picks scroll">
                {#each stableLoaderVersions.slice(0, 60) as item (item.version)}
                  <button
                    class="pick"
                    class:on={loaderVersion === item.version}
                    onclick={() => (loaderVersion = item.version)}
                  >
                    <span class="t-mono">{item.version}</span>
                    {#if loaderVersion === item.version}<Check size={13} strokeWidth={2.4} />{/if}
                  </button>
                {/each}
              </div>
              <div class="row">
                <span class="t-quiet">
                  {loaderVersion ? `已选 ${loaderVersion}` : '未选则使用最新稳定版'}
                </span>
                <Button variant="link" onclick={() => (choosingLoaderVersion = false)}>
                  收起
                </Button>
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>

    <div class="versions-col">
      <div class="field">
        <div class="version-head">
          <label for="version-filter">Minecraft 版本</label>
          <Choice
            label="版本类型"
            value={kind}
            onchange={(next) => {
              kind = next as Kind
              picked = ''
            }}
            options={[
              { value: 'release', label: '正式版' },
              { value: 'snapshot', label: '快照' },
            ]}
          />
        </div>
        <input
          id="version-filter"
          class="input"
          bind:value={query}
          spellcheck="false"
          placeholder="筛选版本号"
        />
        <div class="versions scroll">
          {#if instances.versionsLoading}
            <Loading note="读取版本清单" />
          {:else if shown.length === 0}
            <p class="hint">没有匹配的版本</p>
          {:else}
            {#each shown as version (version.id)}
              <button
                class="version"
                class:on={picked === version.id}
                onclick={() => (picked = version.id)}
              >
                <span class="t-mono id">{version.id}</span>
                <span class="t-mono date">{day(version.releaseTime)}</span>
                {#if picked === version.id}<Check size={14} strokeWidth={2.4} />{/if}
              </button>
            {/each}
          {/if}
        </div>
      </div>
    </div>
  </div>
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}

  <footer>
    <Button onclick={() => nav.back()}>取消</Button>
    {#if pack}
      <Button variant="primary" disabled={busy} onclick={() => void importPack()}>
        <Plus size={15} />{busy ? '导入中' : '导入整合包'}
      </Button>
    {:else}
      <Button variant="primary" disabled={busy} onclick={() => void submit()}>
        <Plus size={15} />{busy ? '创建中' : '创建实例'}
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

  .pack {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    padding: var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
  }

  .badge {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex: none;
    border-radius: var(--r1);
    background: var(--tint-2);
    color: var(--accent);
  }

  .pack-text {
    flex: 1;
    min-width: 0;
  }

  .pack-text strong {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .pack-text small {
    margin-left: var(--s2);
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .pack-text p {
    margin: var(--s2) 0 0;
    font-size: var(--t-small);
    overflow-wrap: anywhere;
  }

  .pack-name {
    max-width: 360px;
    margin-top: var(--s5);
  }

  header {
    padding-bottom: var(--s5);
  }

  header h1 {
    margin: 0;
  }

  header p {
    margin: var(--s2) 0 0;
  }

  /* 左边是决定，右边是清单。窄了就叠起来，清单永远在下面。 */
  .columns {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(240px, 320px) minmax(0, 1fr);
    gap: clamp(var(--s5), 5vw, var(--s8));
  }

  .side {
    display: grid;
    align-content: start;
    gap: var(--s5);
    min-width: 0;
  }

  .versions-col {
    display: flex;
    min-height: 0;
  }

  .field {
    display: grid;
    gap: var(--s2);
    align-content: start;
    min-height: 0;
    width: 100%;
  }

  .field label,
  .label {
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .version-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .versions {
    min-height: 0;
    margin-top: var(--s1);
    padding-right: var(--s2);
  }

  .version {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2) var(--s2) var(--s2) 0;
    border-radius: var(--r1);
    color: var(--ink-3);
    text-align: left;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .version:hover {
    background: var(--tint-1);
    color: var(--ink-2);
  }

  .version.on {
    color: var(--ink);
  }

  .version .id {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: var(--t-body);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .version .date {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .version :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .loaders {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }

  .loader {
    padding: 5px var(--s3);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-small);
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .loader:hover {
    color: var(--ink-2);
    background: var(--tint-2);
  }

  .loader.on {
    background: var(--accent);
    color: var(--on-accent);
  }

  .picks {
    max-height: 168px;
    margin-top: var(--s1);
    padding-right: var(--s2);
  }

  .pick {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
    width: 100%;
    padding: 5px 0;
    color: var(--ink-3);
    font-size: var(--t-small);
    text-align: left;
  }

  .pick:hover {
    color: var(--ink-2);
  }

  .pick.on {
    color: var(--ink);
  }

  .pick :global(svg) {
    color: var(--accent);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .hint {
    margin: var(--s3) 0;
    color: var(--ink-4);
    font-size: var(--t-small);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
    padding-top: var(--s5);
  }

  .alert {
    margin-top: var(--s4);
  }

  @media (max-width: 860px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--s5);
    }
  }
</style>
