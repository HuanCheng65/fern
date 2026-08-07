<script lang="ts">
  /**
   * 补给站——一个独立的、找东西的地方。
   *
   * 上一版把搜索结果按当前实例的版本和加载器过滤掉了。那让它成了「给这个实例
   * 装东西」的附属品：没有实例就什么都看不到，而且永远看不见「这个模组还没
   * 适配你这个版本」这个事实——一个空列表说不出这句话。
   *
   * 现在筛选条件是明确的控件，默认全不限；「装到哪个实例」是另一件事，摆在
   * 旁边，只在真的要装的时候才用得上。装不装得上是版本上的标注。
   *
   * 只做模组、资源包、光影：这三种是「下一个文件放进一个目录」就完事的。
   * 整合包要建实例，那是另一条路径，没做就不摆在这里。
   */
  import { Search } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import ProjectDetailView from './ProjectDetail.svelte'
  import { instances } from '../lib/instances.svelte'
  import { nav } from '../lib/nav.svelte'
  import { compactNumber, KINDS, LOADER_FILTERS, SORTS, supply } from '../lib/supply.svelte'

  /** 版本筛选只给正式版，快照在补给站里几乎没人找。 */
  const releases = $derived(
    instances.versions.filter((item) => item.kind === 'release').slice(0, 60),
  )

  // 从实例的模组页跳过来时带着实例 id，直接把条件对准它。用掉就从地址里去掉，
  // 否则用户随后改了「安装到」，下一次求值又会被它盖回去。
  $effect(() => {
    const aim = nav.params.forInstance
    if (!aim) return
    supply.aimAt(aim)
    nav.consume('forInstance')
  })

  // 进来就给一屏，空着一片白比什么都不给更糟。
  $effect(() => {
    if (!supply.loaded && !supply.searching) void supply.search()
  })

  void instances.loadVersions()
</script>

{#if nav.detail}
  <ProjectDetailView slug={nav.detail} />
{:else}
  <section class="supply">
    <header class="bar">
      <div class="field">
        <Search size={15} strokeWidth={1.9} />
        <input
          class="input"
          bind:value={supply.query}
          spellcheck="false"
          placeholder="搜索 Modrinth"
          onkeydown={(event) => event.key === 'Enter' && supply.refresh()}
        />
      </div>

      <div class="kinds">
        {#each KINDS as item (item.id)}
          <button
            class="chip"
            class:on={supply.kind === item.id}
            onclick={() => {
              supply.kind = item.id
              supply.refresh()
            }}
          >
            {item.label}
          </button>
        {/each}
      </div>
    </header>

    <div class="filters">
      <label class="pick">
        <span class="t-quiet">游戏版本</span>
        <select
          class="select"
          bind:value={supply.gameVersion}
          onchange={() => supply.refresh()}
        >
          <option value="">不限</option>
          {#each releases as version (version.id)}
            <option value={version.id}>{version.id}</option>
          {/each}
        </select>
      </label>

      <!-- 资源包和光影没有加载器这个概念，摆一个选了没用的控件是噪音。 -->
      {#if supply.kind === 'mod'}
        <label class="pick">
          <span class="t-quiet">加载器</span>
          <select class="select" bind:value={supply.loader} onchange={() => supply.refresh()}>
            <option value="">不限</option>
            {#each LOADER_FILTERS as item (item.id)}
              <option value={item.id}>{item.label}</option>
            {/each}
          </select>
        </label>
      {/if}

      <label class="pick">
        <span class="t-quiet">排序</span>
        <select class="select" bind:value={supply.sort} onchange={() => supply.refresh()}>
          {#each SORTS as item (item.id)}
            <option value={item.id}>{item.label}</option>
          {/each}
        </select>
      </label>

      <!-- 装到哪，是上下文不是筛选，所以推到最右边并且和左边隔开。 -->
      {#if instances.list.length > 0}
        <label class="pick target">
          <span class="t-quiet">安装到</span>
          <select class="select" bind:value={supply.targetId}>
            {#each instances.recent as item (item.id)}
              <option value={item.id}>{item.name}</option>
            {/each}
          </select>
        </label>
      {/if}
    </div>

    {#if supply.error}
      <div class="alert">{supply.error}</div>
    {:else if supply.searching && supply.hits.length === 0}
      <p class="t-quiet hint">搜索中</p>
    {:else if supply.hits.length === 0}
      <p class="t-quiet hint">没有匹配的结果。</p>
    {:else}
      <div class="grid scroll">
        {#each supply.hits as hit (hit.projectId)}
          <button class="card" onclick={() => nav.open(hit.slug)}>
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
              <small class="t-mono meta">{compactNumber(hit.downloads)} · {hit.author}</small>
            </span>
          </button>
        {/each}
      </div>

      <div class="foot">
        <span class="t-quiet">共 {supply.total} 个结果，已显示 {supply.hits.length} 个</span>
        {#if supply.canLoadMore}
          <button class="btn btn--link" disabled={supply.searching} onclick={() => supply.more()}>
            {supply.searching ? '加载中' : '加载更多'}
          </button>
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .supply {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--s3);
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

  .kinds {
    display: flex;
    gap: var(--s2);
  }

  .chip {
    padding: 5px var(--s3);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-small);
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .chip:hover {
    color: var(--ink-2);
    background: var(--tint-2);
  }

  .chip.on {
    background: var(--accent);
    color: var(--on-accent);
  }

  .filters {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--s2) var(--s5);
    padding: var(--s4) 0;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  /* 「安装到」不是筛选，推到另一头去。 */
  .target {
    margin-left: auto;
  }

  /* 列数跟着窗口走，不写断点。 */
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

  .hint {
    margin: var(--s4) 0 0;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding-top: var(--s3);
  }
</style>
