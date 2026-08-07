<script lang="ts">
  /**
   * 实例详情——实例场景向内的那一级。
   *
   * 结构固定：横幅加五个 tab，tab 不再往下嵌套。所以整个应用最深三步，
   * 任何位置一次返回就回到场景首页。
   *
   * 启动按钮在这里常驻，是刻意的：管理和游玩之间不该有距离。翻完模组列表
   * 想立刻试一下，不该先返回、再设为当前、再去启动场景。
   *
   * 横幅就是这张封面本身——它从曲库的卡片位置展开到这里，是贯穿两级的视觉
   * 锚点，实例的身份感靠它建立。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { Check, FolderOpen, Play, RefreshCw } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import Detail from '../layouts/Detail.svelte'
  import InstanceSettings from '../components/InstanceSettings.svelte'
  import LogLines from '../components/LogLines.svelte'
  import ModList from '../components/ModList.svelte'
  import SaveList from '../components/SaveList.svelte'
  import { instances, type Instance } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { prefs } from '../lib/prefs.svelte'

  interface Props {
    instance: Instance
  }

  let { instance }: Props = $props()

  /** 原版实例装不了模组，那个 tab 摆在那里只会浪费一次点击。 */
  const tabs = $derived([
    { id: 'overview', label: '概览', reading: true },
    ...(instance.loader === 'Vanilla' ? [] : [{ id: 'mods', label: '模组' }]),
    { id: 'saves', label: '存档' },
    { id: 'settings', label: '设置', reading: true },
    { id: 'log', label: '日志' },
  ])

  const tab = $derived(tabs.some((item) => item.id === nav.tab) ? nav.tab : 'overview')
  const isCurrent = $derived(instances.current?.id === instance.id)
  /** 这段日志是不是这个实例的。别的实例的崩溃栈显示在这里比不显示更糟。 */
  const ownLog = $derived(launch.instanceId === instance.id ? launch.log : [])

  const played = $derived(
    instance.lastPlayed === undefined
      ? '尚未玩过'
      : `上次游玩 ${new Date(instance.lastPlayed * 1000).toLocaleDateString('zh-CN')}`,
  )
</script>

<Detail {tabs} {tab} ontab={(id) => nav.setTab(id)}>
  {#snippet banner()}
    <Cover seed={instance.cover} quality={0.7} />
    <div class="banner-fade"></div>
  {/snippet}

  {#snippet head()}
    <div class="titles">
      <div>
        <h1 class="t-h1">{instance.name}</h1>
        <p class="t-mono facts">
          Minecraft {instance.gameVersion} · {instance.loader} · {played}
        </p>
      </div>

      <div class="acts">
        <button
          class="btn btn--primary"
          disabled={launch.busy || launch.running}
          onclick={() => void launch.launch(instance.id, prefs.playerName)}
        >
          {#if launch.running}
            运行中
          {:else if launch.busy}
            {launch.label || '准备中'}
          {:else}
            <Play size={15} fill="currentColor" strokeWidth={0} />启动
          {/if}
        </button>

        <!--
          「设为当前」和「打开详情」是两个动作。只想翻一眼模组列表的人，不该
          因此把启动场景上的实例换掉。
        -->
        {#if isCurrent}
          <span class="t-quiet now"><Check size={14} strokeWidth={2.2} />当前实例</span>
        {:else}
          <button class="btn btn--ghost" onclick={() => instances.select(instance.id)}>
            设为当前
          </button>
        {/if}
      </div>
    </div>
  {/snippet}

  {#snippet compactHead()}
    <span class="mini-title">{instance.name}</span>
  {/snippet}

  {#if tab === 'overview'}
    <dl class="grid">
      <div><dt>Minecraft</dt><dd class="t-mono">{instance.gameVersion}</dd></div>
      <div><dt>加载器</dt><dd class="t-mono">{instance.loader}</dd></div>
      <div><dt>实例 ID</dt><dd class="t-mono selectable">{instance.id}</dd></div>
    </dl>
    <div class="links">
      <button
        class="btn btn--link"
        onclick={() => void invoke('open_instance_directory', { instanceId: instance.id })}
      >
        <FolderOpen size={13} strokeWidth={1.9} />游戏目录
      </button>
      <button class="btn btn--link" onclick={() => void launch.repair(instance.id)}>
        <RefreshCw size={13} strokeWidth={1.9} />校验文件
      </button>
    </div>
    {#if launch.error}
      <div class="alert">{launch.error}</div>
    {/if}
  {:else if tab === 'mods'}
    <ModList instanceId={instance.id} />
  {:else if tab === 'saves'}
    <SaveList instanceId={instance.id} />
  {:else if tab === 'settings'}
    <InstanceSettings
      instanceId={instance.id}
      instanceName={instance.name}
      ongone={(replacement) => (replacement ? nav.open(replacement) : nav.back())}
    />
  {:else}
    <LogLines
      lines={ownLog}
      emptyNote={launch.instanceId === instance.id
        ? '本次运行尚无输出。'
        : '这个实例本次会话还没有运行过。'}
    />
    <button class="btn btn--link logs" onclick={() => void invoke('open_logs_directory')}>
      <FolderOpen size={13} strokeWidth={1.9} />日志目录
    </button>
  {/if}
</Detail>

<style>
  .mini-title {
    color: var(--ink-2);
  }

  /* 底边化开，标题才不像压在一张图上。 */
  .titles {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--s4);
    min-width: 0;
  }

  .titles h1 {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .banner-fade {
    position: absolute;
    inset: auto 0 0;
    height: 55%;
    background: linear-gradient(to bottom, transparent, var(--bg, rgba(6, 8, 10, 0.55)));
    pointer-events: none;
  }

  .facts {
    margin: var(--s2) 0 0;
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .acts {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }

  .now {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .now :global(svg) {
    color: var(--accent);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--s4);
    margin: 0;
  }

  .grid dt {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .grid dd {
    margin: 4px 0 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    overflow-wrap: anywhere;
  }

  .links {
    display: flex;
    gap: var(--s4);
    margin-top: var(--s5);
  }

  .logs {
    align-self: flex-start;
    margin-top: var(--s4);
  }

  .alert {
    margin-top: var(--s4);
  }
</style>
