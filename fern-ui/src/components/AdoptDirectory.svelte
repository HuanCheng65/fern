<script lang="ts">
  /**
   * 把一个已有的游戏目录接进来。
   *
   * **认两种来源，但只有一个入口。** 用户手上只有一个目录，不该由他来告诉
   * 我们它是官方那一系（`versions/` 一堆版本）还是 Prism / MultiMC（一个
   * 目录一个实例）——选完我们自己认，见 `instance::discover`。两者在交互上
   * 是同一件事：给一张清单，勾选要接进来的哪些，所以它们共用下面这一套。
   *
   * 大多数人用启动器的方式是把它和 `.minecraft` 放在一起，那个目录里已经有
   * 版本、有存档、有几百个 Mod。上一版的 Fern 只认自己私有目录里的实例，
   * 这样的用户第一步就得放弃已有的一切。
   *
   * **不移动任何文件。** 添加只是写入一份指向那个目录的实例描述，所以这一页
   * 是可以随便试的：添加错了把实例删掉即可，那个目录不受影响。这句话要写在
   * 界面上——「导入」在别的启动器里通常意味着复制几十 GB。
   *
   * **默认全选，一次提交。** 上一版每行一个「添加」按钮，一个装了十几个版本
   * 的目录就是十几次点击，每次还重扫一遍。而选中一个现成目录的人想做的是换
   * 启动器，不是挑版本——全要才是常态，只要其中几个是例外。所以默认勾上所有
   * 还没添加的，想去掉的自己去掉。
   *
   * 目录用系统选择器选，不让用户手打路径。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { open } from '@tauri-apps/plugin-dialog'
  import { FolderOpen } from 'lucide-svelte'
  import { instances, inTauri } from '../lib/instances.svelte'
  import { nav } from '../lib/nav.svelte'
  import { notices } from '../lib/notices.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface ExternalVersion {
    id: string
    gameVersion: string
    loader: string
    loaderVersion?: string
    isolation: 'shared' | 'perVersion'
    attached: boolean
    saves: number
    mods: number
  }

  /** 一个没能成为版本的目录，以及原因。 */
  interface SkippedVersion {
    name: string
    reason: string
  }

  interface ExternalScan {
    /** 真正读的那个目录。选中的是它的上一层时，两者不同。 */
    root: string
    versions: ExternalVersion[]
    skipped: SkippedVersion[]
  }

  /** Prism / MultiMC 的一个实例。 */
  interface PrismInstance {
    directory: string
    name: string
    gameVersion: string
    components: { kind: string; version: string }[]
    unsupported: string[]
    jarMods: string[]
    imported: boolean
  }

  type Discovery =
    | ({ kind: 'game-directory' } & ExternalScan)
    | { kind: 'prism-instances'; root: string; instances: PrismInstance[] }
    | { kind: 'unrecognised'; lookedAt: string }

  /**
   * 一条可以勾选的候选。
   *
   * 两种来源归一成同一个形状：一个版本和一个 Prism 实例，在这一屏上要回答的
   * 是同一个问题——「要不要把它接进来」。归一之后下面那份清单只写一遍。
   */
  interface Candidate {
    /** 提交时用来指名的东西：版本 id，或者实例目录。 */
    key: string
    title: string
    detail: string
    /** 已经添加过了，不再是选项。 */
    taken: boolean
    /** 需要提醒的一句（Prism 里我们装不了的那些层）。 */
    note?: string
  }

  interface Props {
    /** 一开始就指向某个目录（首次启动时发现的那一个）。 */
    initial?: string
    /**
     * 自带那颗提交按钮。
     *
     * 向导里要关掉：那一屏本来就有一颗主按钮，两颗并排的结果是用户按了更显眼
     * 的那一颗「继续」，然后带着一个什么都没导入的启动器走完向导——他以为勾上
     * 复选框就已经选好了。关掉之后由调用方用 `commit()` 提交。
     */
    standalone?: boolean
    /** 现在选了几个、忙不忙。自己提交的调用方要照着它写按钮上的字。 */
    onstatus?: (status: { chosen: number; busy: boolean }) => void
  }

  let { initial = '', standalone = true, onstatus }: Props = $props()

  let directory = $state('')
  /** 这个目录是哪一种来源。`none` 是还没选。 */
  let source = $state<'none' | 'game' | 'prism' | 'unknown'>('none')
  let candidates = $state<Candidate[] | null>(null)
  let skipped = $state<SkippedVersion[]>([])
  /** 勾选了哪些版本 id。用数组而不是 Set：几十条的量，可读比省事重要。 */
  let chosen = $state<string[]>([])
  let busy = $state<'' | 'scan' | 'add'>('')
  /** 添加到第几个了。一次几十份实例描述很快，但不该是一段没有交代的空白。 */
  let done = $state(0)
  /** 哪几个没添加成功，以及为什么。剩下的照常添加，不因为一个失败全盘停下。 */
  let failures = $state<{ id: string; reason: string }[]>([])
  let error = $state('')
  /**
   * 使用 Fern 的共享资源与依赖库。
   *
   * 默认使用：多个实例共享一份 assets 能省下数 GB。关闭后使用该目录自带的
   * 那一份，占用更多磁盘空间，但该目录仍可被原启动器单独使用。
   */
  let shared = $state(true)

  const ISOLATION_LABEL = {
    shared: '存档与其他版本共用',
    perVersion: '独立存档与模组',
  }

  const LOADER_LABEL: Record<string, string> = {
    vanilla: '原版',
    fabric: 'Fabric',
    quilt: 'Quilt',
    neo_forge: 'NeoForge',
    forge: 'Forge',
  }

  /** 还能添加的那些。已添加的留在列表里，但它们不是选项。 */
  const available = $derived((candidates ?? []).filter((item) => !item.taken))
  const attachedCount = $derived((candidates ?? []).length - available.length)
  const allChosen = $derived(available.length > 0 && chosen.length === available.length)
  /** 清单上那些东西的量词。两种来源数的不是同一种东西。 */
  const noun = $derived(source === 'prism' ? '实例' : '版本')

  const ISOLATION = (isolation: 'shared' | 'perVersion') => ISOLATION_LABEL[isolation]

  /** 官方那一系的一个版本，摊成一条候选。 */
  const fromVersion = (version: ExternalVersion): Candidate => ({
    key: version.id,
    title: version.id,
    detail: [
      version.gameVersion,
      version.loader !== 'vanilla'
        ? `${LOADER_LABEL[version.loader] ?? version.loader}${version.loaderVersion ? ` ${version.loaderVersion}` : ''}`
        : '',
      ISOLATION(version.isolation),
      version.saves > 0 ? `${version.saves} 个存档` : '',
      version.mods > 0 ? `${version.mods} 个模组` : '',
    ]
      .filter(Boolean)
      .join(' · '),
    taken: version.attached,
  })

  /** Prism 的一个实例，摊成一条候选。 */
  const fromPrism = (instance: PrismInstance): Candidate => ({
    key: instance.directory,
    title: instance.name,
    detail: [
      instance.gameVersion,
      ...instance.components.map(
        (one) => `${LOADER_LABEL[one.kind] ?? one.kind} ${one.version}`,
      ),
      instance.jarMods.length > 0 ? `${instance.jarMods.length} 个 jar mod` : '',
    ]
      .filter(Boolean)
      .join(' · '),
    taken: instance.imported,
    // 装不了的层要如实说：不说的话，导进来的是一个「看着成功」但少了半套东西
    // 的实例。
    note:
      instance.unsupported.length > 0
        ? `无法安装：${instance.unsupported.join('、')}`
        : undefined,
  })

  async function choose() {
    if (!inTauri()) return
    const picked = await open({ directory: true, multiple: false, title: '选择游戏目录' })
    if (typeof picked !== 'string') return
    // 换了目录，上一个目录里那些失败就不再说明什么了。
    failures = []
    await scan(picked)
  }

  async function scan(path: string) {
    busy = 'scan'
    error = ''
    candidates = null
    skipped = []
    // 不清 failures：添加完会重扫一次，清了就等于没报过。
    try {
      const found = await invoke<Discovery>('inspect_directory', { path })
      if (found.kind === 'game-directory') {
        source = 'game'
        candidates = found.versions.map(fromVersion)
        skipped = found.skipped
        // 选中的目录里正好有一个 `.minecraft` 时读的是它，后续的添加也用它。
        directory = found.root
      } else if (found.kind === 'prism-instances') {
        source = 'prism'
        candidates = found.instances.map(fromPrism)
        directory = found.root
      } else {
        source = 'unknown'
        candidates = []
        directory = found.lookedAt
      }
      chosen = (candidates ?? []).filter((item) => !item.taken).map((item) => item.key)
    } catch (cause) {
      error = String(cause)
      directory = path
      source = 'none'
    } finally {
      busy = ''
    }
  }

  function toggle(id: string) {
    chosen = chosen.includes(id) ? chosen.filter((item) => item !== id) : [...chosen, id]
  }

  function toggleAll() {
    chosen = allChosen ? [] : available.map((item) => item.key)
  }

  async function add() {
    const wanted = [...chosen]
    busy = 'add'
    error = ''
    failures = []
    done = 0
    let last: { id: string; name: string } | undefined
    for (const key of wanted) {
      try {
        last =
          source === 'prism'
            ? await invoke<{ id: string; name: string }>('import_prism_instance', { path: key })
            : await invoke<{ id: string; name: string }>('attach_game_version', {
                path: directory,
                versionId: key,
                sharedLibraries: shared,
              })
        done += 1
      } catch (cause) {
        failures.push({ id: key, reason: String(cause) })
      }
    }
    await instances.load()
    const added = done
    busy = ''
    // 重新扫描：刚添加的那些现在应显示为已添加，勾选也随之清空。
    await scan(directory)
    if (added === 0 || !last) return
    const target = last
    notices.say({
      title: added === 1 ? `已添加 ${target.name}` : `已添加 ${added} 个${noun}`,
      detail: '游戏文件保留在原位置。',
      action: {
        label: '打开',
        run: () => {
          if (added === 1) {
            instances.select(target.id)
            nav.enter('instances', target.id)
          } else {
            nav.go('instances')
          }
        },
      },
    })
  }

  // 首次启动时已经发现了一个目录，直接扫给用户看，不必再让他选一遍。
  $effect(() => {
    if (initial && !directory && inTauri()) void scan(initial)
  })

  $effect(() => {
    onstatus?.({ chosen: chosen.length, busy: busy !== '' })
  })

  /** 由调用方按下的提交。没勾中任何一个时什么都不做。 */
  export async function commit() {
    if (chosen.length > 0) await add()
  }
