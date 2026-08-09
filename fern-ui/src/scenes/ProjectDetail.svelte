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
  import { ArrowUpRight, Download, Plus } from 'lucide-svelte'
  import Loading from '../components/Loading.svelte'
  import Detail from '../layouts/Detail.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'
  import { jobs } from '../lib/jobs.svelte'
  import { nav } from '../lib/nav.svelte'
  import { notices } from '../lib/notices.svelte'
  import {
    compactNumber,
    compatibility,
    supply,
    type InstallOutcome,
    type InstallPlan,
    type ProjectDetail,
    type ProjectVersion,
    type ResourceKind,
  } from '../lib/supply.svelte'
  import Button from 'fern-kit/ui/Button.svelte'
  import Select from 'fern-kit/ui/Select.svelte'

  interface Props {
    slug: string
  }

  let { slug }: Props = $props()

  let detail = $state<ProjectDetail | null>(null)
  let versions = $state<ProjectVersion[]>([])
  let loading = $state(true)
  let error = $state('')
  /**
   * 点的是哪一行。纯粹是本地的高亮，不是「装没装完」——那个由作业说。
   *
   * 上一版把安装状态整个存在这里，于是导航一走组件销毁，任务还在后台跑，回来
   * 看不到任何痕迹；失败了错误也随组件一起蒸发了。
   */
  let clicked = $state('')
  let showAll = $state(false)
  /**
   * 展开看前置的那一行，以及它的计划。
   *
   * 计划要联网算（要问上游每个依赖是什么，还要把实例里已有的模组哈希一遍），
   * 所以不给每一行都算——只算被展开的那一个。
   */
  let opened = $state('')
  let plan = $state<InstallPlan | null>(null)
  let planning = $state(false)

  /** 这个项目上现在有没有事情在跑。走开再回来它还在。 */
  const job = $derived(jobs.forSubject(slug))

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

  /** 数一数这个版本声明了几个必需前置。只要一个数，不用联网。 */
  const required = (version: ProjectVersion) =>
    version.dependencies.filter((item) => item.kind === 'required').length

  /** 展开／收起某一行的前置。展开时才去算计划。 */
  async function inspect(version: ProjectVersion) {
    if (opened === version.id) {
      opened = ''
      return
    }
    opened = version.id
    plan = null
    await loadPlan(version)
  }

  async function loadPlan(version: ProjectVersion) {
    if (!target || !inTauri()) return
    planning = true
    try {
      const result = await invoke<InstallPlan>('install_plan', {
        instanceId: target.id,
        versionId: version.id,
        kind,
      })
      // 等回来的时候用户可能已经点开了另一行。
      if (opened === version.id) plan = result
    } catch (cause) {
      if (opened === version.id) error = String(cause)
    } finally {
      planning = false
    }
  }

  const STATE_LABEL: Record<string, string> = {
    satisfied: '已安装',
    disabled: '已安装但已禁用',
    mismatched: '已安装的版本不适用',
    planned: '将一并安装',
    unavailable: '没有适用版本',
    conflicting: '与已安装的冲突',
  }

  async function install(version: ProjectVersion) {
    clicked = version.id
    error = ''
    try {
      if (isPack) {
        const created = await invoke<{ id: string; name: string }>('install_modpack', {
          versionId: version.id,
          name: detail?.title ?? null,
          title: `安装 ${detail?.title ?? '整合包'}`,
          subjects: [slug],
        })
        await instances.load()
        instances.select(created.id)
        // 建完直接去看它，而不是留在这一页让人自己找。
        nav.enter('instances', created.id)
        return
      }
      if (!target) return
      const where = target
      // 这件事既属于这个项目，也属于那个实例——两边的页面都该看得见它。
      const outcome = await invoke<InstallOutcome>('install_from_modrinth', {
        instanceId: where.id,
        versionId: version.id,
        kind,
        title: `安装 ${detail?.title ?? '资源'}`,
        subjects: [slug, where.id],
      })
      // 结果不留在这一页：它是一件已经做完的事，而用户下一秒可能就走了。
      const extra = outcome.installed.length - 1
      notices.say({
        title: `已安装 ${outcome.installed[0] ?? detail?.title ?? '资源'}`,
        detail: [
          extra > 0 ? `连同 ${outcome.installed.slice(1).join('、')}` : '',
          outcome.reused.length > 0 ? `${outcome.reused.join('、')}已经有了，未重复安装` : '',
        ]
          .filter(Boolean)
          .join('。'),
        action: {
          label: `在 ${where.name} 中查看`,
          run: () => {
            instances.select(where.id)
            nav.enter('instances', where.id)
          },
        },
      })
      // 装完之后计划就过期了：刚装上的那些，现在是「已经有了」。
      if (opened === version.id) void loadPlan(version)
    } catch (cause) {
      error = String(cause)
    } finally {
      clicked = ''
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
  <Detail
    tabs={TABS}
    {tab}
    ontab={(id) => nav.setTab(id)}
    showBanner={detail.gallery.length > 0}
  >
    {#snippet banner()}
      {#if detail?.gallery[0]}
        <img class="shot" src={detail.gallery[0].url} alt="" />
      {/if}
      <div class="fade"></div>
    {/snippet}

    {#snippet head()}
      <div class="titles">
        {#if detail?.iconUrl}
          <span class="icon">
            <img src={detail.iconUrl} alt="" />
          </span>
        {/if}
        <div class="words">
          <h1 class="t-h1">{detail?.title}</h1>
          <p class="summary">{detail?.description}</p>
        </div>
      </div>
    {/snippet}

    {#snippet compactHead()}
      <span class="mini-title">{detail?.title}</span>
    {/snippet}

    {#if tab === 'versions'}
      <div class="v-head">
        {#if isPack}
          <p class="note">整合包自带游戏版本与加载器。选一个版本，它会建成一个新实例。</p>
        {:else if instances.list.length === 0}
          <p class="note">还没有实例。先创建一个，才有地方安装。</p>
        {:else}
          <!--
            装到哪，在这一页也能改——装东西这个动作发生在这里。
            值取自实际生效的目标而不是 targetId：没选过时它是空的，绑上去
            下拉框会显示一片空白，而实际上装的是当前实例。
          -->
          <label class="target">
            <span class="t-quiet">安装到</span>
            <div class="picker">
              <Select
                label="安装到"
                value={target?.id ?? ''}
                options={instances.recent.map((item) => ({
                  value: item.id,
                  label: `${item.name}（${item.gameVersion} · ${item.loader}）`,
                }))}
                onchange={(id) => (supply.targetId = id)}
              />
            </div>
          </label>
          <span class="t-quiet">
            {judged.length} 个版本中 {fitting.length} 个装得上
          </span>
        {/if}
      </div>

      {#if versions.length === 0}
        <p class="t-quiet">这个项目还没有发布任何版本。</p>
      {:else}
        <ul class="list">
          {#each shown as { version, fit } (version.id)}
            <li class="row" class:off={!fit.ok}>
              <div class="line">
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

                <!--
                  前置在装之前就该看得见。展开才去算：算一份计划要问上游每个
                  依赖是什么，再把实例里已有的模组哈希一遍，不该为了列表里
                  二十行都做一遍。
                -->
                {#if !isPack && required(version) > 0}
                  <Button
                    variant="link"
                    class="deps"
                    aria-expanded={opened === version.id}
                    onclick={() => void inspect(version)}>
                    {required(version)} 个前置
                  </Button>
                {/if}

                <Button
                  variant="ghost"
                  disabled={(!isPack && !target) || !fit.ok || job !== undefined}
                  title={fit.ok ? (isPack ? '建成新实例' : '安装') : fit.note}
                  onclick={() => void install(version)}>
                  {#if job && clicked === version.id}
                    {job.stage || (isPack ? '创建中' : '安装中')}
                  {:else if job}
                    等待中
                  {:else if isPack}
                    <Plus size={14} strokeWidth={2} />创建实例
                  {:else}
                    <Download size={14} strokeWidth={1.9} />安装
                  {/if}
                </Button>
              </div>

              {#if opened === version.id}
                <div class="plan">
                  {#if planning && !plan}
                    <p class="t-quiet">正在核对这个实例里已经有什么…</p>
                  {:else if !target}
                    <p class="t-quiet">先选一个安装目标，才能知道哪些前置已经有了。</p>
                  {:else if plan}
                    <ul class="reqs">
                      {#each plan.requirements as item (item.projectId)}
                        <li class="req {item.state}">
                          <span class="dot"></span>
                          <span class="who">
                            {item.title}
                            {#if item.versionNumber}<small class="t-mono">{item.versionNumber}</small
                              >{/if}
                          </span>
                          <span class="state">
                            {STATE_LABEL[item.state] ?? item.state}{item.kind === 'optional'
                              ? '（可选）'
                              : ''}
                          </span>
                        </li>
                      {/each}
                    </ul>
                    <!--
                      「这次会下几个文件」和「已经有几个」都要说。只说前者，
                      用户看不出去重发生过；只说后者，他不知道要等多久。
                    -->
                    <p class="t-quiet tally">
                      这次会下载 {plan.files.length} 个文件{plan.requirements.filter(
                        (item) => item.state === 'satisfied',
                      ).length > 0
                        ? `，${plan.requirements.filter((item) => item.state === 'satisfied').length} 个前置已经有了`
                        : ''}。
                    </p>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>

        {#if fitting.length === 0 && !showAll && !isPack}
          <p class="t-quiet">没有适用于这个实例的版本。</p>
        {/if}
        {#if judged.length > shown.length || showAll}
          <Button variant="link" onclick={() => (showAll = !showAll)}>
            {showAll ? '只看装得上的' : `显示全部 ${judged.length} 个版本`}
          </Button>
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
          <Button variant="link" tone="quiet" onclick={() => void invoke('open_external', { url: link.url })}>
            {link.label}<ArrowUpRight size={13} strokeWidth={1.9} />
          </Button>
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
    padding-bottom: var(--s4);
  }

  .note {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  /* 有标签的字段。一个光秃秃的下拉框谁也不知道它在问什么。 */
  .target {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  .target .picker {
    min-width: 200px;
  }

  .mini-title {
    color: var(--ink-2);
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

  .list {
    display: grid;
    gap: 1px;
    margin: 0 0 var(--s3);
    padding: 0;
    list-style: none;
  }

  .row {
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .deps {
    flex: none;
    margin-left: auto;
    color: var(--ink-3);
  }

  .deps:hover {
    color: var(--ink);
  }

  /* 展开的那一块缩进对齐版本号，读起来才是「属于这一行」。 */
  .plan {
    padding: var(--s3) 0 var(--s2);
  }

  .reqs {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .req {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    font-size: var(--t-small);
  }

  /* 状态先用一个点说，再用文字说。扫一眼够了的时候不用读字。 */
  .req .dot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--ink-4);
  }

  .req.satisfied .dot {
    background: var(--accent);
  }

  .req.unavailable .dot,
  .req.conflicting .dot,
  .req.mismatched .dot,
  .req.disabled .dot {
    background: var(--danger);
  }

  .req .who {
    color: var(--ink-2);
  }

  .req .who small {
    margin-left: 4px;
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .req .state {
    margin-left: auto;
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .req.unavailable .state,
  .req.conflicting .state {
    color: var(--danger);
  }

  .tally {
    margin: var(--s3) 0 0;
    font-size: var(--t-micro);
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
</style>
