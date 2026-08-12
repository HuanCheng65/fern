/**
 * 皮肤头像。
 *
 * 后端那边已经有磁盘缓存和一天的保鲜期（`fern-core/src/account/skin.rs`），
 * 这一层只解决一件事：一屏里同一个账户的脸会被画好几次（题头、名单、实例设置
 * 各一张），不该因此问后端好几遍。
 *
 * 一个账户只问一次，答案连同「问过了但没有」一起记住——`null` 和「还没问」是
 * 两种状态，把它们混成一个 falsy 值就会变成每次渲染都重新去问一遍。
 *
 * 离线账户不问：它们本来就没有皮肤，那不是一次失败的查询。
 */

import { invoke } from '@tauri-apps/api/core'
import alex from '../assets/alex.png'
import steve from '../assets/steve.png'
import { inTauri } from './instances.svelte'
import type { Account } from './accounts.svelte'

export interface Face {
  url: string
  /**
   * 能不能叠帽子层。
   *
   * 64×32 的老皮肤不能：那个年代没有 alpha，帽子层拿纯黑当透明键，整块叠上去
   * 就是一颗黑头。判断在后端做（那边读得到 PNG 的高度）。
   */
  hat: boolean
}

/**
 * 没有皮肤的时候该长什么样。
 *
 * 和游戏本体同一条规则：UUID 哈希的奇偶决定 Alex 还是 Steve。所以一个没设过
 * 皮肤的号在 Fern 里和在游戏里是同一张脸——这正是「默认皮肤」的意义，换成别的
 * 图形就等于让启动器自己发明了一个和游戏对不上的身份。
 *
 * Java 那边 `UUID.hashCode()` 是 `(int)(hilo >> 32) ^ (int)hilo`，其中
 * `hilo` 是高 64 位异或低 64 位。这里只要最低一位，所以算到奇偶就够。
 */
function fallback(uuid: string): Face {
  const hex = uuid.replace(/-/g, '')
  // 两张默认皮肤都是 64×64 的新格式，帽子层照叠。
  // 认不出的 UUID 给 Steve：这是 Java 那边 hashCode 为偶数时的那一档。
  if (!/^[0-9a-fA-F]{32}$/.test(hex)) return { url: steve, hat: true }
  const hilo = BigInt(`0x${hex.slice(0, 16)}`) ^ BigInt(`0x${hex.slice(16)}`)
  return { url: (((hilo >> 32n) ^ hilo) & 1n) === 1n ? alex : steve, hat: true }
}

class SkinStore {
  /** 账户 id → 那张脸。`null` 表示问过了，这个账户没有皮肤。 */
  private known = $state<Record<string, Face | null>>({})
  /** 正在问的。不进 `$state`：它只用来去重，不该引起重绘。 */
  private asking = new Set<string>()

  /**
   * 这个账户的脸。**永远给得出一张**：拿不到真皮肤就是默认的 Steve/Alex。
   *
   * 不退回生成式色块——那套图形是实例的脸，拿它当没有皮肤时的头像，等于告诉
   * 玩家「你长这样」，而他在游戏里根本不长这样。默认皮肤是游戏本身给的答案。
   *
   * 头一次问后端的那一两秒里显示的也是默认皮肤，随后换成真的。这一下轻微的
   * 跳变是游戏自己也有的行为，而且只发生在第一次——之后后端直接从磁盘给。
   *
   * **读脸不发请求**：新的调用点要自己配一个 `request` 的 `$effect`，否则它那一
   * 屏永远停在默认皮肤上，直到别处替它问过。
   */
  face(account: Account): Face {
    if (account.kind === 'offline') return fallback(account.uuid)
    return this.known[account.id] ?? fallback(account.uuid)
  }

  /**
   * 要一次。重复调用是安全的——组件的 `$effect` 每次重跑都会调它。
   */
  async request(account: Account) {
    if (!inTauri() || account.kind === 'offline') return
    if (this.asking.has(account.id) || account.id in this.known) return
    this.asking.add(account.id)
    try {
      this.known[account.id] = await invoke<Face | null>('account_skin', { id: account.id })
    } catch {
      // 拿不到脸就用生成图。这件事不值得说给任何人听，更不该打断别的事。
      this.known[account.id] = null
    }
  }
}

export const skins = new SkinStore()
