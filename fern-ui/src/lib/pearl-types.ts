/**
 * 会话事件。
 *
 * 形状与 pearl-core 的 sidecar 协议（DESIGN §8）逐字段一致——UI 走进程内的
 * Tauri 命令，第三方启动器走 JSON-RPC，两边说同一套词。这样这套词汇有人天天
 * 在用，不会因为没人看而烂掉。
 */

export type PathState = 'lan' | 'direct_ip6' | 'mapped' | 'punched' | 'via'

export type PeerState =
  | PathState
  | 'connecting'
  | 'connected'
  | 'disconnected'
  | 'failed'
  | 'path_lost'

export type PunchStage = 'direct' | 'mappings' | 'guessing' | 'waiting'

export type SessionEvent =
  | { event: 'identity'; node_id: string }
  | {
      event: 'network'
      listen_addr: string
      nat: unknown
      port_taken: number | null
      forwarded: unknown
    }
  | { event: 'room_created'; code: string; invite: string; spoken: string }
  | { event: 'waiting' }
  | { event: 'peer_named'; peer: string; name: string }
  | { event: 'peer_left_room'; peer: string }
  | {
      event: 'peer_state'
      node_id: string
      state: PeerState
      rtt_ms?: number
      detail?: string
      via?: string
      from?: PathState
      session?: string
    }
  | {
      event: 'punch_progress'
      peer: string
      stage: PunchStage
      done?: number
      total?: number
    }
  | { event: 'forwarding'; for: string[]; rate_kbps: number; total_mb: number }
  | { event: 'signal'; connected: boolean; detail?: string }
  | { event: 'lan_ready'; local_port: number }
  | { event: 'watching_log'; path: string }
  | { event: 'watching_lan' }
  | { event: 'lan_world'; motd: string; address: string }
  | { event: 'lan_announced'; motd: string }
  /** 主机手动指定了共享端口(优先于游戏自己宣告的世界),null 表示改回自动。 */
  | { event: 'sharing_port'; port: number | null }
  | { event: 'path_switch_cost'; node_id: string; from: PathState; to: PathState }
  | { event: 'error'; code?: string; detail?: string; wrong_passwords?: number }
  /** 会话整个结束了——之后不会再有任何事件。detail 是失败原因，正常结束没有。 */
  | { event: 'ended'; detail?: string | null }

/** 路径的说法。玩家不需要知道 EIM/EDM，只需要知道这条路好不好。 */
export const PATH_LABEL: Record<PathState, string> = {
  lan: '局域网',
  direct_ip6: 'IPv6 直连',
  mapped: '端口映射',
  punched: '直连',
  via: '中转',
}

/** 中转是能用但要花别人带宽的，值得单独标出来。 */
export const PATH_QUALITY: Record<PathState, 'best' | 'good' | 'fallback'> = {
  lan: 'best',
  direct_ip6: 'best',
  mapped: 'good',
  punched: 'good',
  via: 'fallback',
}

export const PUNCH_STAGE_LABEL: Record<PunchStage, string> = {
  direct: '尝试直连',
  mappings: '建立端口映射',
  guessing: '探测端口',
  waiting: '等待对方响应',
}
