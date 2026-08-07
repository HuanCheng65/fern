<script lang="ts">
  /**
   * 项目详情——补给场景向内的那一级。
   *
   * 页面本身回答三个问题：这是什么、长什么样、装哪个版本。
   *
   * **版本一个都不藏。** 装不上的标出原因而不是过滤掉——「还没适配 1.21」是
   * 用户需要知道的事实，一个空列表说不出这句话。而且版本不对和加载器不对分开
   * 报告：前者要等作者更新，后者是选错了实例，应对方式不同。
   *
   * **不渲染介绍正文。** Modrinth 的 body 是一整篇 markdown，渲染它要么引一个
   * 解析器加一层消毒，要么自己写一个——把网络来的字符串变成 DOM 是 XSS 面，
   * 不值得为一段介绍开这个口。正文交给「在 Modrinth 打开」。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ArrowUpRight, Check, Download } from 'lucide-svelte'
  import Cover from '../components/Cover.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'
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
        : 'mod',
  )
  /** 装不上的不藏，但默认收在「显示全部版本」后面——多数人要的是能装的那个。 */
  const judged = $derived(
    versions.map((version) => ({
      version,
      fit: compatibility(version, target, kind),
    })),
  )
  const fitting = $derived(judged.filter((item) => item.fit.ok))
  const shown = $derived(showAll ? judged : fitting.slice(0, 12))
  const hiddenCount = $derived(judged.length - shown.length)

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
      supply.viewingTitle = project.title
      error = ''
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  async function install(version: ProjectVersion) {
    if (!target) return
    installing = version.id
    error = ''
    try {
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

<section class="project scroll">
  {#if loading}
    <p class="t-quiet hint">读取中</p>
  {:else if !detail}
    <div class="alert">{error || '读不到这个项目。'}</div>
  {:else}
    <header class="head">
      <span class="icon">
        {#if detail.iconUrl}
          <img src={detail.iconUrl} alt="" />
        {:else}
          <Cover seed={detail.slug} quality={0.5} />
        {/if}
      </span>
      <div class="titles">
        <h1 class="t-h1">{detail.title}</h1>
        <p class="summary">{detail.description}</p>
        <p class="t-mono facts">
          {compactNumber(detail.downloads)} 次下载 · {compactNumber(detail.followers)} 关注
          {#if detail.license}· {detail.license}{/if}
          · 更新于 {day(detail.updated)}
        </p>
      </div>
    </header>

    {#if detail.categories.length > 0}
      <div class="tags">
        {#each detail.categories as tag (tag)}<span class="tag">{tag}</span>{/each}
      </div>
    {/if}

    {#if detail.gallery.length > 0}
      <div class="gallery scroll">
        {#each detail.gallery as image (image.url)}
          <figure>
            <img src={image.url} alt={image.title} loading="lazy" />
            {#if image.title}<figcaption class="t-quiet">{image.title}</figcaption>{/if}
          </figure>
        {/each}
      </div>
    {/if}

    <div class="links">
      {#each detail.links as link (link.url)}
        <button class="btn btn--link" onclick={() => void invoke('open_external', { url: link.url })}>
          {link.label}<ArrowUpRight size={13} strokeWidth={1.9} />
        </button>
      {/each}
    </div>

    <section class="versions">
      <div class="v-head">
        <span class="label">版本</span>
        {#if target}
          <span class="t-quiet">
            安装到 <strong>{target.name}</strong> · {target.gameVersion} · {target.loader}
          </span>
        {:else}
          <span class="t-quiet">还没有实例，创建一个才能安装</span>
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
        <p class="t-quiet hint">这个项目还没有发布任何版本。</p>
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
                disabled={!target || !fit.ok || installing !== ''}
                title={fit.ok ? '安装' : fit.note}
                onclick={() => void install(version)}
              >
                {#if installing === version.id}
                  安装中
                {:else}
                  <Download size={14} strokeWidth={1.9} />安装
                {/if}
              </button>
            </li>
          {/each}
        </ul>

        {#if hiddenCount > 0 || showAll}
          <button class="btn btn--link" onclick={() => (showAll = !showAll)}>
            {showAll ? '只看装得上的' : `显示全部 ${judged.length} 个版本`}
          </button>
        {/if}
        {#if fitting.length === 0 && !showAll}
          <p class="t-quiet hint">没有适用于这个实例的版本。展开可以看全部。</p>
        {/if}
      {/if}
    </section>

    {#if error}<div class="alert">{error}</div>{/if}
  {/if}
</section>

<style>
  .project {
    height: 100%;
    min-height: 0;
    padding-right: var(--s2);
  }

  .hint {
    margin: var(--s4) 0;
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--s4);
    padding-bottom: var(--s4);
  }

  .icon {
    display: block;
    width: 76px;
    height: 76px;
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

  .titles {
    min-width: 0;
  }

  .titles h1 {
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

  .facts {
    margin: var(--s3) 0 0;
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    padding-bottom: var(--s4);
  }

  .tag {
    padding: 3px var(--s2);
    border-radius: 999px;
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  /* 图库横向滚：竖着铺会把版本列表推到屏幕外面去。 */
  .gallery {
    display: flex;
    gap: var(--s3);
    overflow-x: auto;
    overflow-y: hidden;
    padding-bottom: var(--s3);
  }

  .gallery figure {
    flex: none;
    width: min(300px, 68vw);
    margin: 0;
  }

  .gallery img {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: var(--r2);
    background: var(--tint-1);
  }

  .gallery figcaption {
    margin-top: 6px;
    overflow: hidden;
    font-size: var(--t-micro);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s4);
    padding: var(--s3) 0 var(--s5);
  }

  .links .btn {
    gap: 4px;
    color: var(--ink-3);
  }

  .links .btn:hover {
    color: var(--ink);
  }

  .versions {
    padding-top: var(--s4);
    padding-bottom: var(--s6);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }

  .v-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--s3);
  }

  .label {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .v-head strong {
    color: var(--ink-2);
    font-weight: 500;
  }

  .list {
    display: grid;
    gap: 1px;
    margin: var(--s3) 0;
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
    padding: var(--s3) 0;
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

  .done p:last-child {
    margin: 0;
  }
</style>
