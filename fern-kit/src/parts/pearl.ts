/**
 * 联机在界面上的说法。
 *
 * 这里只有「一个人此刻连得怎么样」这件事的视图模型和它的中文说法。事件流的形状
 * （`SessionEvent`，与 sidecar 协议逐字段一致）留在产品那边——那是后端的词汇，
 * 官网既没有 sidecar 也不需要认识它。
 *
 * 分界线就是 `Peer`：它是折叠完事件流之后剩下的东西，是卡片真正要显示的那几样。
 * 产品从真会话里折出来，官网直接写一个——两边喂给 `PeerCard` 的是同一种东西。
 */

/** 连接实际走通的那条路。 */
export type PathState = 'lan' | 'direct_ip6' | 'mapped' | 'punched' | 'via'

export type PeerState =
  | PathState
  | 'connecting'
  | 'connected'
  | 'disconnected'
  | 'failed'
  | 'path_lost'

export type PunchStage = 'direct' | 'mappings' | 'guessing' | 'waiting'

/** 房间里的一个人。卡片要显示的全部内容。 */
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

const PATH_STATES: PathState[] = ['lan', 'direct_ip6', 'mapped', 'punched', 'via']

export const isConnected = (peer: Peer) =>
  PATH_STATES.includes(peer.state as PathState) || peer.state === 'connected'
