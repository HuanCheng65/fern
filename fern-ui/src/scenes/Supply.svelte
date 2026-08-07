<script lang="ts">
  /**
   * 补给站——一个独立的、找东西的地方。用浏览布局（docs/UI_DESIGN.md 十）。
   *
   * 上一版把搜索结果按当前实例的版本和加载器过滤掉了。那让它成了「给这个实例
   * 装东西」的附属品：没有实例就什么都看不到，而且永远看不见「这个模组还没
   * 适配你这个版本」这个事实——一个空列表说不出这句话。
   *
   * 筛选在左栏纵向排布，不是顶部一排 chip：**横向换地方，纵向改视图**。资源
   * 类型、游戏版本、加载器改的都是同一批东西的呈现，不是「去别处」。
   *
   * 结果无限滚动，翻到哪、搜的什么、筛选了什么全在 store 里——点进一个项目
   * 再返回，接着看，不用重新往下滑一遍。
   */
  import { Check, Search } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import FilterGroup from '../components/FilterGroup.svelte'
  import Loading from '../components/Loading.svelte'
  import Browse from '../layouts/Browse.svelte'
  import ProjectDetailView from './ProjectDetail.svelte'
  import { instances } from '../lib/instances.svelte'
  import { expand, riseIn } from '../lib/motion'
  import { nav } from '../lib/nav.svelte'
  import { compactNumber, KINDS, LOADER_FILTERS, SORTS, supply } from '../lib/supply.svelte'

  let results = $state<HTMLElement>()
  let sentinel = $state<HTMLElement>()

  let versionQuery = $state('')
  /**
   * 快照默认不出现。
   *
   * Mojang 的版本清单里快照占了绝大多数，混在一起就是几百条 `24w14a` 淹掉
   * 十几个正式版——而来补给站找模组的人九成九在玩正式版。
   */
  let snapshots = $state(false)

  /** 搜索框负责在这张几百条的清单里定位版本号。 */
  const versions = $derived(
    instances.versions
      .filter((item) => snapshots || item.kind === 'release')
      .filter((item) => item.id.toLowerCase().includes(versionQuery.trim().toLowerCase()))
      .map((item) => ({ id: item.id, label: item.id })),
  )

  /**
   * 关掉快照时，如果当前正按着一个快照筛，就把它一起放掉。
   *
   * 否则会留下一个看不见的筛选条件：列表里找不到选中的那一条，结果却还被它
   * 限制着——用户只会觉得补给站坏了。
   */
  function toggleSnapshots() {
    snapshots = !snapshots
    if (snapshots) return
    const picked = instances.versions.find((item) => item.id === supply.gameVersion)
    if (picked && picked.kind !== 'release') {
      supply.gameVersion = ''
      supply.refresh()
    }
  }

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

  // 回到列表时把滚动位置放回去。等一帧，让结果先铺出来。
  $effect(() => {
    if (nav.detail || !results) return
    const node = results
    requestAnimationFrame(() => (node.scrollTop = supply.scrollTop))
  })

  /**
   * 无限滚动。用 IntersectionObserver 而不是监听 scroll：哨兵进入视野才问，
   * 滚动过程中一次计算都不用做。
   */
  $effect(() => {
    if (!sentinel || !results) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) supply.more()
      },
      { root: results, rootMargin: '400px' },
    )
    observer.observe(sentinel)
    return () => observer.disconnect()
  })

  function open(slug: string, title: string) {
    supply.scrollTop = results?.scrollTop ?? 0
    // 点的那一刻就知道叫什么，面包屑不必等详情加载完。
    supply.beginViewing(title)
    nav.open(slug)
  }

  void instances.loadVersions()
</script>

