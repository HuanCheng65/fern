<script lang="ts">
  /**
   * 项目详情——补给场景向内的那一级。用详情布局（docs/UI_DESIGN.md 十）。
   *
   * **版本一个都不藏。** 装不上的标出原因而不是过滤掉——「还没适配 1.21」是
   * 用户需要知道的事实，一个空列表说不出这句话。而且版本不对和加载器不对分开
   * 报告：前者要等作者更新，后者是选错了实例，应对方式不同。
   *
   * 「安装到哪个实例」在这一页也能改。装东西这个动作发生在这里，决定装给谁的
   * 控件却留在上一页，等于逼人退回去改完再进来。
   *
   * **不渲染介绍正文。** Modrinth 的 body 是一整篇 markdown，渲染它要么引一个
   * 解析器加一层消毒，要么自己写一个——把网络来的字符串变成 DOM 是 XSS 面，
   * 不值得为一段介绍开这个口。正文交给「在 Modrinth 打开」。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ArrowUpRight, Check, Download, Plus } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import Loading from '../components/Loading.svelte'
  import Detail from '../layouts/Detail.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'
  import { nav } from '../lib/nav.svelte'
  import {
    compactNumber,
    compatibility,
    supply,
    type ProjectDetail,
    type ProjectVersion,
    type ResourceKind,
  } from '../lib/supply.svelte'

  interface Props {
    slug: string
  }

  let { slug }: Props = $props()

  let detail = $state<ProjectDetail | null>(null)
  let versions = $state<ProjectVersion[]>([])
  let loading = $state(true)
  let error = $state('')
  let installing = $state('')
  let installed = $state<string[]>([])
  let showAll = $state(false)

  const TABS = [
    { id: 'versions', label: '版本' },
    { id: 'about', label: '关于', reading: true },
  ]
  const tab = $derived(TABS.some((item) => item.id === nav.tab) ? nav.tab : 'versions')

  const target = $derived(supply.target)
  /**
   * 装到哪个目录由**项目自己**说了算，不是看筛选栏现在选着什么——从地址直接
   * 打开一个光影时，筛选栏可能还停在「模组」上，那会把 zip 丢进 mods/。
   */
  const kind = $derived<ResourceKind>(
    detail?.projectType === 'resourcepack'
      ? 'resource_pack'
      : detail?.projectType === 'shader'
        ? 'shader'
        : detail?.projectType === 'modpack'
          ? 'modpack'
          : 'mod',
  )
  /**
   * 整合包是另一条路径：它自带游戏版本和加载器，装它就是**建一个新实例**。
   * 往一个已有实例上盖只会得到一个谁也说不清是什么的混合体。
   */
  const isPack = $derived(kind === 'modpack')

  const judged = $derived(
    versions.map((version) => ({ version, fit: compatibility(version, target, kind) })),
  )
  const fitting = $derived(judged.filter((item) => item.fit.ok))
  /** 装不上的不藏，但默认收在展开后面——多数人要的是能装的那个。 */
  const shown = $derived(showAll ? judged : fitting.slice(0, 20))

  const day = (iso: string) => iso.slice(0, 10)

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    loading = true
    try {
      // 两个请求各自独立，一起发。
      const [project, list] = await Promise.all([
        invoke<ProjectDetail>('project_detail', { project: slug }),
        invoke<ProjectVersion[]>('project_versions', { project: slug }),
      ])
      detail = project
      versions = list
      supply.beginViewing(project.title)
      error = ''
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  async function install(version: ProjectVersion) {
    installing = version.id
    error = ''
    try {
      if (isPack) {
        const created = await invoke<{ id: string; name: string }>('install_modpack', {
          versionId: version.id,
          name: detail?.title ?? null,
        })
        await instances.load()
        instances.select(created.id)
        // 建完直接去看它，而不是留在这一页让人自己找。
        nav.enter('instances', created.id)
        return
      }
      if (!target) return
      installed = await invoke<string[]>('install_from_modrinth', {
        instanceId: target.id,
        versionId: version.id,
        kind,
      })
    } catch (cause) {
      error = String(cause)
    } finally {
      installing = ''
    }
  }

  $effect(() => {
    slug
    void load()
  })
</script>

{#if loading}
  <Loading note="读取项目" fill />
{:else if !detail}
  <div class="alert pad">{error || '读不到这个项目。'}</div>
{:else}
  <Detail tabs={TABS} {tab} ontab={(id) => nav.setTab(id)}>
    {#snippet banner()}
      {#if detail?.gallery[0]}
        <img class="shot" src={detail.gallery[0].url} alt="" />
      {:else}
        <Cover seed={detail?.slug ?? slug} quality={0.6} />
      {/if}
      <div class="fade"></div>
    {/snippet}

    {#snippet head()}
      <div class="titles">
        <span class="icon">
          {#if detail?.iconUrl}
            <img src={detail.iconUrl} alt="" />
          {:else}
            <Cover seed={detail?.slug ?? slug} quality={0.5} />
          {/if}
        </span>
        <div class="words">
          <h1 class="t-h1">{detail?.title}</h1>
          <p class="summary">{detail?.description}</p>
        </div>
      </div>
    {/snippet}

    {#if tab === 'versions'}
      <div class="v-head">
        {#if isPack}
          <span class="t-quiet">选一个版本，它会建成一个新实例</span>
        {:else}
          <span class="t-quiet">{fitting.length} 个版本可以装进</span>
          <!-- 装到哪，在这一页也能改。 -->
          {#if instances.list.length > 0}
            <select class="select" bind:value={supply.targetId}>
              {#each instances.recent as item (item.id)}
                <option value={item.id}>{item.name} · {item.gameVersion} · {item.loader}</option>
              {/each}
            </select>
          {:else}
            <span class="t-quiet">还没有实例，创建一个才能安装</span>
          {/if}
        {/if}
      </div>

      {#if installed.length > 0}
        <div class="done">
          <p class="ok"><Check size={15} strokeWidth={2.4} />已安装 {installed.length} 个文件</p>
          <ul class="files t-mono">
            {#each installed as file (file)}<li>{file}</li>{/each}
          </ul>
          {#if installed.length > 1}
            <p class="t-quiet">其中包含自动解析的必需依赖。</p>
          {/if}
        </div>
      {/if}

      {#if versions.length === 0}
        <p class="t-quiet">这个项目还没有发布任何版本。</p>
      {:else}
        <ul class="list">
          {#each shown as { version, fit } (version.id)}
            <li class="row" class:off={!fit.ok}>
              <span class="text">
                <strong>{version.versionNumber}</strong>
                <small class="t-mono">
                  {day(version.datePublished)}
                  {#if version.versionType !== 'release'}
                    · <em class="pre">{version.versionType}</em>
                  {/if}
                  {#if fit.note}· {fit.note}{/if}
                </small>
              </span>
              <button
                class="btn btn--ghost"
                disabled={(!isPack && !target) || !fit.ok || installing !== ''}
                title={fit.ok ? (isPack ? '建成新实例' : '安装') : fit.note}
                onclick={() => void install(version)}
              >
                {#if installing === version.id}
                  {isPack ? '创建中' : '安装中'}
                {:else if isPack}
                  <Plus size={14} strokeWidth={2} />创建实例
                {:else}
                  <Download size={14} strokeWidth={1.9} />安装
                {/if}
              </button>
            </li>
          {/each}
        </ul>

        {#if fitting.length === 0 && !showAll && !isPack}
          <p class="t-quiet">没有适用于这个实例的版本。</p>
        {/if}
        {#if judged.length > shown.length || showAll}
          <button class="btn btn--link" onclick={() => (showAll = !showAll)}>
            {showAll ? '只看装得上的' : `显示全部 ${judged.length} 个版本`}
          </button>
        {/if}
      {/if}
    {:else}
      <dl class="facts">
        <div><dt>下载</dt><dd class="t-mono">{compactNumber(detail?.downloads ?? 0)}</dd></div>
        <div><dt>关注</dt><dd class="t-mono">{compactNumber(detail?.followers ?? 0)}</dd></div>
        <div><dt>更新</dt><dd class="t-mono">{day(detail?.updated ?? '')}</dd></div>
        {#if detail?.license}
          <div><dt>许可证</dt><dd class="t-mono">{detail.license}</dd></div>
        {/if}
      </dl>

      {#if detail && detail.categories.length > 0}
        <div class="tags">
          {#each detail.categories as name (name)}<span class="tag">{name}</span>{/each}
        </div>
      {/if}

      {#if detail && detail.gallery.length > 0}
        <div class="gallery">
          {#each detail.gallery as image (image.url)}
            <figure>
              <img src={image.url} alt={image.title} loading="lazy" />
              {#if image.title}<figcaption class="t-quiet">{image.title}</figcaption>{/if}
            </figure>
          {/each}
        </div>
      {/if}

      <div class="links">
        {#each detail?.links ?? [] as link (link.url)}
          <button
            class="btn btn--link"
            onclick={() => void invoke('open_external', { url: link.url })}
          >
            {link.label}<ArrowUpRight size={13} strokeWidth={1.9} />
          </button>
        {/each}
      </div>
    {/if}

    {#if error}<div class="alert">{error}</div>{/if}
  </Detail>
{/if}

<style>
  .pad {
    margin: var(--s5) 0;
  }

  .shot,
  .fade {
    position: absolute;
    inset: 0;
  }

  .shot {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* 底边化开，标题才不像压在一张图上。 */
  .fade {
    top: auto;
    height: 60%;
    background: linear-gradient(to bottom, transparent, rgba(6, 8, 10, 0.62));
    pointer-events: none;
  }

  .titles {
    display: flex;
    align-items: flex-start;
    gap: var(--s4);
    min-width: 0;
  }

  .icon {
    display: block;
    width: 64px;
    height: 64px;
    flex: none;
    overflow: hidden;
    border-radius: var(--r2);
    background: var(--tint-1);
  }

  .icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .words {
    min-width: 0;
  }

  .words h1 {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .summary {
    margin: var(--s2) 0 0;
    max-width: 72ch;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.6;
  }

  .v-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--s3);
    padding-bottom: var(--s3);
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: var(--s4);
    margin: 0 0 var(--s5);
  }

  .facts dt {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .facts dd {
    margin: 4px 0 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    overflow-wrap: anywhere;
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    padding-bottom: var(--s5);
  }

  .tag {
    padding: 3px var(--s2);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .gallery {
    display: grid;
    gap: var(--s4);
    padding-bottom: var(--s5);
  }

  .gallery figure {
    margin: 0;
  }

  .gallery img {
    display: block;
    width: 100%;
    border-radius: var(--r2);
    background: var(--tint-1);
  }

  .gallery figcaption {
    margin-top: 6px;
    font-size: var(--t-micro);
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s4);
  }

  .links .btn {
    gap: 4px;
    color: var(--ink-3);
  }

  .links .btn:hover {
    color: var(--ink);
  }

  .list {
    display: grid;
    gap: 1px;
    margin: 0 0 var(--s3);
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row:last-child {
    box-shadow: none;
  }

  /* 装不上的压暗但留着：这条信息本身就是答案。 */
  .row.off {
    opacity: 0.5;
  }

  .text {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  /* 预览版标一下就够，不用警告色——作者标了它就是给人试的。 */
  .pre {
    font-style: normal;
    color: var(--ink-3);
  }

  .done {
    padding-bottom: var(--s4);
  }

  .ok {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin: 0;
    color: var(--ink);
    font-size: var(--t-body);
  }

  .ok :global(svg) {
    color: var(--accent);
  }

  .files {
    margin: var(--s2) 0;
    padding: 0;
    list-style: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    line-height: 1.7;
    overflow-wrap: anywhere;
  }
</style>
