<script lang="ts">
  /**
   * 补给站。
   *
   * 「去哪里找」和「装了什么」是两件事：这一屏只管找和装，装完之后的状态在
   * 实例详情页的模组列表里。
   *
   * 结果按当前实例的游戏版本和加载器过滤。用户是在为某个实例找东西——列出一个
   * 它装不上的模组，只会浪费他一次点击才发现装不了。所以顶上一直写着这次是在
   * 为谁筛选。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ArrowRight, Search } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import ProjectVersions from '../components/ProjectVersions.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'

  interface Props {
    onback: () => void
  }

  let { onback }: Props = $props()

  interface Hit {
    projectId: string
    slug: string
    title: string
    description: string
    author: string
    downloads: number
    iconUrl?: string
    categories: string[]
  }

  let query = $state('')
  let hits = $state<Hit[]>([])
  let total = $state(0)
  let searching = $state(false)
  let error = $state('')
  let searched = $state(false)
  let picked = $state<Hit | null>(null)

  const target = $derived(instances.current)
  /** 原版实例没有加载器，装不了模组——与其给一列装不上的结果，不如直接说清。 */
  const modded = $derived(target !== undefined && target.loader !== 'Vanilla')

  const compact = (value: number) =>
    value >= 1_000_000
      ? `${(value / 1_000_000).toFixed(1)}M`
      : value >= 1000
        ? `${(value / 1000).toFixed(0)}K`
        : String(value)

  async function run() {
    if (!target || !inTauri()) return
    searching = true
    error = ''
    try {
      const result = await invoke<{ hits: Hit[]; total: number }>('search_mods', {
        query: query.trim(),
        instanceId: target.id,
        offset: 0,
        limit: 40,
      })
      hits = result.hits
      total = result.total
      searched = true
    } catch (cause) {
      error = String(cause)
    } finally {
      searching = false
    }
  }

  // 进来就给一屏热门的，空着一片白比什么都不给更糟。
  $effect(() => {
    if (modded && !searched && !searching) void run()
  })
</script>

<section class="supply">
  {#if !target}
    <div class="blank">
      <h1 class="t-h1">还没有实例</h1>
      <p class="note">模组要装进某个实例。先创建一个，再回到这里。</p>
      <button class="btn btn--link" onclick={onback}>返回启动<ArrowRight size={14} /></button>
    </div>
  {:else if !modded}
    <div class="blank">
      <h1 class="t-h1">这是一个原版实例</h1>
      <p class="note">
        {target.name} 没有安装模组加载器，无法安装模组。新建实例时选择 Fabric、Quilt、NeoForge
        或 Forge 即可。
      </p>
      <button class="btn btn--link" onclick={onback}>返回启动<ArrowRight size={14} /></button>
    </div>
  {:else}
    <header class="bar">
      <div class="field">
        <Search size={15} strokeWidth={1.9} />
        <input
          class="input"
          bind:value={query}
          spellcheck="false"
          placeholder="搜索模组"
          onkeydown={(event) => event.key === 'Enter' && void run()}
        />
      </div>
      <p class="t-quiet scope">
        为 <strong>{target.name}</strong> 筛选 · {target.gameVersion} · {target.loader}
      </p>
    </header>

    {#if error}
      <div class="alert">{error}</div>
    {:else if searching && hits.length === 0}
      <p class="t-quiet hint">搜索中</p>
    {:else if hits.length === 0}
      <p class="t-quiet hint">没有匹配的模组。</p>
    {:else}
      <div class="grid scroll">
        {#each hits as hit (hit.projectId)}
          <button class="card" onclick={() => (picked = hit)}>
            <span class="icon">
              {#if hit.iconUrl}
                <img src={hit.iconUrl} alt="" loading="lazy" />
              {:else}
                <!-- 没有图标的项目用生成式色块补位，网格才不会破相。 -->
                <Cover seed={hit.slug} quality={0.4} />
              {/if}
            </span>
            <span class="text">
              <strong>{hit.title}</strong>
              <small class="desc">{hit.description}</small>
              <small class="t-mono meta">{compact(hit.downloads)} · {hit.author}</small>
            </span>
          </button>
        {/each}
      </div>
      <p class="t-quiet count">共 {total} 个结果，显示前 {hits.length} 个</p>
    {/if}
  {/if}
</section>

{#if picked && target}
  <ProjectVersions
    project={picked.slug}
    title={picked.title}
    instanceId={target.id}
    onclose={() => (picked = null)}
  />
{/if}

<style>
  .supply {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .blank {
    display: grid;
    align-content: center;
    justify-items: start;
    gap: var(--s3);
    height: 100%;
    max-width: 46ch;
  }

  .note {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--s3);
    padding-bottom: var(--s4);
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--s2);
    width: min(420px, 100%);
    color: var(--ink-4);
  }

  .field .input {
    flex: 1;
    min-width: 0;
  }

  .scope {
    margin: 0;
  }

  .scope strong {
    color: var(--ink-2);
    font-weight: 500;
  }

  /* 瀑布流式的网格，卡片自己撑开——列数跟着窗口走，不写断点。 */
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--s2);
    align-content: start;
    padding-right: var(--s2);
  }

  .card {
    display: flex;
    gap: var(--s3);
    padding: var(--s3);
    border-radius: var(--r2);
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .card:hover {
    background: var(--tint-1);
  }

  .icon {
    display: block;
    width: 46px;
    height: 46px;
    flex: none;
    overflow: hidden;
    border-radius: var(--r1);
    background: var(--tint-1);
  }

  .icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 描述压到两行：卡片高度一致，网格才立得住。 */
  .desc {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.5;
  }

  .meta {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .hint,
  .count {
    margin: var(--s4) 0 0;
  }

  .count {
    padding-top: var(--s3);
  }
</style>
