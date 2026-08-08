/**
 * 存档与服务器。
 *
 * 它们比实例更接近「我要去的地方」——人想的是「进生存服」，不是「打开生存服
 * 所在的那个实例，再从多人游戏列表里找到它」。而启动器是唯一同时知道你有哪些
 * 世界、哪些服务器、以及它们各属于哪个实例的地方，所以回车之后可以直接进去
 * （游戏的 quickPlay 参数），而不是把人送到一个列表前面。
 *
 * **随开随取，不缓存。** 世界是在游戏里创建的，服务器是在游戏里添加的，启动器
 * 无从知道这两件事发生过——任何缓存都会悄悄过期，而「搜不到我昨天刚建的世界」
 * 比慢一点糟得多。取的代价也不高：只读目录名，不算体积（后者要把每个世界的
 * 几万个区块文件都 stat 一遍，那是详情页才需要付的）。
 */

import { invoke } from '@tauri-apps/api/core'
import { inTauri } from './instances.svelte'
import { launch } from './launch.svelte'
import { onOpen, provides, type Subject } from 'fern-kit/palette'

export interface Place {
  instanceId: string
  instanceName: string
  name: string
  /** 服务器才有。存档是 null。 */
  address: string | null
}

class PlaceStore {
  saves = $state<Place[]>([])
  servers = $state<Place[]>([])
  #loading = false

  /** 取一次快照。面板打开时调，不做去重之外的任何缓存。 */
  async refresh() {
    if (!inTauri() || this.#loading) return
    this.#loading = true
    try {
      const places = await invoke<{ saves: Place[]; servers: Place[] }>('list_places')
      this.saves = places.saves
      this.servers = places.servers
    } catch {
      // 读不到就少一类结果。这两份数据都不归我们管，出问题不该让搜索失败。
    } finally {
      this.#loading = false
    }
  }
}

export const places = new PlaceStore()

/**
 * 打字才出现。
 *
 * 不必额外标记：空态只列最近用过的加实例，所以没被用过的世界和服务器天然
 * 不在那里；用过的会自己浮上来，那正是应该的——上周天天进的那个服，下次
 * 打开面板就该在第一屏。
 */
provides(() => [
  ...places.saves.map(
    (item): Subject => ({
      type: 'world',
      id: `${item.instanceId}/${item.name}`,
      title: item.name,
      hint: item.instanceName,
      run: () => void launch.launch(item.instanceId, { world: item.name }),
    }),
  ),
  ...places.servers.map(
    (item): Subject => ({
      type: 'server',
      id: `${item.instanceId}/${item.address}`,
      // 没起过名字的条目拿地址顶上——列表里必须有个能认的东西。
      title: item.name || (item.address ?? ''),
      hint: item.name ? `${item.address} · ${item.instanceName}` : item.instanceName,
      run: () => void launch.launch(item.instanceId, { server: item.address ?? '' }),
    }),
  ),
])

/** 面板一打开就重取一次。 */
onOpen(() => void places.refresh())
