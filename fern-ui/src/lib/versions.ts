/**
 * 八百多个版本怎么摆。
 *
 * 数字是反直觉的，所以先摆在这里（2026-08 实测 Mojang 清单）：
 *
 * ```text
 * 总共 905 个 ── 正式版 102，快照 742，远古 61
 * 正式版分 23 代，最大一代 12 条，中位数 3 条
 * ```
 *
 * **「版本太多」这件事几乎全部来自快照。** 正式版按代分完，每代只有几条，
 * 一列行就摆得下，不需要搜索也不需要跳转条。而找快照的人九成只要最新那个，
 * 剩下的一成知道自己要 `24w14a`——他要的是搜索，不是一棵树。
 *
 * 所以两类分开：正式版按代折叠，快照按时间倒序加搜索。把快照埋进某个正式版
 * 底下是错的——`24w14a` 属于哪一代，从名字上根本看不出来。
 */

export interface VersionOption {
  id: string
  kind: string
  releaseTime: string
}

/** 一代正式版：`1.21` 或 `26.1`。 */
export interface Generation {
  /** 代号，也是这一行的标题。 */
  name: string
  versions: VersionOption[]
}

/**
 * 这个版本属于哪一代——取前两段。
 *
 * 一条规则吃下两代编号：`1.21.11` → `1.21`，2026 年起的 `26.1.2` → `26.1`。
 * 老编号开头那个 `1.` 恒定不变，真正区分世代的是第二段；新编号没有那个开头，
 * 前两段本来就是「年 + 这一年的第几个大版本」。两边都是「前两段」。
 */
export const generationOf = (id: string): string | null => {
  const matched = /^(\d+)\.(\d+)/.exec(id)
  return matched ? `${matched[1]}.${matched[2]}` : null
}

/** 正式版按代分组，新的在前。 */
export const generations = (versions: VersionOption[]): Generation[] => {
  const byName = new Map<string, VersionOption[]>()
  for (const version of versions) {
    if (version.kind !== 'release') continue
    const name = generationOf(version.id)
    if (!name) continue
    const bucket = byName.get(name)
    if (bucket) bucket.push(version)
    else byName.set(name, [version])
  }
  // 清单本身就是新到旧，Map 保插入顺序，所以这里不必再排。
  return [...byName].map(([name, list]) => ({ name, versions: list }))
}

/** 远古那些（`b1.7.3`、`c0.30_01c`）——它们没有代可言，归成一堆。 */
export const ancient = (versions: VersionOption[]): VersionOption[] =>
  versions.filter((version) => version.kind !== 'release' && version.kind !== 'snapshot')

/** 快照，时间倒序（清单本身就是）。 */
export const snapshots = (versions: VersionOption[]): VersionOption[] =>
  versions.filter((version) => version.kind === 'snapshot')

/** 最新的那个正式版。清单是新到旧，所以是第一条。 */
export const newestRelease = (versions: VersionOption[]): VersionOption | undefined =>
  versions.find((version) => version.kind === 'release')

/** 最新的那个快照。 */
export const newestSnapshot = (versions: VersionOption[]): VersionOption | undefined =>
  versions.find((version) => version.kind === 'snapshot')
