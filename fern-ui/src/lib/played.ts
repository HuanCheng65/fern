/**
 * 「什么时候玩过、玩了多久」的说法。实例卡片和实例详情共用这一份。
 */
import { dayLabel } from 'fern-kit/parts/snapshots'

/**
 * 上次玩是哪天：今天 / 昨天 / 9月23日。
 *
 * 和快照分组同一条规矩：最近两天说相对的，再往前说日期——「五天前」要人自己
 * 换算，日期才是记得住的。
 */
export const lastPlayedLabel = (seconds: number | undefined) =>
  seconds === undefined ? '尚未玩过' : dayLabel(seconds)

/**
 * 累计玩了多久。
 *
 * 只有一个单位：小时以上不再说分钟——「玩了 37 小时」是这句话要传达的全部，
 * 后面跟一个分钟数并不会让它更准确。不足一小时才说分钟，否则一场刚玩完的
 * 会话会显示成 0。
 */
export function playTimeLabel(seconds: number): string {
  if (seconds < 60) return '不足 1 分钟'
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟`
  return `${Math.floor(seconds / 3600)} 小时`
}
