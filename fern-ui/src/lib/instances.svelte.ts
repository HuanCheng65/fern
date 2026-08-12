/**
 * 实例数据。
 *
 * 只放后端真的给得出来的字段。模组数量这类暂时没有数据源，界面上就不该出现
 * 它们的位置——留着一个永远显示 0 的格子，比没有这个格子更糟。
 *
 * 「当前实例」和「正在看的实例」是两件事。曲库里点开一张卡片只是看，把它
 * 送上启动场景是另一个明确的动作——混淆这两者是很多启动器难用的根源：
 * 用户只想翻一眼模组列表，结果下次打开发现要玩的实例被换掉了。
 */

import { invoke } from '@tauri-apps/api/core'
import { commands, provides, type Subject } from 'fern-kit/parts/palette'
import { nav } from './nav.svelte'

/**
 * 实例场景那一级里，不是实例的那两屏。
 *
 * 实例 id 是随机发的，撞不上这两个词。它们只是**保留的段名**——各自叫什么
 * 由渲染它的那一屏自己登记（`nav.name`），这里不留标题。
 */
export const CREATE = 'new'
export const EXISTING = 'existing'

/**
 * 实例的一层。
 *
 * 一个实例是一摞有序的层，不是「一个版本加一个加载器」——第 0 层是游戏本体，
 * 加载器叠在它上面。界面上此前只说得出「Fabric」，说不出是哪一版，而装模组前
 * 要知道的恰恰是后者。
 */
export interface Layer {
  /** 显示名：游戏本体那一层是 Minecraft，其余是加载器名。 */
  name: string
  version: string
}

export interface Instance {
  id: string
  name: string
  gameVersion: string
  loader: string
  /** 从游戏本体开始，按叠放顺序。 */
  layers: Layer[]
  /** 封面的恒定种子。用它而不是名字——改个名字不该换一张脸。 */
  cover: string
  /** 上次玩过的 Unix 秒。从没玩过是 undefined。 */
  lastPlayed?: number
  /** 这个实例是什么时候加进来的，Unix 秒。文件系统答不出来时是 undefined。 */
  createdAt?: number
  /** 累计游玩秒数。没有记录过是 0。 */
  playSeconds: number
  /** 这个实例用哪个账户。没记过就是 undefined，跟着当前账户走。 */
  accountId?: string
  /** 游戏文件所在的目录。只有外部实例有——它的文件不在 Fern 的数据目录里。 */
  externalRoot?: string
}

export interface VersionOption {
  id: string
  kind: string
  releaseTime: string
}

/** 现在装得上的加载器。列表由后端给——能装什么是后端知道的事。 */
export interface LoaderOption {
  kind: string
  label: string
  /** 它是叠在别人上面的一层，不是「主加载器」的候选之一。 */
  stackable?: boolean
}

interface CoreInstance {
  id: string
  name: string
  gameVersion: string
  loader: string
  components?: { kind: string; version: string }[]
  cover?: { identity: string }
  lastPlayed?: number
  createdAt?: number
  playSeconds?: number
  accountId?: string
  external?: { root: string }
}

export const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

const LOADER_NAMES: Record<string, string> = {
  vanilla: 'Vanilla',
  fabric: 'Fabric',
  forge: 'Forge',
  neo_forge: 'NeoForge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  liteloader: 'LiteLoader',
  lite_loader: 'LiteLoader',
}

export const loaderName = (loader: string) => LOADER_NAMES[loader] ?? loader

/**
 * 层表。
 *
 * 游戏本体那一层的 kind 也是 vanilla，但它不是一个加载器——在界面上它叫
 * Minecraft。层表为空的实例（还没迁移过的旧描述）退回版本号那一层，列表里
 * 至少要说得出这是哪个版本。
 */
const toLayers = (profile: CoreInstance): Layer[] => {
  const components = profile.components ?? []
  if (components.length === 0) {
    return [{ name: 'Minecraft', version: profile.gameVersion }]
  }
  return components.map((component) => ({
    name: component.kind === 'vanilla' ? 'Minecraft' : loaderName(component.kind),
    version: component.version,
  }))
}

const toInstance = (profile: CoreInstance): Instance => ({
  id: profile.id,
  name: profile.name,
  gameVersion: profile.gameVersion,
  loader: loaderName(profile.loader),
  layers: toLayers(profile),
  cover: profile.cover?.identity || profile.id,
  lastPlayed: profile.lastPlayed,
  createdAt: profile.createdAt,
  playSeconds: profile.playSeconds ?? 0,
  accountId: profile.accountId,
  externalRoot: profile.external?.root,
})

/**
 * 曲库按什么排。
 *
 * 三档各回答一个不同的问题：`played` 是「接着玩哪个」，`name` 是「我有什么」，
 * `created` 是「上周建的那个在哪」。不加「占用」——那要为每个实例走一遍目录树，
 * 而「占用都在哪」在设置的存储那一屏已经答过了。
 */
