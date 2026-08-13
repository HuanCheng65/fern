<script lang="ts">
  /**
   * 实例里的存档。
   *
   * 三个动作长在行上：进入、导出、删除。
   *
   * 「进入」是这一屏最该有的那一个——人在这里盯着的就是那个世界的名字，而
   * 游戏本来就支持直接进去（quickPlay），此前只有命令面板用得上它。
   *
   * 「删除」移到系统回收站，不真删：一个世界是玩家投入最多的东西，而它旁边
   * 那两个动作都是无害的。移到回收站之后，删错了在文件管理器里就能还原。
   *
   * 显示的是目录名而不是世界名：目录名是它在磁盘上的真实身份，两个都叫
   * 「新的世界」的存档在列表里长得一模一样反而认不出来。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { save as pickPath } from '@tauri-apps/plugin-dialog'
  import { FolderOpen, Play } from 'lucide-svelte'
  import Loading from './Loading.svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { notices } from '../lib/notices.svelte'
  import { exportWorld, fileStem } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'
  import Dialog from 'fern-kit/ui/Dialog.svelte'
  import SectionHead from 'fern-kit/ui/SectionHead.svelte'

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

  /**
   * 直接进这个世界。
   *
   * 走的是启动那条正路，只是多带一个参数——所以补全、预检查、账户这些一样
   * 会发生，界面上也是同一套进度。
   */
  function enter(name: string) {
    void launch.launch(instanceId, { world: name })
  }

  /** 正在问「要删吗」的那个世界。空字符串是没在问。 */
  let removing = $state('')
  let trashing = $state(false)

  async function trash() {
    const name = removing
    trashing = true
    try {
      await invoke('trash_save', { instanceId, save: name })
      error = ''
      removing = ''
      notices.say({ title: `已删除「${name}」`, detail: '文件在系统回收站中，可以还原。' })
      await load()
    } catch (cause) {
      // 回收站用不上时（跨文件系统、沙箱里没有权限）后端会直说，不会退回真删。
      error = String(cause)
      removing = ''
    } finally {
      trashing = false
    }
  }
</script>

<section class="saves">
  <SectionHead title="存档">
    {#snippet note()}
      {#if saves.length > 0}<small class="t-quiet">{saves.length} 个世界</small>{/if}
    {/snippet}
    {#snippet actions()}
      <Button variant="link" onclick={() => void invoke('open_instance_directory', { instanceId, sub: 'saves' })}>
        <FolderOpen size={13} strokeWidth={1.9} />存档目录
      </Button>
    {/snippet}
  </SectionHead>

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
          <span class="acts" class:busy={exporting === item.name}>
            <Button
              variant="link"
              disabled={launch.occupied(instanceId)}
              onclick={() => enter(item.name)}>
              <Play size={12} fill="currentColor" strokeWidth={0} />进入
            </Button>
            <Button
              variant="link"
              disabled={exporting !== ''}
              onclick={() => void shareWorld(item.name)}>
              {exporting === item.name ? '导出中' : '导出'}
            </Button>
            <Button variant="link" tone="quiet" onclick={() => (removing = item.name)}>删除</Button>
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</section>

{#if removing}
  <Dialog label="删除存档" width="400px" onclose={() => (removing = '')}>
    <div class="ask">
      <h2>删除「{removing}」</h2>
      <p>这个世界将被移到系统回收站，可以在文件管理器中还原。</p>
      <div class="ask-acts">
        <Button variant="ghost" onclick={() => (removing = '')}>取消</Button>
        <Button variant="primary" loading={trashing} onclick={() => void trash()}>
          移到回收站
        </Button>
      </div>
    </div>
  </Dialog>
{/if}

<style>
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

  /* 动作平时收着——列表是用来认出那个世界的，不是一排按钮。 */
  .acts {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--s3);
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease);
  }

  .row:hover .acts,
  .acts:focus-within,
  /* 正在导出的那一行始终露着：鼠标移开不该让「导出中」跟着消失。 */
  .acts.busy {
    opacity: 1;
  }

  .ask {
    display: grid;
    gap: var(--s3);
    padding: var(--s5);
  }

  .ask h2 {
    margin: 0;
    color: var(--ink);
    font-size: var(--t-h3);
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .ask p {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.6;
  }

  .ask-acts {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
    margin-top: var(--s2);
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
