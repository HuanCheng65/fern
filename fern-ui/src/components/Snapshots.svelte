<script lang="ts">
  /**
   * 实例的快照。
   *
   * 排版按「时间轴」组织而不是一张平铺的表：快照唯一的排序依据是时间，而人
   * 找的是「装那个模组之前那一张」——按天分组之后，找的动作从读十二行时间戳
   * 变成了先落到某一天再挑一行。
   *
   * 一行只回答三件事，从左到右：**什么时候、为什么在这里、有多大**。别的都
   * 收进浮层——恢复要做的选择（恢复哪一部分、覆盖还是另存）是一次决定，不是
   * 一行里塞得下的三颗按钮。二十行各三颗按钮的列表没法扫读。
   *
   * 所以整行是一个按钮：点进去做决定，列表本身只用来找。
   */
  import { Camera, CircleAlert, Pin } from 'lucide-svelte'
  import Loading from './Loading.svelte'
  import SnapshotSheet from './SnapshotSheet.svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { notices } from '../lib/notices.svelte'
  import {
    backupUsage,
    clock,
    day,
    dayLabel,
    listSnapshots,
    pinned,
    takeSnapshot,
    why,
    type Snapshot,
  } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'

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

  /** 按天分组，新的在前。列表本身已经是从新到旧的。 */
  const groups = $derived.by(() => {
    const out: { key: string; label: string; items: Snapshot[] }[] = []
    for (const item of snapshots) {
      const key = day(item.takenAt)
      const last = out.at(-1)
      if (last?.key === key) last.items.push(item)
      else out.push({ key, label: dayLabel(item.takenAt), items: [item] })
    }
    return out
  })

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
        title: '已拍下快照',
        detail: `${snapshot.files} 个文件`,
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
  <div class="head">
    <span class="label">
      快照
      {#if snapshots.length > 0}
        <small class="t-quiet">
          {snapshots.length} 张 · {formatBytes(reclaimable)}
        </small>
      {/if}
    </span>
    <Button variant="ghost" disabled={taking || running} onclick={() => void take()}>
      <Camera size={14} strokeWidth={1.9} />
      {taking ? '正在拍摄' : '拍一张'}
    </Button>
  </div>

  <p class="t-quiet note">
    {#if running}
      游戏运行时存档正在写入，此时拍下的内容不完整。请先退出游戏。
    {:else}
      改动模组前和游戏结束后会自动拍下。多张快照之间相同的文件只存一份。
    {/if}
  </p>

  {#if loading}
    <Loading note="读取快照" />
  {:else if snapshots.length === 0}
    <div class="empty">
      <p class="lead">还没有快照。</p>
      <p class="t-quiet">
        第一张会在下次改动模组或结束游戏时自动拍下，也可以现在手动拍一张。
      </p>
    </div>
  {:else}
    {#each groups as group (group.key)}
      <div class="group">
        <h3 class="day">{group.label}</h3>
        <ul class="list">
          {#each group.items as item (item.id)}
            <li>
              <button class="row" onclick={() => (open = item)}>
                <span class="t-mono when">{clock(item.takenAt)}</span>
                <span class="what">
                  {item.label ?? why(item).title}
                  {#if pinned(item)}
                    <span class="pin" title="永久保留">
                      <Pin size={11} strokeWidth={2} />
                    </span>
                  {/if}
                  {#if item.inconsistent}
                    <span class="flag" title="拍摄时文件仍在变动，内容可能不一致">
                      <CircleAlert size={11} strokeWidth={2} />
                    </span>
                  {/if}
                </span>
                <span class="t-mono meta">
                  {#if item.saves.length > 0}{item.saves.length} 个世界 · {/if}{formatBytes(
                    item.bytes,
                  )}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
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
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .label {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .label small {
    margin-left: var(--s2);
    font-weight: 400;
  }

  .note {
    margin: var(--s2) 0 var(--s5);
    max-width: 62ch;
    font-size: var(--t-small);
    line-height: 1.6;
  }

  /* 空状态靠排版撑住，不是一行灰字加一个耸肩（见 docs/UI_DESIGN.md 一）。 */
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

  .group + .group {
    margin-top: var(--s5);
  }

  /* 日期是分隔，不是标题——它不该和实例名争视觉层级。 */
  .day {
    margin: 0 0 var(--s1);
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-weight: 500;
    letter-spacing: 0.04em;
  }

  .list {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: var(--s4);
    width: 100%;
    /* 一屏几十行时靠伪元素向外够会互相重叠，行高自己抬到点击区下限。 */
    min-height: var(--hit);
    padding: var(--s2) var(--s2);
    margin-left: calc(var(--s2) * -1);
    border-radius: var(--r1);
    text-align: left;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    transition: background var(--t-fast) var(--ease);
  }

  .row:hover {
    background: var(--tint-1);
    box-shadow: none;
  }

  .when {
    flex: none;
    width: 6ch;
    color: var(--ink-3);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .what {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: 1;
    min-width: 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pin,
  .flag {
    display: inline-grid;
    place-items: center;
    flex: none;
  }

  .pin {
    color: var(--ink-4);
  }

  .flag {
    color: var(--danger);
  }

  .meta {
    flex: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .alert {
    margin-top: var(--s4);
  }

  @media (max-width: 640px) {
    .meta {
      display: none;
    }
  }
</style>
