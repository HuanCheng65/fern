<script lang="ts">
  /**
   * 实例的快照。
   *
   * 排版按「时间轴」组织而不是一张平铺的表：快照唯一的排序依据是时间，而人
   * 找的是「装那个模组之前那一张」——按天分组之后，找的动作从读十二行时间戳
   * 变成了先落到某一天再挑一行。
   *
   * 一行只回答三件事，从左到右：**什么时候、为什么在这里、里面有什么**。别的
   * 都收进浮层——恢复要做的选择（恢复哪一部分、覆盖还是另存）是一次决定，不是
   * 一行里塞得下的三颗按钮。二十行各三颗按钮的列表没法扫读。
   *
   * 所以整行是一个按钮：点进去做决定，列表本身只用来找。
   */
  import { Camera } from 'lucide-svelte'
  import Loading from './Loading.svelte'
  import SnapshotSheet from './SnapshotSheet.svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { format, ui } from '../lib/i18n'
  import { formatBytes } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { notices } from '../lib/notices.svelte'
  import { backupUsage, listSnapshots, pinned, takeSnapshot, why, type Snapshot } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'
  import SectionHead from 'fern-kit/ui/SectionHead.svelte'
  import SnapshotList from 'fern-kit/parts/SnapshotList.svelte'
  import type { SnapshotRow } from 'fern-kit/parts/snapshots'

  interface Props {
    instanceId: string
  }

  let { instanceId }: Props = $props()

  let snapshots = $state<Snapshot[]>([])
  let reclaimable = $state(0)
  let loading = $state(true)
  let taking = $state(false)
  let error = $state('')
  let open = $state<Snapshot | undefined>(undefined)

  /** 游戏跑着的时候拍到的是半个存档，所以后端会直接拒绝。按钮先说清楚。 */
  const running = $derived(launch.phaseOf(instanceId) !== undefined)

  /**
   * 折成名单要显示的样子。分组和排版交给 SnapshotList，这里只负责说清每一行。
   *
   * 行尾说的是**里面有什么**（几个世界、几个模组），不是它多大：去重之后
   * 各快照大小相加是个没有意义的数（设计文档 §7），一列相仿的兆字节只会
   * 引人去加。大小在浮层里，和文件数放在一起说。
   */
  const contents = (item: Snapshot) =>
    [
      item.saves.length > 0 ? format(ui.snapshots.worlds, { count: String(item.saves.length) }) : '',
      item.mods > 0 ? format(ui.snapshots.mods, { count: String(item.mods) }) : '',
    ]
      .filter(Boolean)
      .join(' · ') || format(ui.snapshots.files, { count: String(item.files) })

  const rows = $derived<SnapshotRow[]>(
    snapshots.map((item) => ({
      id: item.id,
      takenAt: item.takenAt,
      title: item.label ?? why(item).title,
      pinned: pinned(item),
      inconsistent: item.inconsistent,
      meta: contents(item),
    })),
  )

  const byId = $derived(new Map(snapshots.map((item) => [item.id, item])))

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    try {
      snapshots = await listSnapshots(instanceId)
      error = ''
      // 占用是全局账本，这里只取和这个实例有关的那一个数。
      const usage = await backupUsage()
      reclaimable = usage.instances.find((it) => it.instance === instanceId)?.reclaimable ?? 0
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  async function take() {
    taking = true
    try {
      const snapshot = await takeSnapshot(instanceId)
      error = ''
      notices.say({
        title: ui.snapshots.taken,
        detail: format(ui.snapshots.takenFiles, { count: String(snapshot.files) }),
      })
      await load()
    } catch (cause) {
      error = String(cause)
    } finally {
      taking = false
    }
  }

  $effect(() => {
    instanceId
    void load()
  })
</script>

<section class="snapshots">
  <SectionHead title={ui.snapshots.head}>
    {#snippet note()}
      {#if snapshots.length > 0}
        <small class="t-quiet">
          {format(ui.snapshots.count, { count: String(snapshots.length) })} · {formatBytes(
            reclaimable,
          )}
        </small>
      {/if}
    {/snippet}
    {#snippet actions()}
      <Button variant="ghost" loading={taking} disabled={running} onclick={() => void take()}>
        {#snippet icon()}<Camera size={14} strokeWidth={1.9} />{/snippet}
        {ui.snapshots.take}
      </Button>
    {/snippet}
  </SectionHead>

  <p class="t-quiet note">
    {#if running}
      {ui.snapshots.noteRunning}
    {:else}
      {ui.snapshots.noteAuto}
      {ui.snapshots.noteRetention}
    {/if}
  </p>

  {#if loading}
    <Loading note={ui.snapshots.loading} />
  {:else if snapshots.length === 0}
    <div class="empty">
      <p class="lead">{ui.snapshots.emptyLead}</p>
      <p class="t-quiet">{ui.snapshots.emptyDetail}</p>
    </div>
  {:else}
    <SnapshotList {rows} onpick={(row) => (open = byId.get(row.id))} />
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</section>

{#if open}
  <SnapshotSheet
    {instanceId}
    snapshot={open}
    onclose={() => (open = undefined)}
    onchanged={() => void load()}
  />
{/if}

<style>
  .note {
    margin: var(--s2) 0 var(--s5);
    max-width: 62ch;
    font-size: var(--t-small);
    line-height: 1.6;
  }

  /* 空状态靠排版撑住，不是一行灰字加一个耸肩（见 docs/frond-design-system.md）。 */
  .empty {
    max-width: 46ch;
    padding: var(--s6) 0 var(--s7);
  }

  .empty .lead {
    margin: 0 0 var(--s2);
    color: var(--ink-2);
    font-size: var(--t-lead);
  }

  .empty p:last-child {
    margin: 0;
    line-height: 1.7;
  }

  .alert {
    margin-top: var(--s4);
  }
</style>
