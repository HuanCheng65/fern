<script lang="ts">
  /**
   * 实例概览顶上那一排：世界、模组、快照各一块，点进去就是那个 tab。
   *
   * 概览以前只有层表、实例 ID 和占用——都是「这个实例是什么」，没有「这个实例
   * 里有什么」。这三块回答后者，而数据本来就有命令。
   *
   * 三份各取各的，谁先回来谁先显示；取不到的那一块不出现，不写一个错的数字。
   * 模组要把每个 jar 打开读一遍清单，几百个的实例上要一会儿，所以回来之前写
   * 「正在读取」而不是 0。
   */
  import { appear } from '../lib/motion'
  import { invoke } from '@tauri-apps/api/core'
  import { dayLabel } from 'fern-kit/parts/snapshots'
  import { listSnapshots, type Snapshot } from '../lib/backup'

  interface Props {
    instanceId: string
    /** 原版实例没有模组 tab，这一块也就不出现。 */
    mods: boolean
    ontab: (id: string) => void
  }

  let { instanceId, mods, ontab }: Props = $props()

  interface SaveEntry {
    name: string
    modified?: number
  }

  let saves = $state<SaveEntry[] | null | undefined>(undefined)
  let modCount = $state<number | null | undefined>(undefined)
  let snapshots = $state<Snapshot[] | null | undefined>(undefined)

  $effect(() => {
    const id = instanceId
    saves = undefined
    modCount = undefined
    snapshots = undefined
    invoke<SaveEntry[]>('list_saves', { instanceId: id })
      .then((value) => id === instanceId && (saves = value))
      .catch(() => id === instanceId && (saves = null))
    if (mods) {
      invoke<unknown[]>('list_mods', { instanceId: id })
        .then((value) => id === instanceId && (modCount = value.length))
        .catch(() => id === instanceId && (modCount = null))
    }
    listSnapshots(id)
      .then((value) => id === instanceId && (snapshots = value))
      .catch(() => id === instanceId && (snapshots = null))
  })

  /** 存档列表已经按修改时间新的在前。 */
  const latestSave = $derived(saves?.[0])
  const latestSnapshot = $derived(
    snapshots?.reduce<Snapshot | undefined>(
      (best, item) => (best === undefined || item.takenAt > best.takenAt ? item : best),
      undefined,
    ),
  )
</script>

<div class="glance">
  {#if saves !== null}
    <button class="tile" onclick={() => ontab('saves')}>
      <span class="label">世界</span>
      {#if saves === undefined}
        <span class="value quiet">正在读取</span>
      {:else}
        <span class="value" in:appear>{saves.length} 个</span>
        <span class="note" in:appear>
          {#if latestSave}
            {latestSave.name}{latestSave.modified ? ` · ${dayLabel(latestSave.modified)}` : ''}
          {:else}
            还没有世界
          {/if}
        </span>
      {/if}
    </button>
  {/if}

  {#if mods && modCount !== null}
    <button class="tile" onclick={() => ontab('mods')}>
      <span class="label">模组</span>
      {#if modCount === undefined}
        <span class="value quiet">正在读取</span>
      {:else}
        <span class="value" in:appear>{modCount} 个</span>
      {/if}
    </button>
  {/if}

  {#if snapshots !== null}
    <button class="tile" onclick={() => ontab('snapshots')}>
      <span class="label">快照</span>
      {#if snapshots === undefined}
        <span class="value quiet">正在读取</span>
      {:else}
        <span class="value" in:appear>{snapshots.length} 张</span>
        <span class="note" in:appear>
          {latestSnapshot ? `最近 ${dayLabel(latestSnapshot.takenAt)}` : '还没有快照'}
        </span>
      {/if}
    </button>
  {/if}
</div>

<style>
  .glance {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--s2);
    margin: 0 0 var(--s5);
  }

  .tile {
    display: grid;
    align-content: start;
    gap: 2px;
    min-width: 0;
    padding: var(--s3) var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .tile:hover {
    background: var(--tint-2);
  }

  .label {
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  .value {
    color: var(--ink);
    font-size: var(--t-lead);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .value.quiet {
    color: var(--ink-3);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .note {
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-small);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
