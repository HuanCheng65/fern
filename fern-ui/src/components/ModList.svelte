<script lang="ts">
  /**
   * 实例装了哪些模组。
   *
   * 这是设计文档里说的「真正需要密度的地方」——一行一个模组，不套卡片、不加
   * 缩略图。几十上百行的列表里，每一点装饰都会乘以行数。
   *
   * 停用的模组留在列表里，只是压暗：文件其实还在磁盘上（加了 `.disabled`
   * 后缀），从列表里消失会让人以为被删了。
   *
   * 本地的 jar 靠拖放安装。Tauri 的拖放事件给的是真实路径，而 webview 里的
   * 文件选择框拿不到——所以这里没有「浏览」按钮。要找新模组走「添加模组」，
   * 它带着这个实例跳到补给站。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { FolderOpen, Plus, Trash2 } from 'lucide-svelte'
  import { onMount } from 'svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/launch.svelte'
  import Loading from './Loading.svelte'
  import { nav } from '../lib/nav.svelte'

  interface ModFile {
    fileName: string
    name: string
    version?: string
    enabled: boolean
    bytes: number
  }

  interface Props {
    instanceId: string
  }

  let { instanceId }: Props = $props()

  let mods = $state<ModFile[]>([])
  let loading = $state(true)
  let error = $state('')
  let dropping = $state(false)

  const enabledCount = $derived(mods.filter((item) => item.enabled).length)

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    try {
      mods = await invoke<ModFile[]>('list_mods', { instanceId })
      error = ''
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  async function toggle(item: ModFile) {
    try {
      await invoke('set_mod_enabled', {
        instanceId,
        fileName: item.fileName,
        enabled: !item.enabled,
      })
      await load()
    } catch (cause) {
      error = String(cause)
    }
  }

  async function remove(item: ModFile) {
    try {
      await invoke('remove_mod', { instanceId, fileName: item.fileName })
      await load()
    } catch (cause) {
      error = String(cause)
    }
  }

  async function install(files: string[]) {
    const jars = files.filter((path) => path.toLowerCase().endsWith('.jar'))
    if (jars.length === 0) {
      error = '只能安装 jar 文件'
      return
    }
    try {
      await invoke('install_mods', { instanceId, pathsToInstall: jars })
      error = ''
      await load()
    } catch (cause) {
      error = String(cause)
    }
  }

  // 实例切换时重新读。
  $effect(() => {
    instanceId
    void load()
  })

  onMount(() => {
    if (!inTauri()) return
    let stop: (() => void) | undefined
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === 'over') dropping = true
        else if (event.payload.type === 'drop') {
          dropping = false
          void install(event.payload.paths)
        } else dropping = false
      })
      .then((unlisten) => (stop = unlisten))
    return () => stop?.()
  })
</script>

<section class="mods" class:dropping>
  <div class="head">
    <span class="label">
      模组
      {#if mods.length > 0}
        <small class="t-quiet">{enabledCount}/{mods.length} 启用</small>
      {/if}
    </span>
    <span class="acts">
      <!--
        带着实例跳到补给站，那边的筛选条件会对准它。跨场景跳转必须带参数，
        否则用户到了那边还要自己把版本和加载器再选一遍。
      -->
      <button class="btn btn--link" onclick={() => nav.enter('supply', '', { forInstance: instanceId })}>
        <Plus size={13} strokeWidth={2} />添加模组
      </button>
      <button
        class="btn btn--link"
        onclick={() => void invoke('open_instance_directory', { instanceId, sub: 'mods' })}
      >
        <FolderOpen size={13} strokeWidth={1.9} />模组目录
      </button>
    </span>
  </div>

  {#if loading}
    <Loading note="读取模组" />
  {:else if mods.length === 0}
    <p class="t-quiet empty">尚未安装模组。将 jar 文件拖入窗口即可安装。</p>
  {:else}
    <ul class="list">
      {#each mods as item (item.fileName)}
        <li class="row" class:off={!item.enabled}>
          <button
            class="toggle"
            role="switch"
            aria-checked={item.enabled}
            aria-label={item.enabled ? `停用 ${item.name}` : `启用 ${item.name}`}
            onclick={() => void toggle(item)}
          ></button>
          <span class="name">{item.name}</span>
          {#if item.version}<span class="t-mono version">{item.version}</span>{/if}
          <span class="t-mono size">{formatBytes(item.bytes)}</span>
          <button
            class="btn btn--icon"
            aria-label={`删除 ${item.name}`}
            title="删除"
            onclick={() => void remove(item)}
          >
            <Trash2 size={13} strokeWidth={1.8} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</section>

<style>
  .mods {
    margin-top: var(--s5);
    padding-top: var(--s4);
    box-shadow: inset 0 1px 0 var(--hairline-2);
    transition: background var(--t-fast) var(--ease);
  }

  /* 拖到窗口上时整块亮一下，说明这里接得住。 */
  .mods.dropping {
    background: var(--accent-soft);
    border-radius: var(--r2);
  }

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

  .acts {
    display: flex;
    gap: var(--s4);
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
    padding: var(--s2) var(--s2) var(--s2) 0;
    border-radius: var(--r1);
    transition:
      background var(--t-fast) var(--ease),
      opacity var(--t-fast) var(--ease);
  }

  .row:hover {
    background: var(--tint-1);
  }

  /* 停用的压暗但不移走：文件还在磁盘上，从列表里消失会让人以为被删了。 */
  .row.off {
    opacity: 0.45;
  }

  /* 开关是一条短横，亮起表示启用——比一个方形复选框安静。 */
  .toggle {
    width: 26px;
    height: 14px;
    flex: none;
    padding: 0;
    border-radius: 999px;
    background: var(--tint-2);
    transition: background var(--t-base) var(--ease);
  }

  .toggle[aria-checked='true'] {
    background: var(--accent);
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

  .version,
  .size {
    flex: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .size {
    width: 8ch;
    text-align: right;
  }

  .alert {
    margin-top: var(--s3);
  }

  @media (max-width: 720px) {
    .version {
      display: none;
    }
  }
</style>