export type Order = 'played' | 'name' | 'created'

export const ORDERS: { value: Order; label: string }[] = [
  { value: 'played', label: '最近游玩' },
  { value: 'name', label: '名称' },
  { value: 'created', label: '创建时间' },
]

const byName = (left: Instance, right: Instance) =>
  left.name.localeCompare(right.name, 'zh-Hans-CN')

/** 排序都以名称收尾：并列的两个必须有一个确定的先后，否则它们的位置会跳。 */
const SORTS: Record<Order, (left: Instance, right: Instance) => number> = {
  played: (left, right) => (right.lastPlayed ?? 0) - (left.lastPlayed ?? 0) || byName(left, right),
  name: byName,
  // 新加进来的在前。答不出创建时刻的沉底，而不是被当成 1970 年建的。
  created: (left, right) => (right.createdAt ?? 0) - (left.createdAt ?? 0) || byName(left, right),
}

const SELECTED_KEY = 'fern.instance.selected'
const ORDER_KEY = 'fern.instances.order'

const savedOrder = (): Order => {
  const saved = localStorage.getItem(ORDER_KEY)
  return ORDERS.some((item) => item.value === saved) ? (saved as Order) : 'played'
}

/** 浏览器里跑 `pnpm dev` 时的兜底存储，让界面在没有 Tauri 的情况下也能走通。 */
const PREVIEW_KEY = 'fern.instances.preview'

class InstanceStore {
  list = $state<Instance[]>([])
  selectedId = $state<string>(localStorage.getItem(SELECTED_KEY) ?? '')
  loading = $state(true)
  error = $state('')

  versions = $state<VersionOption[]>([])
  versionsLoading = $state(false)
  loaders = $state<LoaderOption[]>([])

  current = $derived(this.list.find((item) => item.id === this.selectedId) ?? this.list[0])

  /**
   * 最近玩过的在前，没玩过的按名字排在后面。
   *
   * 「上次玩的那个」几乎总是「这次要玩的那个」，所以这个顺序本身就是一次
   * 免费的推荐。要给一个实例选东西的那些地方（补给站的目标实例）用它，而且
   * 不跟着曲库的排序走——那是曲库的视图偏好，不是所有列表的顺序。
   */
  recent = $derived([...this.list].sort(SORTS.played))

  /** 曲库当前按什么排。记在本地，下次打开还是这个。 */
  order = $state<Order>(savedOrder())
  ordered = $derived([...this.list].sort(SORTS[this.order]))

  setOrder(order: Order) {
    this.order = order
    try {
      localStorage.setItem(ORDER_KEY, order)
    } catch {
      // 记不住只是下次回到默认顺序，不值得打断使用。
    }
  }

  select(id: string) {
    this.selectedId = id
    try {
      localStorage.setItem(SELECTED_KEY, id)
    } catch {
      // 记不住选择只是下次打开回到第一个，不值得打断使用。
    }
  }

  async load() {
    this.loading = true
    this.error = ''
    try {
      const profiles = inTauri()
        ? await invoke<CoreInstance[]>('list_instances')
        : (JSON.parse(localStorage.getItem(PREVIEW_KEY) ?? '[]') as CoreInstance[])
      this.list = profiles.map(toInstance)
      if (!this.list.some((item) => item.id === this.selectedId)) {
        this.selectedId = this.list[0]?.id ?? ''
      }
    } catch (error) {
      this.error = String(error)
    } finally {
      this.loading = false
    }
  }

  /**
   * 版本清单。
   *
   * 这一进程里只拉一次；`refresh` 是用户主动要最新的那一下，它会穿过这一层，
   * 也会穿过后端六小时的缓存。
   */
  async loadVersions(refresh = false) {
    if (!refresh && (this.versions.length > 0 || this.versionsLoading)) return
    this.versionsLoading = true
    try {
      this.versions = inTauri()
        ? await invoke<VersionOption[]>('list_versions', { refresh })
        : ((await fetch('https://piston-meta.mojang.com/mc/game/version_manifest_v2.json').then(
            (r) => r.json(),
          )) as { versions: { id: string; type: string; releaseTime: string }[] }).versions.map(
            (v) => ({ id: v.id, kind: v.type, releaseTime: v.releaseTime }),
          )
    } finally {
      this.versionsLoading = false
    }
  }

  /** 装得上哪些加载器。只拉一次。 */
  /**
   * 这个游戏版本上装得上的加载器。
   *
   * **必须带上版本**：1.7.10 上没有 Fabric，1.21 上没有 LiteLoader。摆一个
   * 装不上的选项，等于让人走到一半才被拦住。所以这一份不缓存——换一个版本
   * 就是另一份答案。
   */
  async loadLoaders(gameVersion = ''): Promise<LoaderOption[]> {
    if (!inTauri()) return this.loaders
    try {
      this.loaders = await invoke<LoaderOption[]>('installable_loaders', { gameVersion })
    } catch {
      // 拿不到就只给原版：少一个选项，好过给一个点了会失败的选项。
    }
    return this.loaders
  }

