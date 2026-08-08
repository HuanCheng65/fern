/**
 * 标志的几何（见 docs/fern-brand-system.html）。
 *
 * 标志是 7×9 网格上的一条直角螺线：2.5 圈、圈距一格、茎两格，终点为芽尖。
 * 全部几何由八段走线构成，可以用一段规则完整复述——所以这里存的是走线，
 * 不是一份导出的 SVG 路径。改一格就是改一个数字。
 *
 * 走线的**顺序**同样是规格的一部分：茎底 → 右侧上行 → 顶部左行 → 左侧下行
 * → 底部右行 → 收入内圈 → 芽尖。加载动画沿这条顺序点亮，螺线画完即完成。
 */

type Run = [number, number, number, number]

const RUNS: Run[] = [
  [7, 9, 7, 8],
  [7, 7, 7, 1],
  [6, 1, 1, 1],
  [1, 2, 1, 7],
  [2, 7, 5, 7],
  [5, 6, 5, 3],
  [4, 3, 3, 3],
  [3, 4, 3, 5],
]

/** 16px 场景用的精确版：茎缩短一格，7×8 格，每格恰为 2px。 */
const RUNS_COMPACT: Run[] = RUNS.map((run, index) => (index === 0 ? [7, 8, 7, 8] : run))

/** 把走线展开成格子，按顺序去重。 */
function walk(runs: Run[]): [number, number][] {
  const out: [number, number][] = []
  const seen = new Set<string>()
  for (const [ax, ay, bx, by] of runs) {
    const dx = Math.sign(bx - ax)
    const dy = Math.sign(by - ay)
    let x = ax
    let y = ay
    for (;;) {
      const key = `${x},${y}`
      if (!seen.has(key)) {
        seen.add(key)
        out.push([x, y])
      }
      if (x === bx && y === by) break
      x += dx
      y += dy
    }
  }
  return out
}

export const CELLS = walk(RUNS)
export const CELLS_COMPACT = walk(RUNS_COMPACT)

export interface Rect {
  x: number
  y: number
  size: number
}

/**
 * 摆进一个 100×100 的 viewBox。
 *
 * `pad` 是四周留白，占 box 的比例——保护区规定是标志四周各一格，小尺寸下
 * 留白反而要收掉，所以由调用方给。
 */
export function layout(cells: readonly [number, number][], box = 100, pad = 14): Rect[] {
  const xs = cells.map((cell) => cell[0])
  const ys = cells.map((cell) => cell[1])
  const x0 = Math.min(...xs)
  const y0 = Math.min(...ys)
  const width = Math.max(...xs) - x0 + 1
  const height = Math.max(...ys) - y0 + 1
  const size = (box - 2 * pad) / Math.max(width, height)
  const ox = (box - width * size) / 2
  const oy = (box - height * size) / 2
  return cells.map(([x, y]) => ({
    x: ox + (x - x0) * size,
    y: oy + (y - y0) * size,
    size,
  }))
}
