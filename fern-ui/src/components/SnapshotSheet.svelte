<script lang="ts">
  /**
   * 一张快照：它是什么，以及要不要恢复。
   *
   * 恢复是一次**决定**，不是列表里的一颗按钮——要先说清恢复哪一部分、写回原处
   * 还是另存一份，还要在按下去之前把后果讲出来。所以它是一个浮层：列表负责
   * 找，这里负责决定（见 docs/UI_DESIGN.md 十）。
   *
   * 那句后果是这一屏最重要的一行。「恢复」这个词本身不说明会发生什么——尤其
   * 是「快照之后新增的文件会被删掉」这件事，不写出来就是在用户不知情的时候
   * 删他的东西。写出来之后它才是一次知情的选择，而下面那句「恢复之前会自动
   * 拍一张」是它的退路。
   */
  import { untrack } from 'svelte'
  import { Check, X } from 'lucide-svelte'
  import SegmentedControl from 'fern-kit/ui/SegmentedControl.svelte'
  import Dialog from 'fern-kit/ui/Dialog.svelte'
  import { format, loaderName, ui } from '../lib/i18n'
  import { formatBytes } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { notices } from '../lib/notices.svelte'
  import {
    copyName,
    deleteSnapshot,
    labelSnapshot,
    moment,
    nameList,
    restoreSnapshot,
    sameAsNow,
    snapshotDiff,
    snapshotMods,
    whySkipped,
    why,
    type Restored,
    type RestoreMode,
    type RestoreScope,
    type Snapshot,
    type SnapshotDiff,
  } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'
  import Select from 'fern-kit/ui/Select.svelte'

  interface Props {
    instanceId: string
    snapshot: Snapshot
    onclose: () => void
    /** 删了、改了名、恢复完——列表都要重读。 */
    onchanged: () => void
  }

  let { instanceId, snapshot, onclose, onchanged }: Props = $props()

  type Part = 'all' | 'save' | 'config' | 'mods'

  /**
   * 这几个初值只在挂载时算一次。
   *
   * 浮层是按快照挂载的（列表里 `{#if open}`），换一张就是换一个组件实例，
   * 所以「读一次初值」正是要的行为——`untrack` 是把这件事说给编译器听。
   */
  const start = untrack(() => ({
    part: (snapshot.saves.length > 0 ? 'save' : 'all') as Part,
    save: snapshot.saves[0] ?? '',
    name: copyName(snapshot.saves[0] ?? '', snapshot.takenAt),
    label: snapshot.label ?? '',
  }))

  /**
   * 先看，后决定。
   *
   * 点开一张快照，先回答「这是哪一刻、里面有什么」；恢复是从这里再进一层
   * 的决定页。上一版点开直接是恢复表单——问都没让人看就要人做选择。
   */
  let view = $state<'info' | 'restore'>('info')
  /** 模组名单，展开那一节才拉。undefined 表示还没拉过。 */
  let modFiles = $state<string[] | undefined>(undefined)
  /** 与现在的差异。是补充信息：拉不到就不显示，不拦着别的事。 */
  let changes = $state<SnapshotDiff | undefined>(undefined)

  let part = $state<Part>(start.part)
  let save = $state(start.save)
  // 另存为新世界是默认：它对原世界零风险，「就想看看昨天的基地长什么样」
  // 也只有这条路能满足。想覆盖的人是在做一个更重的决定，多点一下应当。
  let copy = $state(true)
  let name = $state(start.name)
  let naming = $state(false)
  let draft = $state(start.label)
  /** 命名之后浮层不关，标题就得跟着变——本地记一份，不等列表重读。 */
  let label = $state(start.label)
  let confirming = $state(false)
  let busy = $state('')
  let error = $state('')
  let missing = $state<Restored['missing']>([])
  /** 恢复已经做完（哪怕有文件缺失）。做完之后这一屏只剩「关闭」。 */
  let restored = $state(false)

  const running = $derived(launch.phaseOf(instanceId) !== undefined)
  const title = $derived(label || why(snapshot).title)
  /** 「Fabric 0.16.5」。原版没有加载器，这一段就不出现。 */
  const loaderStamp = $derived(
    snapshot.loader !== 'vanilla'
      ? `${loaderName(snapshot.loader)}${snapshot.loaderVersion ? ` ${snapshot.loaderVersion}` : ''}`
      : '',
  )

  const parts = $derived([
    { value: 'all' as const, label: ui.snapshot.scopeAll },
    ...(snapshot.saves.length > 0 ? [{ value: 'save' as const, label: ui.snapshot.scopeSave }] : []),
    { value: 'config' as const, label: ui.snapshot.scopeConfig },
    ...(snapshot.mods > 0 ? [{ value: 'mods' as const, label: ui.snapshot.scopeMods }] : []),
  ])

  const scope = $derived<RestoreScope>(
    part === 'save' ? { kind: 'save', name: save } : { kind: part },
  )
  const mode = $derived<RestoreMode>(
    part === 'save' && copy ? { kind: 'copy', name: name.trim() } : { kind: 'replace' },
  )

  /** 世界名在句子里带引号，文件名不带。 */
  const quoted = (names: string[]) => nameList(names.map((world) => `「${world}」`))

  /**
   * 拍摄之后发生了什么，一句话。
   *
   * 这一行回答的是找快照的人真正的问题——「回到这一刻会退掉多少东西」。
   * 没有差异就直说没有：那意味着恢复是零风险的。
   */
  const since = $derived.by(() => {
    if (!changes) return ''
    if (sameAsNow(changes)) return ui.snapshot.diffSame
    const parts = [
      changes.modsAdded.length &&
        format(ui.snapshot.diffModsAdded, { count: String(changes.modsAdded.length) }),
      changes.modsRemoved.length &&
        format(ui.snapshot.diffModsRemoved, { count: String(changes.modsRemoved.length) }),
      changes.savesAdded.length &&
        format(ui.snapshot.diffSavesAdded, { names: quoted(changes.savesAdded) }),
      changes.savesRemoved.length &&
        format(ui.snapshot.diffSavesRemoved, { names: quoted(changes.savesRemoved) }),
      changes.savesChanged.length &&
        format(ui.snapshot.diffSavesChanged, { names: quoted(changes.savesChanged) }),
      changes.configChanged &&
        format(ui.snapshot.diffConfigChanged, { count: String(changes.configChanged) }),
    ].filter((part): part is string => Boolean(part))
    return format(ui.snapshot.diffLead, { parts: parts.join('，') })
  })

  /**
   * 按下去会发生什么。这一行不写，「恢复」就只是个词。
   *
   * 差异拉到了就说具体的（删哪几个、带回哪几个）；拉不到退回笼统的那句——
   * 笼统但不错，具体但要等，两头都不该挡住另一头。
   */
  const consequence = $derived.by(() => {
    if (part === 'save' && copy) {
      return format(ui.snapshot.consequenceCopy, { name: name.trim() || '…' })
    }
    if (part === 'save') {
      if (changes?.savesRemoved.includes(save)) {
        return format(ui.snapshot.consequenceSaveReturn, { save })
      }
      if (changes && !changes.savesChanged.includes(save)) {
        return format(ui.snapshot.consequenceSaveSame, { save })
      }
      return format(ui.snapshot.consequenceSave, { save })
    }
    if (part === 'config') return ui.snapshot.consequenceConfig
    if (part === 'mods') {
      if (!changes) return format(ui.snapshot.consequenceMods, { count: String(snapshot.mods) })
      return [
        format(ui.snapshot.consequenceModsBase, { count: String(snapshot.mods) }),
        changes.modsAdded.length &&
          format(ui.snapshot.consequenceModsDrop, {
            count: String(changes.modsAdded.length),
            names: nameList(changes.modsAdded),
          }),
        changes.modsRemoved.length &&
          format(ui.snapshot.consequenceModsReturn, {
            count: String(changes.modsRemoved.length),
            names: nameList(changes.modsRemoved),
          }),
      ]
        .filter(Boolean)
        .join('')
    }
    return [
      ui.snapshot.consequenceAll,
      changes?.savesAdded.length
        ? format(ui.snapshot.consequenceAllDrop, { names: quoted(changes.savesAdded) })
        : '',
    ].join('')
  })

  const ready = $derived(
    !running &&
      busy === '' &&
      (part !== 'save' || (save !== '' && (!copy || name.trim() !== ''))),
  )

  async function restore() {
    busy = 'restore'
    error = ''
    missing = []
    try {
      const result = await restoreSnapshot(instanceId, snapshot.id, scope, mode)
      onchanged()
      if (result.missing.length > 0) {
        // 这几个文件没写回去，需要人来看。留在这一屏，不做成说完就走的通知。
        // 但恢复本身已经完成了——下面的按钮必须承认这一点（只剩「关闭」），
        // 不能还站着一颗「恢复」。
        missing = result.missing
        restored = true
        return
      }
      notices.say({
        title: ui.snapshot.restored,
        detail:
          format(ui.snapshot.restoredWritten, { count: String(result.written) }) +
          (result.removed > 0
            ? format(ui.snapshot.restoredRemoved, { count: String(result.removed) })
            : ''),
      })
      onclose()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
    }
  }

  async function rename() {
    busy = 'label'
    try {
      await labelSnapshot(instanceId, snapshot.id, draft.trim() || undefined)
      label = draft.trim()
      naming = false
      error = ''
      // 名字改完人还站在这一屏——多半是顺手，接下来才是恢复。不关。
      onchanged()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
    }
  }

  async function remove() {
    busy = 'delete'
    try {
      await deleteSnapshot(instanceId, snapshot.id)
      onchanged()
      onclose()
    } catch (cause) {
      error = String(cause)
      busy = ''
    }
  }

  async function loadMods() {
    if (modFiles !== undefined) return
    try {
      modFiles = await snapshotMods(instanceId, snapshot.id)
    } catch (cause) {
      error = String(cause)
    }
  }

  /** 从清单里的一个世界直接进恢复：范围已经选好，只剩写回方式。 */
  function restoreWorld(world: string) {
    part = 'save'
    save = world
    copy = true
    name = copyName(world, snapshot.takenAt)
    view = 'restore'
  }

  // 差异只要一次盘面扫描，打开就拉；失败静默——它是补充，不是门槛。
  // untrack 的理由同上面的 start：浮层按快照挂载，初值只读一次。
  void untrack(() => snapshotDiff(instanceId, snapshot.id))
    .then((diff) => (changes = diff))
    .catch(() => {})
