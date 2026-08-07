/**
 * 会话状态：把事件流折成界面要显示的那几样东西。
 *
 * 事件是流水账，界面要的是当下。这一层做的就是这个折叠，而且只做这个——
 * 它不发起连接，不知道 Tauri 存不存在，所以假后端和真后端喂进来的东西
 * 走的是同一条路。
 */

import { backend } from './pearl-backend'
import type { PathState, PeerState, PunchStage, SessionEvent } from './pearl-types'

export interface Peer {
  id: string
  name: string
  state: PeerState
  rttMs?: number
  /** 谁在替这条路转发。只有 state 是 via 时有值。 */
  via?: string
  stage?: PunchStage
  stageDone?: number
  stageTotal?: number
  detail?: string
}

export type Mode = 'idle' | 'hosting' | 'joining'

/** 没有记住过名字时用的那个。第一次打开的那一屏不能是空的。 */
export const DEFAULT_NAME = '玩家'

const PATH_STATES: PathState[] = ['lan', 'direct_ip6', 'mapped', 'punched', 'via']
export const isConnected = (peer: Peer) =>
  PATH_STATES.includes(peer.state as PathState) || peer.state === 'connected'

class Session {
  mode = $state<Mode>('idle')
  /** 显示给房间里其他人的名字。记在配置目录里，跨次启动。 */
  name = $state(DEFAULT_NAME)
  /** 房间码，六位数字。 */
  code = $state<string | null>(null)
  invite = $state<string | null>(null)
  spoken = $state<string | null>(null)
  nodeId = $state<string | null>(null)
  signalOnline = $state(true)
  signalDetail = $state<string | null>(null)

  /** 主机侧：等世界 / 已经找到的世界。 */
  watchingLan = $state(false)
  world = $state<{ motd: string; address: string } | null>(null)
  /** 主机手动指定的共享端口。设了它就优先于自动发现,转发指向 127.0.0.1:端口。 */
  sharedPort = $state<number | null>(null)
  /** 访客侧：本地代理端口，和游戏列表里那一条的名字。 */
  localPort = $state<number | null>(null)
  lanName = $state<string | null>(null)

  peers = $state<Peer[]>([])
  error = $state<string | null>(null)
  /**
   * 会话已经整个结束了。和 error 不同：error 是会话里发生的一件坏事（比如房间
   * 被锁),会话可能还在;ended 之后什么都不剩,界面不能再声称「准备就绪」。
   */
  ended = $state(false)
  /** 开始连接的那一刻，用来显示已经等了多久。 */
  startedAt = $state<number | null>(null)

  readonly connected = $derived(this.peers.filter(isConnected))
  readonly relayed = $derived(this.peers.filter((p) => p.state === 'via'))
  readonly backendKind = backend.kind

  #stop: (() => void) | null = null

  #peer(id: string): Peer {
    const found = this.peers.find((p) => p.id === id)
    if (found) return found
    // 名字可能比状态晚到，先用一个能认的短名占位。
    const created: Peer = { id, name: id.slice(6, 12), state: 'connecting' }
    this.peers = [...this.peers, created]
    return created
  }

  #update(id: string, change: Partial<Peer>) {
    const peer = this.#peer(id)
    this.peers = this.peers.map((p) => (p.id === peer.id ? { ...p, ...change } : p))
  }

  apply(event: SessionEvent) {
    switch (event.event) {
      case 'identity':
        this.nodeId = event.node_id
        break
      case 'room_created':
        this.code = event.code
        this.invite = event.invite
        this.spoken = event.spoken
        break
      case 'watching_lan':
        this.watchingLan = true
        break
      case 'lan_world':
        this.watchingLan = false
        this.world = { motd: event.motd, address: event.address }
        break
      case 'lan_ready':
        this.localPort = event.local_port
        break
      case 'lan_announced':
        this.lanName = event.motd
        break
      case 'sharing_port':
        this.sharedPort = event.port
        break
      case 'peer_named':
        this.#update(event.peer, { name: event.name })
        break
      case 'peer_left_room':
        this.peers = this.peers.filter((p) => p.id !== event.peer)
        break
      case 'peer_state':
        this.#update(event.node_id, {
          state: event.state,
          rttMs: event.rtt_ms,
          via: event.via,
          detail: event.detail,
          // 状态定下来了，中间过程就不必再显示。
          stage: undefined,
          stageDone: undefined,
          stageTotal: undefined,
        })
        break
      case 'punch_progress':
        this.#update(event.peer, {
          stage: event.stage,
          stageDone: event.done,
          stageTotal: event.total,
        })
        break
      case 'signal':
        this.signalOnline = event.connected
        this.signalDetail = event.detail ?? null
        break
      case 'error':
        this.error = event.detail ?? event.code ?? '遇到未知错误'
        break
      case 'ended':
        this.ended = true
        if (event.detail) this.error = event.detail
        break
    }
  }

  #reset(mode: Mode) {
    this.mode = mode
    this.code = null
    this.invite = null
    this.spoken = null
    this.world = null
    this.watchingLan = false
    this.sharedPort = null
    this.localPort = null
    this.lanName = null
    this.peers = []
    this.error = null
    this.ended = false
    this.signalOnline = true
    this.startedAt = Date.now()
    this.#stop?.()
    this.#stop = backend.subscribe((event) => this.apply(event))
  }

  /** 读回上次用的名字。没有就保持默认，不算失败。 */
  async loadName() {
    const remembered = await backend.rememberedName().catch(() => null)
    if (remembered?.trim()) this.name = remembered.trim()
  }

  /**
   * 记住当前的名字。
   *
   * 开房和加入时后端也会记一次——那是名字真正被用出去的时刻。这一条是给「改了
   * 名字但没开房」的情况：那样的一次输入明天还应该在，因为房间没开就把它丢掉，
   * 这个输入框会显得记性随机。
   */
  async rememberName() {
    const name = this.name.trim()
    if (!name) {
      // 清空不是一个想被记住的状态，把默认值还回去。
      this.name = DEFAULT_NAME
      return
    }
    this.name = name
    await backend.rememberName(name).catch(() => {})
  }

  async host(name: string) {
    this.#reset('hosting')
    try {
      await backend.host(name)
    } catch (error) {
      this.error = String(error)
      this.mode = 'idle'
    }
  }

  async join(invite: string, name: string) {
    this.#reset('joining')
    // 访客不会收到 room_created，但邀请里就带着房间码：前六位是码，后六位是
    // 密码。取前六位，一个房间在两边就是同一张脸。后六位一个字都不要碰——
    // 它会变成一张画在屏幕上、随手就被截图发出去的东西。
    const digits = invite.replace(/\D/g, '')
    if (digits.length >= 6) this.code = digits.slice(0, 6)
    try {
      await backend.join(invite, name)
    } catch (error) {
      this.error = String(error)
      this.mode = 'idle'
    }
  }

  /**
   * 把转发指向手动指定的端口,或改回跟随游戏。
   *
   * 不做乐观更新:sharedPort 只由后端的 sharing_port 事件改,界面显示的
   * 永远是转发真实指着的地方,而不是一次可能失败的请求。
   */
  async sharePort(port: number | null) {
    try {
      await backend.sharePort(port)
    } catch (error) {
      this.error = String(error)
    }
  }

  async leave() {
    await backend.stop()
    this.#stop?.()
    this.#stop = null
    this.mode = 'idle'
    this.startedAt = null
    this.peers = []
  }
}

export const session = new Session()