  /** 这个版本 × 这个主加载器之下还能叠哪些附加层。多数组合下是空的。 */
  async loadAddons(gameVersion: string, loader: string): Promise<LoaderOption[]> {
    if (!inTauri() || !gameVersion) return []
    try {
      return await invoke<LoaderOption[]>('loader_addons', { gameVersion, loader })
    } catch {
      return []
    }
  }

  /**
   * 钉住这个实例用哪个账户。null 是「跟着当前账户走」，也是默认。
   *
   * 只有人明确要求时才写它。启动那一步曾经也悄悄写一次——于是任何启动过的
   * 实例都被永久钉在了当时的账户上，此后换当前账户对它毫无作用，而界面上没有
   * 任何地方说过这件事。一次沉默不该变成一次表态。
   */
  async setAccount(id: string, accountId: string | null) {
    if (!inTauri()) return
    try {
      await invoke('set_instance_account', { instanceId: id, accountId })
      await this.load()
    } catch (error) {
      this.error = String(error)
    }
  }

  /** 改名、复制、删除之后都要重新读一遍，列表和封面才对得上。 */
  async rename(id: string, name: string) {
    await invoke('rename_instance', { instanceId: id, name })
    await this.load()
  }

  async duplicate(id: string, name: string): Promise<string> {
    const created = await invoke<CoreInstance>('duplicate_instance', { instanceId: id, name })
    await this.load()
    return created.id
  }

  async remove(id: string) {
    await invoke('delete_instance', { instanceId: id })
    await this.load()
  }

  async create(
    name: string,
    gameVersion: string,
    loader = 'vanilla',
    loaderVersion = '',
  ): Promise<Instance> {
    const created: CoreInstance = inTauri()
      ? await invoke<CoreInstance>('create_instance', {
          name,
          gameVersion,
          loader,
          // 留空就让后端取最新稳定版——那是绝大多数人想要的答案。
          loaderVersion: loaderVersion || null,
        })
      : { id: `preview-${Date.now()}`, name, gameVersion, loader }
    const instance = toInstance(created)
    this.list = [...this.list, instance]
    if (!inTauri()) {
      localStorage.setItem(PREVIEW_KEY, JSON.stringify(this.list.map((i) => ({ ...i, loader: 'vanilla' }))))
    }
    this.select(instance.id)
    return instance
  }
}

export const instances = new InstanceStore()

/**
 * 实例是这个面板的主角，所以它们平铺在顶层，不藏在一次下钻后面。
 *
 * 默认动作是「送上启动场景」而不是「打开详情」：点实例名呼出的切换器和 ⌘K
 * 是同一个东西，而切换器的意思就是换一个来玩。
 */
provides(() =>
  instances.list.map(
    (item): Subject => ({
      type: 'instance',
      id: item.id,
      title: item.name,
      hint: `${item.gameVersion} · ${item.loader}`,
      seed: item.cover,
      // 玩过的时间就是面板要的那个先验：不必等它从零学起。核心那边记的是秒。
      seen: item.lastPlayed === undefined ? undefined : item.lastPlayed * 1000,
      run: () => {
        instances.select(item.id)
        nav.go('launch')
      },
    }),
  ),
)

const asSubject = (id: string): Subject | undefined => {
  const item = instances.list.find((entry) => entry.id === id)
  if (!item) return undefined
  return {
    type: 'instance',
    id: item.id,
    title: item.name,
    hint: `${item.gameVersion} · ${item.loader}`,
    seed: item.cover,
    run: () => instances.select(item.id),
  }
}

/** 当前实例。动作有它就一步到位，没有就下钻去问「对哪个」。 */
const current = () => (instances.current ? asSubject(instances.current.id) : undefined)

commands(() => [
  {
    id: 'instance.open',
    title: '打开实例详情',
    accepts: 'instance',
    subject: current,
    run: (subject) => subject && nav.enter('instances', subject.id),
  },
  {
    id: 'instance.directory',
    title: '打开游戏目录',
    accepts: 'instance',
    subject: current,
    run: (subject) => {
      if (subject) void invoke('open_instance_directory', { instanceId: subject.id })
    },
  },
  {
    id: 'instance.snapshots',
    title: '查看快照',
    accepts: 'instance',
    subject: current,
    run: (subject) => {
      if (!subject) return
      nav.enter('instances', subject.id)
      nav.setTab('snapshots')
    },
  },
  {
    id: 'instance.create',
    title: '新建实例',
    accepts: 'none',
    creates: 'instance',
    run: () => nav.enter('instances', CREATE),
  },
])
