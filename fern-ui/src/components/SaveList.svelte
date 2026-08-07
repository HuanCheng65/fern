<script lang="ts">
  /**
   * 实例里的存档。
   *
   * 只读。删一个世界是不可挽回的，而「打开存档目录」已经能满足所有真实
   * 需求——文件管理器里删，至少还有回收站。
   *
   * 显示的是目录名而不是世界名：目录名是它在磁盘上的真实身份，两个都叫
   * 「新的世界」的存档在列表里长得一模一样反而认不出来。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { FolderOpen } from 'lucide-svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/launch.svelte'

  interface SaveEntry {
    name: string
    bytes: number
    modified?: number
  }

  interface Props {
    instanceId: string
  }

  let { instanceId }: Props = $props()

  let saves = $state<SaveEntry[]>([])
  let loading = $state(true)
  let error = $state('')

  const day = (seconds?: number) =>
    seconds === undefined ? '' : new Date(seconds * 1000).toLocaleDateString('zh-CN')

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    loading = true
    try {
      saves = await invoke<SaveEntry[]>('list_saves', { instanceId })
      error = ''
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  $effect(() => {
    instanceId
    void load()
  })
</script>

<section class="saves">
  <div class="head">
    <span class="label">
      存档
      {#if saves.length > 0}<small class="t-quiet">{saves.length} 个世界</small>{/if}
    </span>
    <button
      class="btn btn--link"
      onclick={() => void invoke('open_instance_directory', { instanceId, sub: 'saves' })}
    >
      <FolderOpen size={13} strokeWidth={1.9} />存档目录
    </button>
  </div>

  {#if loading}
    <p class="t-quiet empty">读取中</p>
  {:else if saves.length === 0}
    <p class="t-quiet empty">还没有存档。进游戏创建一个世界之后会出现在这里。</p>
  {:else}
    <ul class="list">
      {#each saves as item (item.name)}
        <li class="row">
          <span class="name">{item.name}</span>
          <span class="t-mono when">{day(item.modified)}</span>
          <span class="t-mono size">{formatBytes(item.bytes)}</span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</section>

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

  .empty {
    margin: var(--s3) 0 0;
  }

  .list {
    display: grid;
    gap: 1px;
    margin: var(--s2) 0 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row:last-child {
    box-shadow: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--t-body);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .when,
  .size {
    flex: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .size {
    width: 9ch;
    text-align: right;
  }

  .alert {
    margin-top: var(--s3);
  }

  @media (max-width: 720px) {
    .when {
      display: none;
    }
  }
</style>