{#if nav.detail}
  <div class="depth" in:expand>
    <ProjectDetailView slug={nav.detail} />
  </div>
{:else}
  <Browse>
    {#snippet search()}
      <div class="field">
        <Search class="glass" size={16} strokeWidth={1.9} />
        <input
          class="input"
          bind:value={supply.query}
          spellcheck="false"
          placeholder="搜索 Modrinth"
          onkeydown={(event) => event.key === 'Enter' && supply.refresh()}
        />
      </div>
    {/snippet}

    {#snippet filters()}
      <FilterGroup
        label="类型"
        value={supply.kind}
        options={KINDS.map((item) => ({ id: item.id, label: item.label }))}
        onchange={(value) => {
          supply.kind = value as typeof supply.kind
          supply.refresh()
        }}
      />

      <FilterGroup
        label="游戏版本"
        value={supply.gameVersion}
        options={versions}
        anyLabel="全部"
        scrolls
        onchange={(value) => {
          supply.gameVersion = value
          supply.refresh()
        }}
      >
        {#snippet aside()}
          <!--
            这是个开关，不是又一个标题——所以给它一个方框。第三级的其他控件靠
            透明度区分选中，那套语言只说得清「N 选 1」；一个布尔值光靠深浅，
            用户根本不知道它可以点。
          -->
          <button
            class="snap"
            class:on={snapshots}
            aria-pressed={snapshots}
            onclick={toggleSnapshots}
          >
            <span class="box">
              {#if snapshots}<Check size={9} strokeWidth={3.4} />{/if}
            </span>
            含快照
          </button>
        {/snippet}

        {#snippet control()}
          <div class="version-search">
            <Search size={13} strokeWidth={1.8} />
            <input
              class="input"
              bind:value={versionQuery}
              spellcheck="false"
              aria-label="搜索游戏版本"
              placeholder="搜索版本号"
            />
          </div>
        {/snippet}
      </FilterGroup>

      <!-- 资源包和光影没有加载器这个概念，摆一组选了没用的选项是噪音。 -->
      {#if supply.kind === 'mod' || supply.kind === 'modpack'}
        <FilterGroup
          label="加载器"
          value={supply.loader}
          options={LOADER_FILTERS}
          anyLabel="全部"
          onchange={(value) => {
            supply.loader = value
            supply.refresh()
          }}
        />
      {/if}
    {/snippet}

    <div class="results" bind:this={results}>
      {#if supply.error}
        <div class="alert">{supply.error}</div>
      {:else if supply.searching && supply.hits.length === 0}
        <Loading note="搜索中" fill />
      {:else if supply.hits.length === 0}
        <p class="t-quiet hint">没有匹配的结果。</p>
      {:else}
        <!--
          排序不是筛选条件——它不删掉任何东西，只换个顺序，所以跟着结果走
          而不是待在左栏那一组条件里。
        -->
        <div class="results-head">
          <span class="t-quiet">{supply.total} 个结果</span>
          <label class="sort">
            <span class="t-quiet">排序</span>
            <select class="bare" bind:value={supply.sort} onchange={() => supply.refresh()}>
              {#each SORTS as item (item.id)}
                <option value={item.id}>{item.label}</option>
              {/each}
            </select>
          </label>
        </div>

        <div class="grid">
          {#each supply.hits as hit, index (hit.projectId)}
            <button class="card" onclick={() => open(hit.slug, hit.title)} in:riseIn={{ index }}>
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

        <div class="foot" bind:this={sentinel}>
          {#if supply.searching}
            <span class="t-quiet">加载中</span>
          {:else if supply.canLoadMore}
            <span class="t-quiet">已显示 {supply.hits.length} / {supply.total}</span>
          {:else}
            <span class="t-quiet">到底了</span>
          {/if}
        </div>
      {/if}
    </div>
  </Browse>
{/if}

<style>
  .depth {
    height: 100%;
    min-height: 0;
  }

  /*
   * 搜索是浏览型页面的核心动作，是这套布局唯一允许的重型控件——所以它是一个
   * 真的输入框，不是一条浮着的文字。放大镜在框里面，不在框旁边。
   */
  .field {
    position: relative;
    width: min(520px, 100%);
  }

  .field :global(.glass) {
    position: absolute;
    top: 50%;
    left: var(--s3);
    color: var(--ink-4);
    transform: translateY(-50%);
    pointer-events: none;
  }

  .field .input {
    min-height: 44px;
    padding-left: calc(var(--s3) * 2 + 16px);
  }

  .results-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding-bottom: var(--s3);
  }

  .snap {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: none;
    padding: 0;
    color: var(--ink);
    font-size: var(--t-micro);
    /* 比旁边的标题（0.35）亮一档：它是控件，不是说明。 */
    opacity: 0.6;
    transition: opacity var(--t-fast) var(--ease);
  }

  .snap:hover,
  .snap.on {
    opacity: 1;
  }

  .box {
    display: grid;
    place-items: center;
    width: 12px;
    height: 12px;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px var(--tint-3);
    color: var(--accent-ink);
    transition:
      background var(--t-fast) var(--ease),
      box-shadow var(--t-fast) var(--ease);
  }

  .snap.on .box {
    background: var(--accent);
    box-shadow: none;
  }

  .version-search {
    position: relative;
  }

  .version-search :global(svg) {
    position: absolute;
    top: 50%;
    left: var(--s2);
    color: var(--ink-4);
    transform: translateY(-50%);
    pointer-events: none;
  }

  .version-search .input {
    min-height: 30px;
    padding: 0 var(--s2) 0 28px;
    font-size: var(--t-small);
  }

  .sort {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  /* 排序在结果上方，重量要压住——一个带边框的 select 会盖过它下面的卡片。 */
  .bare {
    padding: 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    cursor: pointer;
  }

  .bare:hover {
    color: var(--ink);
  }

  /* 下拉里的选项由系统画，深色前景色在浅色菜单上读不出来。 */
  .bare option {
    color: #10171b;
    background: #dfe6e6;
  }

  /* 滚动容器就是结果区本身，哨兵和滚动记忆都挂在它身上。 */
  .results {
    height: 100%;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
    scrollbar-color: var(--tint-3) transparent;
    padding-right: var(--s2);
  }

  /* 列数跟着窗口走，不写断点。 */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--s2);
    align-content: start;
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
    margin: 0;
  }

  .foot {
    padding: var(--s5) 0 var(--s4);
    text-align: center;
  }
</style>
