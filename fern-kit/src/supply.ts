/**
 * 补给里那些和数据源无关的东西。
 *
 * 一条搜索结果长什么样、下载量怎么写成人看的数字——这两件事既不需要 Tauri，
 * 也不需要知道结果是从哪来的。产品那边 `supply.svelte.ts` 从这里取，再补上
 * 请求、分页和缓存；官网只要这个形状。
 */

/** 一条补给搜索结果。字段对着 Modrinth 的搜索返回，只留界面用得上的。 */
export interface Hit {
  projectId: string
  slug: string
  title: string
  description: string
  author: string
  downloads: number
  iconUrl?: string
  categories: string[]
}

/** 下载量写成人看的数字。列表里一行放不下九位数，也没人真的去数那几位。 */
export const compactNumber = (value: number) =>
  value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(1)}M`
    : value >= 1000
      ? `${(value / 1000).toFixed(0)}K`
      : String(value)
