/**
 * 快照名单要显示的东西。
 *
 * 分界线在「一行长什么样」和「这一行是怎么来的」之间：拍摄、恢复、删除、保留
 * 策略都是产品的事，而「什么时候、为什么在这里、有多大」是一张名单。产品把
 * `Snapshot` 折成 `SnapshotRow`（它认识 reason 的文案表），官网直接写几行。
 */

export interface SnapshotRow {
  id: string
  /** Unix 秒。 */
  takenAt: number
  /** 用户给的名字，没有就是「这一张为什么在这里」。 */
  title: string
  /** 永久保留，不会被保留策略剪掉。 */
  pinned?: boolean
  /** 拍摄时文件仍在变动，内容可能不一致。 */
  inconsistent?: boolean
  /** 右边那一列，例如 `2 个世界 · 184 MB`。 */
  meta?: string
}

const at = (seconds: number) => new Date(seconds * 1000)

/** `14:32`。名单里每一行的第一列，等宽数字，所以对得齐。 */
export const clock = (seconds: number) =>
  at(seconds).toLocaleTimeString('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })

/** 按本地日期分组的键。 */
export const day = (seconds: number) => at(seconds).toLocaleDateString('sv-SE')

/**
 * 一组的标题：今天 / 昨天 / 8月6日 / 2025年8月6日。
 *
 * 最近两天用相对说法，再往前用日期——「三天前」这种说法要用户自己换算成
 * 日期，而人记得住的是「上周六装模组之前」。跨年的补上年份。
 */
export function dayLabel(seconds: number): string {
  const date = at(seconds)
  const today = new Date()
  const midnight = (value: Date) =>
    new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime()
  const days = Math.round((midnight(today) - midnight(date)) / 86_400_000)
  if (days === 0) return '今天'
  if (days === 1) return '昨天'
  return date.toLocaleDateString('zh-CN', {
    year: date.getFullYear() === today.getFullYear() ? undefined : 'numeric',
    month: 'long',
    day: 'numeric',
  })
}

export interface DayGroup {
  key: string
  label: string
  items: SnapshotRow[]
}

/**
 * 按天分组，新的在前。
 *
 * 快照唯一的排序依据是时间，而人找的是「装那个模组之前那一张」——分完组之后，
 * 找的动作从读十二行时间戳变成先落到某一天再挑一行。
 */
export function byDay(rows: SnapshotRow[]): DayGroup[] {
  const groups = new Map<string, DayGroup>()
  for (const row of [...rows].sort((a, b) => b.takenAt - a.takenAt)) {
    const key = day(row.takenAt)
    const group = groups.get(key)
    if (group) group.items.push(row)
    else groups.set(key, { key, label: dayLabel(row.takenAt), items: [row] })
  }
  return [...groups.values()]
}
