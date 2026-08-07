/**
 * 补给站的浏览状态。
 *
 * 放在 store 里而不是组件里，因为它要跨一级纵深活着：点进一个项目再返回，
 * 搜索词、筛选和滚动位置都该还在——否则「翻一翻、点进去看看、回来接着翻」
 * 这条最常见的路径每次都要从头开始。
 *
 * **筛选条件和「装到哪」是两件事。** 前者决定列表里有什么，后者决定装到哪个
 * 实例去。上一版把两者合成了一个——按当前实例过滤——于是想看看有什么就得先
 * 有一个实例，而且看不到任何「这个模组还没适配你这个版本」的事实。现在装不
 * 装得上是版本列表上的**标注**。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri, instances } from './instances.svelte'

/** 只有这三种是「下一个文件放进一个目录」就完事的，所以只做这三种。 */
export type ResourceKind = 'mod' | 'resource_pack' | 'shader'

export const KINDS: { id: ResourceKind; label: string }[] = [
  { id: 'mod', label: '模组' },
  { id: 'resource_pack', label: '资源包' },
  { id: 'shader', label: '光影' },
]

export const SORTS: { id: string; label: string }[] = [
  { id: 'relevance', label: '相关度' },
  { id: 'downloads', label: '下载量' },
  { id: 'follows', label: '关注数' },
  { id: 'updated', label: '最近更新' },
  { id: 'newest', label: '最新发布' },
]

export interface Hit {
  projectId: string
  slug: string
  title: string
  description: string
  author: string
  downloads: number
  iconUrl?: string
  categories: string[]
}

export interface GalleryImage {
  url: string
  title: string
}

export interface ProjectLink {
  label: string
  url: string
}

export interface ProjectDetail {
  id: string
  slug: string
  title: string
  description: string
  projectType: string
  categories: string[]
  iconUrl?: string
  gallery: GalleryImage[]
  downloads: number
  followers: number
  updated: string
  license: string
  gameVersions: string[]
  loaders: string[]
  links: ProjectLink[]
}

export interface ProjectVersion {
  id: string
  name: string
  versionNumber: string
  versionType: string
  gameVersions: string[]
  loaders: string[]
  downloads: number
  datePublished: string
  fileName?: string
}

const PAGE = 40

export const compactNumber = (value: number) =>
  value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(1)}M`
    : value >= 1000
      ? `${(value / 1000).toFixed(0)}K`
      : String(value)

/**
 * 一个版本能不能装进这个实例。
 *
 * 分开报告版本和加载器，因为这两件事的应对方式不同：版本不对要等作者更新，
 * 加载器不对是选错了实例。合成一句「不兼容」就把这个区别抹掉了。
 *
 * Quilt 能加载 Fabric 模组，而多数作者只标 fabric——这里和搜索的 facet 用的
 * 是同一条规则，两处不一致会出现「搜得到但说装不上」。
 */
export function compatibility(
  version: ProjectVersion,
  target: { gameVersion: string; loader: string } | undefined,
  kind: ResourceKind,
): { ok: boolean; note: string } {
  if (!target) return { ok: true, note: '' }
  if (!version.gameVersions.includes(target.gameVersion)) {
    return { ok: false, note: `不支持 ${target.gameVersion}` }
  }
  if (kind !== 'mod') return { ok: true, note: '' }
  const accepted =
    target.loader === 'Quilt' ? ['quilt', 'fabric'] : [target.loader.toLowerCase()]
  if (target.loader === 'Vanilla') return { ok: false, note: '实例没有加载器' }
  if (!version.loaders.some((item) => accepted.includes(item))) {
    return { ok: false, note: `不支持 ${target.loader}` }
  }
  return { ok: true, note: '' }
}

class SupplyStore {
  kind = $state<ResourceKind>('mod')
  query = $state('')
  /** 空串表示不限。默认就是不限——补给站是在浏览整个 Modrinth。 */
  gameVersion = $state('')
  loader = $state('')
  sort = $state('relevance')

  hits = $state<Hit[]>([])
  total = $state(0)
  searching = $state(false)
  error = $state('')
  loaded = $state(false)

  /** 装到哪个实例。是浏览的上下文，不是筛选条件。 */
  targetId = $state('')
  /**
   * 正在看的项目叫什么。
   *
   * 顶栏的面包屑要显示它——地址里是 slug（`fabric-api`），而人认的是标题
   * （`Fabric API`）。详情页读到之后填进来。
   */
  viewingTitle = $state('')

  target = $derived(
    instances.list.find((item) => item.id === this.targetId) ?? instances.current,
  )

  /** 已经翻到第几页。列表在返回详情页之后要还在原地。 */
  #offset = 0
  /** 结果区滚到哪了。切走再回来要接着看，不然又要从头往下滑一遍。 */
  scrollTop = 0

  /** 点卡片的那一刻就知道标题了，不必等详情加载完面包屑才对。 */
  beginViewing(title: string) {
    this.viewingTitle = title
  }

  /** 把筛选条件设成「为这个实例找东西」。跨场景跳转过来时用。 */
  aimAt(instanceId: string) {
    this.targetId = instanceId
    const instance = instances.list.find((item) => item.id === instanceId)
    if (!instance) return
    this.gameVersion = instance.gameVersion
    this.loader = loaderTag(instance.loader)
    this.loaded = false
  }

  async search(append = false) {
    if (!inTauri()) {
      this.loaded = true
      return
    }
    this.searching = true
    this.#offset = append ? this.#offset + PAGE : 0
    try {
      const result = await invoke<{ hits: Hit[]; total: number }>('search_resources', {
        query: {
          query: this.query.trim(),
          kind: this.kind,
          gameVersion: this.gameVersion,
          loader: this.loader || null,
          category: '',
          sort: this.sort,
          offset: this.#offset,
          limit: PAGE,
        },
      })
      this.hits = append ? [...this.hits, ...result.hits] : result.hits
      this.total = result.total
      this.error = ''
      this.loaded = true
    } catch (cause) {
      this.error = String(cause)
    } finally {
      this.searching = false
    }
  }

  /** 条件变了就从头搜。翻页是唯一会追加的情况。 */
  refresh() {
    void this.search(false)
  }

  more() {
    if (!this.searching && this.hits.length < this.total) void this.search(true)
  }

  get canLoadMore() {
    return this.hits.length > 0 && this.hits.length < this.total
  }
}

export const supply = new SupplyStore()

/**
 * 加载器筛选的取值。
 *
 * 这里用的是 Rust `LoaderKind` 的 serde 名（`neo_forge`），不是 Modrinth 的
 * 标签（`neoforge`）。两个命名空间长得像但不是一回事——发错了后端会直接反
 * 序列化失败，而这个错误编译期看不见。`compatibility()` 里比对的才是
 * Modrinth 的标签。
 */
const LOADER_TAGS: Record<string, string> = {
  Fabric: 'fabric',
  Quilt: 'quilt',
  NeoForge: 'neo_forge',
  Forge: 'forge',
}

/** 显示名 → serde 名。原版没有加载器，返回空串表示不限。 */
export const loaderTag = (displayName: string) => LOADER_TAGS[displayName] ?? ''

export const LOADER_FILTERS = Object.entries(LOADER_TAGS).map(([label, id]) => ({ id, label }))
