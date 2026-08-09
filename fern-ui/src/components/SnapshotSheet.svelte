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
  import { Check, Pencil, X } from 'lucide-svelte'
  import Choice from './Choice.svelte'
  import Overlay from 'fern-kit/ui/Overlay.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { notices } from '../lib/notices.svelte'
  import {
    copyName,
    deleteSnapshot,
    labelSnapshot,
    moment,
    restoreSnapshot,
    whySkipped,
    why,
    type Restored,
    type RestoreMode,
    type RestoreScope,
    type Snapshot,
  } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'

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

  let part = $state<Part>(start.part)
  let save = $state(start.save)
  let copy = $state(false)
  let name = $state(start.name)
  let naming = $state(false)
  let draft = $state(start.label)
  let confirming = $state(false)
  let busy = $state('')
  let error = $state('')
  let missing = $state<Restored['missing']>([])

  const running = $derived(launch.phaseOf(instanceId) !== undefined)
  const title = $derived(snapshot.label ?? why(snapshot).title)

  const parts = $derived([
    { value: 'all' as const, label: '整个实例' },
    ...(snapshot.saves.length > 0 ? [{ value: 'save' as const, label: '一个世界' }] : []),
    { value: 'config' as const, label: '配置' },
    ...(snapshot.mods > 0 ? [{ value: 'mods' as const, label: '模组' }] : []),
  ])

  const scope = $derived<RestoreScope>(
    part === 'save' ? { kind: 'save', name: save } : { kind: part },
  )
  const mode = $derived<RestoreMode>(
    part === 'save' && copy ? { kind: 'copy', name: name.trim() } : { kind: 'replace' },
  )

  /** 按下去会发生什么。这一行不写，「恢复」就只是个词。 */
  const consequence = $derived.by(() => {
    if (part === 'save' && copy) {
      return `会新建一个名为「${name.trim() || '…'}」的世界，原来的世界不受影响。`
    }
    if (part === 'save') {
      return `会把「${save}」还原到这一刻，之后新生成的区块和数据会被删除。`
    }
    if (part === 'config') {
      return '会还原 config 目录和游戏目录下的设置文件，存档与模组不受影响。'
    }
    if (part === 'mods') {
      return `会把模组还原成这一刻的 ${snapshot.mods} 个，之后新装的会被删除。`
    }
    return '会还原存档、配置和模组，之后新增的文件会被删除。'
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
        missing = result.missing
        return
      }
      notices.say({
        title: '已恢复',
        detail:
          `写回 ${result.written} 个文件` +
          (result.removed > 0 ? `，删除 ${result.removed} 个` : ''),
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
      naming = false
      error = ''
      onchanged()
      onclose()
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
</script>

<Overlay label="快照" width="520px" {onclose}>
  <header>
    {#if naming}
      <!-- 起了名字的快照永久保留，所以命名不只是个备注。 -->
      <form
        class="rename"
        onsubmit={(event) => {
          event.preventDefault()
          void rename()
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="input"
          bind:value={draft}
          placeholder="例如：装 Create 之前"
          maxlength="60"
          autofocus
        />
        <Button variant="icon" type="submit" aria-label="保存名称">
          <Check size={16} strokeWidth={2} />
        </Button>
        <Button variant="icon" type="button" aria-label="取消" onclick={() => (naming = false)}>
          <X size={16} strokeWidth={2} />
        </Button>
      </form>
    {:else}
      <div class="titles">
        <h2>{title}</h2>
        <div class="rename-btn">
          <Button variant="icon" aria-label="命名" onclick={() => (naming = true)}>
            <Pencil size={14} strokeWidth={1.9} />
          </Button>
        </div>
      </div>
    {/if}

    <p class="t-quiet meta">
      {moment(snapshot.takenAt)} · Minecraft {snapshot.minecraft} · {snapshot.files} 个文件 ·
      {formatBytes(snapshot.bytes)}
    </p>
  </header>

  <div class="body">
    {#if snapshot.inconsistent}
      <p class="warn">拍摄时文件仍在变动，这张快照的内容可能不一致。</p>
    {/if}

    <div class="field">
      <label for="snapshot-part">恢复哪一部分</label>
      <div id="snapshot-part">
        <Choice options={parts} value={part} onchange={(value) => (part = value)} label="恢复范围" />
      </div>
    </div>

    {#if part === 'save'}
      {#if snapshot.saves.length > 1}
        <div class="field">
          <label for="snapshot-save">世界</label>
          <select
            id="snapshot-save"
            class="select"
            bind:value={save}
            onchange={() => (name = copyName(save, snapshot.takenAt))}
          >
            {#each snapshot.saves as world (world)}
              <option value={world}>{world}</option>
            {/each}
          </select>
        </div>
      {/if}

      <div class="field">
        <label for="snapshot-mode">写回方式</label>
        <div id="snapshot-mode">
          <Choice
            options={[
              { value: 'replace' as const, label: '覆盖原世界' },
              { value: 'copy' as const, label: '另存为新世界' },
            ]}
            value={copy ? 'copy' : 'replace'}
            onchange={(value) => (copy = value === 'copy')}
            label="写回方式"
          />
        </div>
      </div>

      {#if copy}
        <div class="field">
          <label for="snapshot-name">新世界的名称</label>
          <input id="snapshot-name" class="input" bind:value={name} maxlength="60" />
        </div>
      {/if}
    {/if}

    <p class="consequence">
      {consequence}
      {#if !copy}
        <span class="t-quiet">恢复前会自动拍一张，可以用它撤销这次恢复。</span>
      {/if}
    </p>

    {#if snapshot.skipped.length > 0}
      <details class="skipped">
        <summary>{snapshot.skipped.length} 项未纳入快照</summary>
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

    {#if missing.length > 0}
      <div class="alert">
        以下文件的内容已不在备份中，没有写回，原文件保持不变：
        {#each missing as item (item.path)}
          <div>{item.path}</div>
        {/each}
      </div>
    {/if}

    {#if running}
      <p class="warn">游戏正在运行，写回的文件会被覆盖。请先退出游戏。</p>
    {/if}

    {#if error}<div class="alert">{error}</div>{/if}
  </div>

  <footer>
    {#if confirming}
      <span class="confirm">
        <span class="t-quiet">删除后这张快照无法找回。</span>
        <Button variant="ghost" onclick={() => (confirming = false)}>取消</Button>
        <Button tone="danger" disabled={busy !== ''} onclick={() => void remove()}>
          确认删除
        </Button>
      </span>
    {:else}
      <Button variant="link" tone="danger" onclick={() => (confirming = true)}>删除</Button>
      <span class="spacer"></span>
      <Button variant="ghost" onclick={onclose}>取消</Button>
      <Button variant="primary" disabled={!ready} onclick={() => void restore()}>
        {busy === 'restore' ? '正在恢复' : '恢复'}
      </Button>
    {/if}
  </footer>
</Overlay>

<style>
  header {
    padding: var(--s5) var(--s5) var(--s4);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .titles {
    display: flex;
    align-items: center;
    gap: var(--s2);
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

  /* 命名不是常用动作，所以它平时几乎看不见，指过去才出现。 */
  .titles .rename-btn {
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease);
  }

  .titles:hover .rename-btn,
  .titles .rename-btn:focus-within {
    opacity: 1;
  }

  .rename {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  .meta {
    margin: var(--s2) 0 0;
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
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