</script>

<div class="adopt">
  {#if standalone}
    <p class="lead">
      选择一个游戏目录，Fern 会认出它是 <code class="t-mono">.minecraft</code> 还是 Prism / MultiMC 的实例，列出其中的内容并默认全部添加。添加后可以照常补全文件、安装模组与启动；游戏文件保留在原位置，不会移动或复制，原来的启动器仍然可以使用它。
    </p>
  {/if}

  <div class="picker">
    <Button variant="ghost" disabled={busy !== ''} onclick={() => void choose()}>
      <FolderOpen size={14} strokeWidth={1.8} />{directory ? '更换目录' : '选择目录'}
    </Button>
    {#if directory}
      <span class="chosen t-mono selectable">{directory}</span>
    {/if}
  </div>

  {#if busy === 'scan'}
    <p class="t-quiet">正在读取目录…</p>
  {:else if source === 'unknown'}
    <!--
      认不出来时要说清我们在找什么。上一版只说「没有可用的版本」，那句话既
      不告诉他选错了目录，也不告诉他我们读不懂。
    -->
    <p class="t-quiet">
      这个目录里没有认得出来的游戏。Fern 找的是带 <code class="t-mono">versions</code>
      的 <code class="t-mono">.minecraft</code>，或者 Prism / MultiMC 的实例目录（里面有
      <code class="t-mono">mmc-pack.json</code>）。选它们的上一层也可以。
    </p>
  {:else if candidates}
    {#if candidates.length === 0}
      <p class="t-quiet">该目录中没有可用的{noun}。</p>
    {:else}
      <div class="summary">
        <span class="t-quiet">
          {candidates.length} 个{noun}{attachedCount > 0 ? `，其中 ${attachedCount} 个已添加` : ''}
        </span>
        {#if available.length > 1}
          <button class="toggle-all" disabled={busy !== ''} onclick={toggleAll}>
            {allChosen ? '全部取消' : '全部选中'}
          </button>
        {/if}
      </div>

      <ul class="versions">
        {#each candidates as item (item.key)}
          <li class="row" class:off={!item.taken && !chosen.includes(item.key)}>
            <label class="pick">
              <input
                type="checkbox"
                checked={item.taken || chosen.includes(item.key)}
                disabled={item.taken || busy !== ''}
                onchange={() => toggle(item.key)}
              />
              <span class="text">
                <strong>{item.title}</strong>
                <small class="t-mono">{item.detail}</small>
                {#if item.note}<small class="note">{item.note}</small>{/if}
              </span>
            </label>
            {#if item.taken}
              <span class="t-quiet done">已添加</span>
            {/if}
          </li>
        {/each}
      </ul>

      {#if source === 'game'}
      <label class="shared">
        <input type="checkbox" bind:checked={shared} disabled={busy !== ''} />
        <span>
          使用 Fern 的共享资源与依赖库
          <small>
            关闭后使用该目录自带的 assets 与 libraries，占用更多磁盘空间，该目录仍可由原启动器单独使用。
          </small>
        </span>
      </label>
      {/if}

      {#if standalone}
        <div class="commit">
          <Button
            variant="primary"
            disabled={chosen.length === 0 || busy !== ''}
            onclick={() => void add()}>
            {busy === 'add'
              ? `正在添加 ${done}/${chosen.length}`
              : `添加 ${chosen.length} 个${noun}`}
          </Button>
        </div>
      {/if}

      {#if failures.length > 0}
        <div class="alert">
          <p>{failures.length} 个{noun}未能添加：</p>
          <ul>
            {#each failures as item (item.id)}
              <li><span class="t-mono">{item.id}</span> — {item.reason}</li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}

    <!--
      跳过的目录要说出来，而不是从列表里悄悄消失。用户是对着一个自己装了
      十几个版本的目录看这一屏的，少了哪个他一眼就看得出来，缺的是原因；
      一个都没扫出来时，这里就是唯一能解释发生了什么的地方。
    -->
    {#if skipped.length > 0}
      <details class="skipped" open={candidates.length === 0}>
        <summary>{skipped.length} 个目录未被识别为版本</summary>
        <ul>
          {#each skipped as item (item.name)}
            <li>
              <span class="t-mono">{item.name}</span>
              <small>{item.reason}</small>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</div>

<style>
  .adopt {
    display: grid;
    gap: var(--s5);
  }

  .lead {
    margin: 0;
    max-width: 62ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.65;
  }

  .picker {
    display: flex;
    align-items: center;
    gap: var(--s3);
    min-width: 0;
  }

  .chosen {
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-micro);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s3);
    margin-bottom: calc(var(--s4) * -1);
    font-size: var(--t-small);
  }

  .toggle-all {
    color: var(--ink-3);
    font-size: var(--t-small);
    transition: color var(--t-fast) var(--ease);
  }

  .toggle-all:hover:not(:disabled) {
    color: var(--ink);
  }

  .shared {
    display: flex;
    align-items: flex-start;
    gap: var(--s2);
    font-size: var(--t-small);
  }

  .shared span {
    display: grid;
    gap: 2px;
    color: var(--ink-2);
  }

  .shared small {
    max-width: 52ch;
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.55;
  }

  .versions {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row:last-child {
    box-shadow: none;
  }

  /* 没勾上的仍然读得清，只是退到后面去。 */
  .row.off .text {
    opacity: 0.55;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: var(--s3);
    flex: 1;
    min-width: 0;
  }

  .text {
    display: grid;
    gap: 1px;
    min-width: 0;
    transition: opacity var(--t-fast) var(--ease);
  }

  /*
   * 装不了的那几层。用次要文字色，不用 --danger：那一档是「出事了」，而这
   * 只是一句必须说出口的损失——导进来仍然是成功的。
   */
  .text .note {
    color: var(--ink-2);
  }

  .text strong {
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .commit {
    display: flex;
    justify-content: flex-start;
  }

  .done {
    flex: none;
    font-size: var(--t-small);
  }

  .alert ul {
    display: grid;
    gap: 2px;
    margin: var(--s2) 0 0;
    padding: 0;
    list-style: none;
    font-size: var(--t-micro);
  }

  .alert p {
    margin: 0;
  }

  /* 收起来的次要信息：多数时候不看，扫不出东西时是唯一的线索。 */
  .skipped {
    font-size: var(--t-small);
  }

  .skipped summary {
    color: var(--ink-3);
    cursor: pointer;
  }

  .skipped ul {
    display: grid;
    gap: var(--s1);
    margin: var(--s2) 0 0;
    padding: 0;
    list-style: none;
  }

  .skipped li {
    display: flex;
    gap: var(--s2);
    min-width: 0;
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .skipped li span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .skipped li small {
    flex: none;
    color: var(--ink-4);
  }
</style>
