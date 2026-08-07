/**
 * 实例数据。
 *
 * 只放后端真的给得出来的字段。游玩时长、模组数量这些暂时没有数据源，
 * 界面上就不该出现它们的位置——留着一个永远显示 0 的格子，比没有这个
 * 格子更糟。
 *
 * 「当前实例」和「正在看的实例」是两件事。曲库里点开一张卡片只是看，把它
 * 送上启动场景是另一个明确的动作——混淆这两者是很多启动器难用的根源：
 * 用户只想翻一眼模组列表，结果下次打开发现要玩的实例被换掉了。
 */

import { invoke } from '@tauri-apps/api/core'

export interface Instance {
  id: string
  name: string
  gameVersion: string
  loader: string
  /** 封面的恒定种子。用它而不是名字——改个名字不该换一张脸。 */
  cover: string
  /** 上次玩过的 Unix 秒。从没玩过是 undefined。 */
  lastPlayed?: number
  /** 这个实例用哪个账户。没记过就是 undefined，跟着当前账户走。 */
  accountId?: string
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
}

interface CoreInstance {
  id: string
  name: string
  gameVersion: string
  loader: string
  cover?: { identity: string }
  lastPlayed?: number
  accountId?: string
}

export const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

const LOADER_NAMES: Record<string, string> = {
  vanilla: 'Vanilla',
  fabric: 'Fabric',
  forge: 'Forge',
  neo_forge: 'NeoForge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
}

export const loaderName = (loader: string) => LOADER_NAMES[loader] ?? loader

const toInstance = (profile: CoreInstance): Instance => ({
  id: profile.id,
  name: profile.name,
  gameVersion: profile.gameVersion,
  loader: loaderName(profile.loader),
  cover: profile.cover?.identity || profile.id,
  lastPlayed: profile.lastPlayed,
  accountId: profile.accountId,
})

const SELECTED_KEY = 'fern.instance.selected'
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
   * 曲库的排列顺序：最近玩过的在前，没玩过的按名字排在后面。
   *
   * 「上次玩的那个」几乎总是「这次要玩的那个」，所以这个顺序本身就是一次
   * 免费的推荐；从没玩过的实例反而需要一个稳定的位置才找得到。
   */
  recent = $derived(
    [...this.list].sort((left, right) => {
      const a = left.lastPlayed ?? 0
      const b = right.lastPlayed ?? 0
      return b - a || left.name.localeCompare(right.name, 'zh-Hans-CN')
    }),
  )

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
  async loadLoaders(): Promise<LoaderOption[]> {
    if (this.loaders.length > 0 || !inTauri()) return this.loaders
    try {
      this.loaders = await invoke<LoaderOption[]>('installable_loaders')
    } catch {
      // 拿不到就只给原版：少一个选项，好过给一个点了会失败的选项。
    }
    return this.loaders
  }

  /**
   * 钉住这个实例用哪个账户。null 是「跟着当前账户走」。
   *
   * 启动那一步也会写它——第一次用某个账户启动之后，这个实例就记住了。所以
   * 这里是「改一下」，不是「设置一次」。
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
