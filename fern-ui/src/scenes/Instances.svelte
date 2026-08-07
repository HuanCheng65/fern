<script lang="ts">
  /**
   * 实例场景。
   *
   * 没有页面标题——顶栏那个词已经亮着了，再写一遍「实例」只是占地方。
   * 这一屏的排版锚点是右边那张封面和它下面的名字。
   *
   * 左右两栏都不套卡片：内容直接坐在背景上，靠留白和一条发丝线分组。
   * 玻璃和影子只留给浮层。
   */
  import { FolderOpen, Play, Plus, RefreshCw, SlidersHorizontal } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import ModList from '../components/ModList.svelte'
  import { instances } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { prefs } from '../lib/prefs.svelte'

  interface Props {
    oncreate: () => void
    onopenDirectory: () => void
    onconfigure: () => void
  }

  let { oncreate, onopenDirectory, onconfigure }: Props = $props()

  const current = $derived(instances.current)
</script>

{#if instances.list.length === 0}
  <section class="blank">
    <h1 class="t-h1">{instances.loading ? '正在读取实例' : '暂无实例'}</h1>
    {#if !instances.loading}
      <button class="btn btn--ghost" onclick={oncreate}><Plus size={15} />新建实例</button>
    {/if}
    {#if instances.error}<div class="alert">{instances.error}</div>{/if}
  </section>
{:else}
  <section class="split">
    <div class="side">
      <div class="side-head">
        <span class="t-quiet">{instances.list.length} 个实例</span>
        <button class="btn btn--icon" aria-label="新建实例" title="新建实例" onclick={oncreate}>
          <Plus size={16} />
        </button>
      </div>
      <div class="list scroll">
        {#each instances.list as item (item.id)}
          <button
            class="row"
            class:on={current?.id === item.id}
            onclick={() => instances.select(item.id)}
          >
            <span class="thumb"><Cover seed={item.cover} quality={0.45} /></span>
            <span class="row-text">
              <strong>{item.name}</strong>
              <small class="t-mono">{item.gameVersion} · {item.loader}</small>
            </span>
          </button>
        {/each}
      </div>
    </div>

    {#if current}
      <div class="detail scroll">
        <div class="banner"><Cover seed={current.cover} quality={0.7} /></div>

        <h1 class="t-h1 title">{current.name}</h1>

        <dl class="facts">
          <div><dt>Minecraft</dt><dd class="t-mono">{current.gameVersion}</dd></div>
          <div><dt>加载器</dt><dd class="t-mono">{current.loader}</dd></div>
          <div><dt>实例 ID</dt><dd class="t-mono selectable">{current.id}</dd></div>
        </dl>

        <div class="actions">
          <button
            class="btn btn--primary"
            disabled={launch.busy || launch.running}
            onclick={() => void launch.launch(current.id, prefs.playerName)}
          >
            <Play size={15} fill="currentColor" strokeWidth={0} />
            {launch.running ? '运行中' : '启动'}
          </button>
          <button class="btn btn--ghost" onclick={onopenDirectory}>
            <FolderOpen size={15} strokeWidth={1.8} />游戏目录
          </button>
          <button
            class="btn btn--ghost"
            disabled={launch.busy}
            onclick={() => void launch.repair(current.id)}
          >
            <RefreshCw size={15} strokeWidth={1.8} />校验文件
          </button>
          <button class="btn btn--ghost" onclick={onconfigure}>
            <SlidersHorizontal size={15} strokeWidth={1.8} />设置
          </button>
        </div>

        {#if launch.busy}
          <p class="status t-mono">{launch.label}{launch.detail ? ` · ${launch.detail}` : ''}</p>
        {/if}
        {#if launch.error}<div class="alert">{launch.error}</div>{/if}

        <!-- 实例内的复杂度收在实例里，不摊到全局导航上（见 UI_DESIGN 四）。 -->
        {#key current.id}
          <ModList instanceId={current.id} />
        {/key}
      </div>
    {/if}
  </section>
{/if}

<style>
  .blank {
    display: grid;
    place-content: center;
    justify-items: start;
    gap: var(--s4);
    height: 100%;
  }

  .split {
    display: grid;
    grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
    gap: clamp(var(--s6), 5vw, var(--s8));
    height: 100%;
    min-height: 0;
  }

  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .side-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--s2) var(--s3);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .list {
    flex: 1;
    min-height: 0;
    padding: var(--s2) var(--s1) var(--s2) 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2);
    border-radius: var(--r1);
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .row:hover {
    background: var(--tint-1);
  }

  .row.on {
    color: var(--ink);
    background: var(--tint-2);
  }

  .thumb {
    display: block;
    width: 32px;
    height: 32px;
    flex: none;
    overflow: hidden;
    border-radius: calc(var(--r1) * 0.85);
  }

  .row-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .row-text strong {
    overflow: hidden;
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-text small {
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .detail {
    min-width: 0;
    padding-right: var(--s2);
    padding-bottom: var(--s6);
  }

  /* 封面就是实例的脸——详情页顶上给它一整条。 */
  .banner {
    aspect-ratio: 2.9;
    max-height: 34vh;
    overflow: hidden;
    border-radius: var(--r3);
  }

  .title {
    margin: var(--s5) 0 0;
    overflow-wrap: anywhere;
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s3) var(--s7);
    margin: var(--s5) 0 0;
    padding: var(--s4) 0;
    box-shadow:
      inset 0 1px 0 var(--hairline-2),
      inset 0 -1px 0 var(--hairline-2);
  }

  .facts div {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  dt {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  dd {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-top: var(--s5);
  }

  .status {
    margin: var(--s4) 0 0;
    color: var(--ink-3);
  }

  .alert {
    margin-top: var(--s4);
    max-width: 62ch;
  }

  @media (max-width: 860px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(120px, 30%) minmax(0, 1fr);
      gap: var(--s5);
    }
  }
</style>
