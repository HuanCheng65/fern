/**
 * 会话事件。
 *
 * 形状与 pearl-core 的 sidecar 协议（DESIGN §8）逐字段一致——UI 走进程内的
 * Tauri 命令，第三方启动器走 JSON-RPC，两边说同一套词。这样这套词汇有人天天
 * 在用，不会因为没人看而烂掉。
 *
 * 状态那几个类型和它们的中文说法在 `fern-kit/parts/pearl`：卡片要显示的东西
 * 官网也要显示，而这份事件流是后端的词汇，只有产品认识。
 */

export type { PathState, PeerState, PunchStage } from 'fern-kit/parts/pearl'
export { PATH_LABEL, PATH_QUALITY, PUNCH_STAGE_LABEL } from 'fern-kit/parts/pearl'

import type { PathState, PeerState, PunchStage } from 'fern-kit/parts/pearl'

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