</script>

<Dialog label={ui.snapshot.dialog} width="520px" {onclose}>
  <header>
    {#if naming}
      <form
        class="rename"
        onsubmit={(event) => {
          event.preventDefault()
          void rename()
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <Input
          aria-label={ui.snapshot.nameAria}
          bind:value={draft}
          placeholder={ui.snapshot.renamePlaceholder}
          maxlength={60}
          autofocus
        />
        <Button variant="icon" type="submit" aria-label={ui.snapshot.renameSave}>
          <Check size={16} strokeWidth={2} />
        </Button>
        <Button
          variant="icon"
          type="button"
          aria-label={ui.snapshot.cancel}
          onclick={() => (naming = false)}
        >
          <X size={16} strokeWidth={2} />
        </Button>
      </form>
      <!-- 命名不只是个备注：这句规则以前只写在代码注释里，用户无从知道。 -->
      <p class="t-quiet hint">{ui.snapshot.renameHint}</p>
    {:else}
      <div class="titles">
        <h2>{title}</h2>
        <Button variant="link" onclick={() => (naming = true)}>{ui.snapshot.rename}</Button>
      </div>
    {/if}

    <p class="t-quiet meta">
      {[
        moment(snapshot.takenAt),
        `Minecraft ${snapshot.minecraft}`,
        loaderStamp,
        format(ui.snapshots.files, { count: String(snapshot.files) }),
        formatBytes(snapshot.bytes),
      ]
        .filter(Boolean)
        .join(' · ')}
    </p>
    <!-- 它为什么在这里。这套说明此前写好了却从没显示过。 -->
    <p class="t-quiet reason">{why(snapshot).detail}</p>
  </header>

  <div class="body">
    {#if snapshot.inconsistent}
      <p class="warn">{ui.snapshot.inconsistent}</p>
    {/if}

    {#if view === 'info'}
      <!-- 拍摄之后世界变了什么。「回到这一刻会退掉多少东西」的答案。 -->
      {#if since}
        <p class="t-quiet since">{since}</p>
      {/if}

      <!-- 里面有什么。恢复的决定要看着这份清单做，所以它排在决定之前。 -->
      {#if snapshot.saves.length > 0}
        <section class="content">
          <h3 class="t-quiet">
            {format(ui.snapshot.contentWorlds, { count: String(snapshot.saves.length) })}
          </h3>
          <ul>
            {#each snapshot.saves as world (world)}
              <li>
                <!-- 点一个世界直接进恢复，范围已选好——找到想回去的那个世界
                     时，人已经做完了这一屏要做的决定。 -->
                <button class="world" onclick={() => restoreWorld(world)}>
                  <span class="world-name">{world}</span>
                  <span class="t-quiet go">{ui.snapshot.enterRestore}</span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if snapshot.mods > 0}
        <details class="skipped" ontoggle={(event) => {
          if ((event.currentTarget as HTMLDetailsElement).open) void loadMods()
        }}>
          <summary>{format(ui.snapshot.contentMods, { count: String(snapshot.mods) })}</summary>
          {#if modFiles === undefined}
            <p class="t-quiet loading-note">{ui.snapshot.contentModsLoading}</p>
          {:else}
            <ul>
              {#each modFiles as file (file)}
                <li><span class="t-mono">{file}</span></li>
              {/each}
            </ul>
          {/if}
        </details>
      {/if}

      {#if snapshot.skipped.length > 0}
        <details class="skipped">
          <summary>
            {format(ui.snapshot.skipped, { count: String(snapshot.skipped.length) })}
          </summary>
          <ul>
            {#each snapshot.skipped as item (item.path)}
              <li>
                <span class="t-mono">{item.path}</span>
                <span class="t-quiet">{whySkipped(item).title}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    {:else}
      <SegmentedControl
        label={ui.snapshot.scope}
        options={parts}
        value={part}
        onchange={(value) => (part = value)}
      />

      {#if part === 'save'}
        {#if snapshot.saves.length > 1}
          <Select
            label={ui.snapshot.world}
            options={snapshot.saves.map((world) => ({ value: world, label: world }))}
            bind:value={save}
            onchange={() => (name = copyName(save, snapshot.takenAt))}
          />
        {/if}

        <SegmentedControl
          label={ui.snapshot.mode}
          options={[
            { value: 'copy' as const, label: ui.snapshot.modeCopy },
            { value: 'replace' as const, label: ui.snapshot.modeReplace },
          ]}
          value={copy ? 'copy' : 'replace'}
          onchange={(value) => (copy = value === 'copy')}
        />

        {#if copy}
          <Input label={ui.snapshot.copyName} bind:value={name} maxlength={60} />
        {/if}
      {/if}

      <p class="consequence">
        {consequence}
        {#if !(part === 'save' && copy)}
          <span class="t-quiet">{ui.snapshot.safety}</span>
        {/if}
      </p>

      {#if missing.length > 0}
        <div class="alert">
          {ui.snapshot.missingLead}
          {#each missing as item (item.path)}
            <div class="missing">
              <span>{item.path}</span>
              {#if item.sha1}
                <!-- 对象仓库坏掉后的唯一退路：拿这个哈希去模组站反查。 -->
                <span class="t-mono selectable">{item.sha1}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      {#if running && !restored}
        <p class="warn">{ui.snapshot.running}</p>
      {/if}
    {/if}

    {#if error}<div class="alert">{error}</div>{/if}
  </div>

  <footer>
    {#if restored}
      <span class="t-quiet">{ui.snapshot.missingDone}</span>
      <span class="spacer"></span>
      <Button variant="primary" onclick={onclose}>{ui.snapshot.close}</Button>
    {:else if view === 'info'}
      {#if confirming}
        <span class="confirm">
          <span class="t-quiet">{ui.snapshot.deleteWarn}</span>
          <Button variant="ghost" onclick={() => (confirming = false)}>
            {ui.snapshot.cancel}
          </Button>
          <Button
            tone="danger"
            loading={busy === 'delete'}
            disabled={busy !== ''}
            onclick={() => void remove()}
          >
            {ui.snapshot.deleteConfirm}
          </Button>
        </span>
      {:else}
        <Button variant="link" tone="danger" onclick={() => (confirming = true)}>
          {ui.snapshot.delete}
        </Button>
        <span class="spacer"></span>
        <Button variant="ghost" onclick={onclose}>{ui.snapshot.cancel}</Button>
        <Button variant="primary" onclick={() => (view = 'restore')}>
          {ui.snapshot.enterRestore}
        </Button>
      {/if}
    {:else}
      <Button variant="ghost" onclick={() => (view = 'info')}>{ui.snapshot.back}</Button>
      <span class="spacer"></span>
      <Button
        variant="primary"
        loading={busy === 'restore'}
        disabled={!ready}
        onclick={() => void restore()}
      >
        {ui.snapshot.restore}
      </Button>
    {/if}
  </footer>
</Dialog>

<style>
  header {
    padding: var(--s5) var(--s5) var(--s4);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .titles {
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    min-width: 0;
  }

  h2 {
    margin: 0;
    min-width: 0;
    color: var(--ink);
    font-size: var(--t-h3);
    font-weight: 560;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  .hint {
    margin: var(--s2) 0 0;
    font-size: var(--t-micro);
  }

  .meta {
    margin: var(--s2) 0 0;
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .reason {
    margin: var(--s1) 0 0;
    font-size: var(--t-micro);
    line-height: 1.6;
  }

  .body {
    display: grid;
    gap: var(--s4);
    padding: var(--s5);
    overflow-y: auto;
  }

  /* 这一段是这一屏最重要的一行：说清按下去会发生什么。 */
  .consequence {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    line-height: 1.7;
  }

  .consequence .t-quiet {
    display: block;
  }

  .warn {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
    line-height: 1.6;
  }

  /* 内容清单：不加卡片不加边框，靠留白和层级说清（Frond：仅在需要边界时
     创造边界）。 */
  .content h3 {
    margin: 0 0 var(--s2);
    font-size: var(--t-micro);
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .content ul {
    display: grid;
    gap: var(--s1);
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  .since {
    margin: 0;
    font-size: var(--t-small);
    line-height: 1.7;
  }

  /* 世界行是一条通往聚焦恢复的路。安静地躺着，指过去才说自己能点。 */
  .world {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--s3);
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .world-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .world .go {
    flex: none;
    font-size: var(--t-micro);
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .world:hover .go,
  .world:focus-visible .go {
    opacity: 1;
  }

  .loading-note {
    margin: var(--s2) 0 0;
    font-size: var(--t-micro);
  }

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
    justify-content: space-between;
    gap: var(--s3);
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .missing {
    display: flex;
    justify-content: space-between;
    gap: var(--s3);
    min-width: 0;
  }

  .missing .t-mono {
    flex: none;
    font-size: var(--t-micro);
  }

  footer {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s4) var(--s5) var(--s5);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }

  .spacer {
    flex: 1;
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    font-size: var(--t-small);
  }

  .confirm .t-quiet {
    flex: 1;
  }
</style>
