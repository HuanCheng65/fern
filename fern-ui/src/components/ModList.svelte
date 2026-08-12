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
   *
   * 更新是问出来的，不是盯出来的：按下「检查更新」才联网，有新版的那几行才多
   * 出一颗按钮。自动检查会让每次打开这一屏都等一次网络，而答案几天才变一次。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { preflight } from '../lib/preflight.svelte'
  import { integrity } from '../lib/integrity.svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { FolderOpen, Plus, RefreshCw, Trash2 } from 'lucide-svelte'
  import { onMount } from 'svelte'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import Loading from './Loading.svelte'
  import { nav } from '../lib/nav.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface ModFile {
    fileName: string
    name: string
    version?: string
    enabled: boolean
    bytes: number
  }

  /** 有新版的那一个。只有查过之后才存在。 */
  interface ModUpdate {
    fileName: string
    title: string
    current: string
    latest: string
    versionId: string
  }

  interface Props {
    instanceId: string
  }

  let { instanceId }: Props = $props()

  let mods = $state<ModFile[]>([])
  let loading = $state(true)
  let error = $state('')
  let dropping = $state(false)
  /**
   * 正在改的那一个。
   *
   * 装、删、开关模组之前，核心会先拍一张快照（见 docs/fern-backup-design.md
   * §5）——第一次要读完整个游戏目录，几秒起步。没有这个状态的话，那几秒里
   * 界面看起来就是「点了没反应」。
   */
  let busy = $state('')

  const enabledCount = $derived(mods.filter((item) => item.enabled).length)

  /**
   * 有新版的那些，按文件名索引。
   *
   * **只在按下「检查更新」时才有内容。** 打开列表就联网，等于替所有人决定这
   * 件事值得一次等待；而一个实例的模组是不是最新，几天才变一次。
   */
  let updates = $state<Record<string, ModUpdate>>({})
  let checking = $state(false)
  /** 查过没有。没查过和查完发现都是最新，是两句不同的话。 */
  let checked = $state(false)
  const updateCount = $derived(Object.keys(updates).length)

  /** 模组变了，启动前预检查和文件对账的结论也就都变了。 */
  function recheck() {
    preflight.refresh(instanceId)
    integrity.refresh(instanceId)
  }

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
    busy = item.fileName
    try {
      await invoke('set_mod_enabled', {
        instanceId,
        fileName: item.fileName,
        enabled: !item.enabled,
      })
      recheck()
      await load()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
    }
  }

  async function remove(item: ModFile) {
    busy = item.fileName
    try {
      await invoke('remove_mod', { instanceId, fileName: item.fileName })
      recheck()
      await load()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
    }
  }

  /**
   * 查一遍哪些模组有新版。
   *
   * 要把 mods 目录读一遍算哈希，再问一次 Modrinth——几秒起步，所以按钮上要有
   * 等待的样子。认不出身份的文件（本地构建、别处下的）不会出现在结果里：说不
   * 出它是什么，就说不出它有没有新版。
   */
  async function check() {
    checking = true
    try {
      const found = await invoke<ModUpdate[]>('mod_updates', { instanceId })
      updates = Object.fromEntries(found.map((item) => [item.fileName, item]))
      checked = true
      error = ''
    } catch (cause) {
      error = String(cause)
    } finally {
      checking = false
    }
  }

  /** 换成新版。装上新的、撤掉旧的在后端是一步，中途断掉不会留下两份。 */
  async function update(item: ModFile) {
    const found = updates[item.fileName]
    if (!found) return
    busy = item.fileName
    try {
      await invoke('update_mod', {
        instanceId,
        fileName: item.fileName,
        versionId: found.versionId,
        title: `更新 ${found.title}`,
        subjects: [instanceId],
      })
      delete updates[item.fileName]
      recheck()
      error = ''
      await load()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
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
      recheck()
      error = ''
      await load()
    } catch (cause) {
      error = String(cause)
    }
  }

  // 实例切换时重新读。上一个实例的检查结果一并作废——它说的是别人的模组。
  $effect(() => {
    instanceId
    updates = {}
    checked = false
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
      <!--
        查完要有句回话。说的是「没找到」，不是「都是最新的」——认不出身份的
        文件本来就不在这次检查的范围里，替它们担保是在编造一个我们没有的结论。
      -->
      {#if checked}
        <small class="t-quiet">
          {updateCount > 0 ? `${updateCount} 个可更新` : '未发现可用更新'}
        </small>
      {/if}
    </span>
    <span class="acts">
      {#if mods.length > 0}
        <Button variant="link" loading={checking} disabled={busy !== ''} onclick={() => void check()}>
          <RefreshCw size={13} strokeWidth={1.9} />检查更新
        </Button>
      {/if}
      <!--
        带着实例跳到补给站，那边的筛选条件会对准它。跨场景跳转必须带参数，
        否则用户到了那边还要自己把版本和加载器再选一遍。
      -->
      <Button variant="link" onclick={() => nav.enter('supply', '', { forInstance: instanceId })}>
        <Plus size={13} strokeWidth={2} />添加模组
      </Button>
      <Button variant="link" onclick={() => void invoke('open_instance_directory', { instanceId, sub: 'mods' })}>
        <FolderOpen size={13} strokeWidth={1.9} />模组目录
      </Button>
    </span>
  </div>

  {#if loading}
    <Loading note="读取模组" />
  {:else if mods.length === 0}
    <p class="t-quiet empty">尚未安装模组。将 jar 文件拖入窗口即可安装。</p>
  {:else}
    <ul class="list">
      {#each mods as item (item.fileName)}
        <li class="row" class:off={!item.enabled} class:busy={busy === item.fileName}>
          <button
            class="toggle"
            role="switch"
            aria-checked={item.enabled}
            aria-label={item.enabled ? `停用 ${item.name}` : `启用 ${item.name}`}
            disabled={busy !== ''}
            onclick={() => void toggle(item)}
          ></button>
          <span class="name">{item.name}</span>
          {#if item.version}<span class="t-mono version">{item.version}</span>{/if}
          {#if updates[item.fileName]}
            <!-- 新版号写在行上，而不是只给一颗按钮：要装的是哪一版，按之前就该看见。 -->
            <Button
              variant="link"
              disabled={busy !== ''}
              onclick={() => void update(item)}>
              更新至 {updates[item.fileName]!.latest}
            </Button>
          {/if}
          <span class="t-mono size">{formatBytes(item.bytes)}</span>
          <Button
            variant="icon"
            aria-label={`删除 ${item.name}`}
            title="删除"
            disabled={busy !== ''}
            onclick={() => void remove(item)}>
            <Trash2 size={13} strokeWidth={1.8} />
          </Button>
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

  /*
   * 正在改的那一行。改动模组之前要先拍一张快照，第一次可能要几秒——那几秒里
   * 这一行得看得出来「正在处理」，否则就是「点了没反应」。
   *
   * 用一道横向扫过的高光，不加转圈：这一行的高度只有二十来像素，塞一个
   * spinner 会把整行的节奏打乱，而它要说的只是「还在动」。
   */
  .row.busy {
    background: var(--tint-1);
  }

  .row.busy .name::after {
    content: '';
    display: inline-block;
    width: 3.2em;
    height: 1px;
    margin-left: var(--s2);
    vertical-align: middle;
    background: linear-gradient(90deg, transparent, var(--accent), transparent);
    animation: sweep calc(var(--t-slow) * 4) linear infinite;
  }

  @keyframes sweep {
    from {
      opacity: 0.15;
      transform: translateX(-40%);
    }
    50% {
      opacity: 1;
    }
    to {
      opacity: 0.15;
      transform: translateX(40%);
    }
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
