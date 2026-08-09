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
  import { save as pickPath } from '@tauri-apps/plugin-dialog'
  import { FolderOpen } from 'lucide-svelte'
  import Loading from './Loading.svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { notices } from '../lib/notices.svelte'
  import { exportWorld, fileStem } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'

  interface SaveEntry {
    name: string
    bytes: number
    modified?: number
  }

  interface Props {
    instanceId: string
    /** 只用来给导出的文件起个默认名。 */
    instanceName?: string
  }

  let { instanceId, instanceName = '' }: Props = $props()

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

  /**
   * 把一个世界打成 zip。
   *
   * 这个动作长在存档那一行上，而不是收进导出面板：分享的单位是「这一个世界」，
   * 而人是先认出那个世界、再想把它发出去的。
   */
  let exporting = $state('')

  async function shareWorld(name: string) {
    const destination = await pickPath({
      defaultPath: `${fileStem(name)}.zip`,
      filters: [{ name: '压缩包', extensions: ['zip'] }],
    })
    if (!destination) return
    exporting = name
    try {
      const result = await exportWorld(instanceId, name, destination)
      error = ''
      notices.say({ title: `已导出「${name}」`, detail: formatBytes(result.bytes) })
    } catch (cause) {
      error = String(cause)
    } finally {
      exporting = ''
    }
  }
</script>

<section class="saves">
  <div class="head">
    <span class="label">
      存档
      {#if saves.length > 0}<small class="t-quiet">{saves.length} 个世界</small>{/if}
    </span>
    <Button variant="link" onclick={() => void invoke('open_instance_directory', { instanceId, sub: 'saves' })}>
      <FolderOpen size={13} strokeWidth={1.9} />存档目录
    </Button>
  </div>

  {#if loading}
    <Loading note="读取存档" />
  {:else if saves.length === 0}
    <p class="t-quiet empty">还没有存档。进游戏创建一个世界之后会出现在这里。</p>
  {:else}
    <ul class="list">
      {#each saves as item (item.name)}
        <li class="row">
          <span class="name">{item.name}</span>
          <span class="t-mono when">{day(item.modified)}</span>
          <span class="t-mono size">{formatBytes(item.bytes)}</span>
          <Button
            variant="link"
            class="share"
            disabled={exporting !== ''}
            onclick={() => void shareWorld(item.name)}>
            {exporting === item.name ? '导出中' : '导出'}
          </Button>
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

  /* 一行一个动作，平时收着——列表是用来认出那个世界的，不是一排按钮。 */
  /* 布局归调用方，但 Svelte 的作用域样式进不了组件，所以罩一层自己的祖先。 */
  .row :global(.share) {
    flex: none;
    width: 4ch;
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease);
  }

  .row:hover :global(.share),
  .row :global(.share:focus-visible),
  .row :global(.share:disabled) {
    opacity: 1;
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
