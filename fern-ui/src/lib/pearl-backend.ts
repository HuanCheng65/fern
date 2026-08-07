/**
 * 会话的后端。
 *
 * 两个实现：Tauri 的进程内命令，和一个演示用的假后端。分开是为了界面能在
 * 浏览器里单独开发和验证——设计上的问题不该等系统依赖装好才发现——但真正
 * 的价值是它逼出了一层窄接口：界面只知道「开始、停止、事件流」，不知道
 * QUIC、不知道信令、不知道打洞。
 */

import { isTauri } from './pearl-platform'
import type { SessionEvent } from './pearl-types'

export interface Backend {
  readonly kind: 'tauri' | 'demo'
  host(name: string): Promise<void>
  join(invite: string, name: string): Promise<void>
  stop(): Promise<void>
  /** 主机把转发指向自己指定的端口;null 改回跟随游戏。仅开房时有效。 */
  sharePort(port: number | null): Promise<void>
  subscribe(listener: (event: SessionEvent) => void): () => void
  /** The name this machine last went by, if it has ever gone by one. */
  rememberedName(): Promise<string | null>
  rememberName(name: string): Promise<void>
}

class TauriBackend implements Backend {
  readonly kind = 'tauri' as const
  #listeners = new Set<(event: SessionEvent) => void>()
  #unlisten: Promise<() => void> | null = null

  #ensureStream() {
    if (this.#unlisten) return
    this.#unlisten = import('@tauri-apps/api/event').then(({ listen }) =>
      listen<SessionEvent>('session', (message) => {
        for (const listener of this.#listeners) listener(message.payload)
      }),
    )
  }

  async #invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<T>(command, args)
  }

  /**
   * 名字存在配置目录里，和身份密钥、控制 token 放在一起——不是存在 WebView 的
   * 本地存储里。理由在 pearl-core/src/settings.rs 的模块注释里：一台机器应该
   * 只有一个名字，命令行、sidecar 和界面问的是同一个地方；而 WebView 的存储
   * 会被跟 Pearl 无关的事情清掉。
   */
  async rememberedName() {
    return (await this.#invoke<string | null>('pearl_remembered_name')) ?? null
  }

  async rememberName(name: string) {
    await this.#invoke('pearl_remember_name', { name })
  }

  async host(name: string) {
    this.#ensureStream()
    await this.#invoke('pearl_host', { name })
  }

  async join(invite: string, name: string) {
    this.#ensureStream()
    await this.#invoke('pearl_join', { invite, name })
  }

  async stop() {
    await this.#invoke('pearl_stop')
  }

  async sharePort(port: number | null) {
    await this.#invoke('pearl_share_port', { port })
  }

  subscribe(listener: (event: SessionEvent) => void) {
    this.#ensureStream()
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }
}

/**
 * 假后端，按真实时序放一段会话。
 *
 * 时序是照着 pearl-core 的实际顺序写的：身份先出来，网络探测要几秒，房间码
 * 立刻可读，世界要等玩家自己去开放局域网，打洞有中间态。界面上那些「等待」
 * 的样子必须在这里就能看见，否则做出来的一定是只在瞬间完成时好看的界面。
 */
class DemoBackend implements Backend {
  readonly kind = 'demo' as const
  #listeners = new Set<(event: SessionEvent) => void>()
  #timers: ReturnType<typeof setTimeout>[] = []

  #emit(event: SessionEvent) {
    for (const listener of this.#listeners) listener(event)
  }

  #script(steps: [number, SessionEvent][]) {
    for (const [delay, event] of steps) {
      this.#timers.push(setTimeout(() => this.#emit(event), delay))
    }
  }

  async host(_name: string) {
    this.stop()
    this.#script([
      [120, { event: 'identity', node_id: 'pearl1demohost0000000000000000000000000000000000000' }],
      [
        900,
        {
          event: 'network',
          listen_addr: '0.0.0.0:25585',
          nat: 'endpoint_independent',
          port_taken: null,
          forwarded: null,
        },
      ],
      [1000, { event: 'watching_lan' }],
      [1200, { event: 'room_created', code: '483920', invite: 'pearl://483920/517304', spoken: '483920 517304' }],
      [1300, { event: 'waiting' }],
      [4200, { event: 'lan_world', motd: '星穹科技包', address: '127.0.0.1:52731' }],
      [6000, { event: 'peer_named', peer: 'pearl1demoguest000000000000000000000000000000000000', name: 'Alex' }],
      [6100, { event: 'peer_state', node_id: 'pearl1demoguest000000000000000000000000000000000000', state: 'connecting' }],
      [6600, { event: 'punch_progress', peer: 'pearl1demoguest000000000000000000000000000000000000', stage: 'direct', total: 4 }],
      [7400, { event: 'punch_progress', peer: 'pearl1demoguest000000000000000000000000000000000000', stage: 'guessing', done: 128, total: 512 }],
      [8600, { event: 'peer_state', node_id: 'pearl1demoguest000000000000000000000000000000000000', state: 'punched', rtt_ms: 34 }],
      [12000, { event: 'peer_named', peer: 'pearl1demofriend00000000000000000000000000000000000', name: '小明' }],
      [12100, { event: 'peer_state', node_id: 'pearl1demofriend00000000000000000000000000000000000', state: 'connecting' }],
      [15000, { event: 'peer_state', node_id: 'pearl1demofriend00000000000000000000000000000000000', state: 'via', via: 'pearl1demoguest000000000000000000000000000000000000', rtt_ms: 96 }],
    ])
  }

  async join(_invite: string, _name: string) {
    this.stop()
    this.#script([
      [120, { event: 'identity', node_id: 'pearl1demoguest000000000000000000000000000000000000' }],
      [
        900,
        {
          event: 'network',
          listen_addr: '0.0.0.0:25585',
          nat: 'endpoint_dependent',
          port_taken: null,
          forwarded: null,
        },
      ],
      [1100, { event: 'peer_named', peer: 'pearl1demohost0000000000000000000000000000000000000', name: 'Steve' }],
      [1200, { event: 'peer_state', node_id: 'pearl1demohost0000000000000000000000000000000000000', state: 'connecting' }],
      [2000, { event: 'punch_progress', peer: 'pearl1demohost0000000000000000000000000000000000000', stage: 'mappings', total: 8 }],
      [3600, { event: 'peer_state', node_id: 'pearl1demohost0000000000000000000000000000000000000', state: 'punched', rtt_ms: 34 }],
      [3800, { event: 'lan_ready', local_port: 25566 }],
      [3900, { event: 'lan_announced', motd: "Steve 的房间" }],
    ])
  }

  async stop() {
    for (const timer of this.#timers) clearTimeout(timer)
    this.#timers = []
  }

  // 真后端的确认走事件回来,假后端也走同一条路,界面才测得出「等确认」的样子。
  async sharePort(port: number | null) {
    setTimeout(() => this.#emit({ event: 'sharing_port', port }), 150)
  }

  subscribe(listener: (event: SessionEvent) => void) {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  // 浏览器里没有配置目录，用 localStorage 顶上。只影响开发时的演示后端。
  async rememberedName() {
    return localStorage.getItem('pearl.name')
  }

  async rememberName(name: string) {
    localStorage.setItem('pearl.name', name)
  }
}

export const backend: Backend = isTauri ? new TauriBackend() : new DemoBackend()
