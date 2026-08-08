/**
 * 文案。
 *
 * **现在只有一种语言，但文案不再散在后端。** 崩溃诊断和启动前预检查这两条链路
 * 的后端只发 id 和参数（`crash.<规则 id>` / `preflight.<类型>`），句子在这里。
 * 这样做的直接好处不是「支持多语言」，而是：措辞归界面管，改一句话不用动 Rust，
 * 也不会出现后端一份、界面一份、慢慢对不上的两套说法。
 *
 * 界面自己的那些中文还散在各个组件里。**不做一次性的全量搬迁**——那会改动几十
 * 个文件而不带来任何行为变化。新增的文案往这里放，改到哪一屏就顺手搬哪一屏。
 *
 * 加一种语言要做的事：写一份和 `zhCN` 同形状的对象，放进 `CATALOGS`，再让
 * `locale` 可选。剩下的调用点一个都不用改。
 */

import { BACKEND_MESSAGES, type BackendMessage } from './keys'
import { zhCN, type Message } from './zh-CN'

export type { BackendMessage, Message }
export { BACKEND_MESSAGES }

const CATALOGS = { 'zh-CN': zhCN }

export type Locale = keyof typeof CATALOGS

/**
 * 当前语言。
 *
 * 只有一种，所以它现在是个常量。留着这一层是为了让调用点从一开始就写对——
 * 等第二种语言进来时要改的只有这个文件。
 */
export const locale: Locale = 'zh-CN'

const catalog = CATALOGS[locale]

/**
 * 界面自己的文案，按属性访问：`ui.about.notOfficial`。
 *
 * 写错的名字是编译错误，而且读组件时那句话在哪一眼看得见。字符串键那一套
 * （`describe`）留给后端发过来的 id——那些只有运行时才知道。
 */
export const ui = catalog.ui

/** 加载器的显示名。后端传 `fabric` 这样的取值。 */
export const loaderName = (tag: string) =>
  catalog.loader[tag as keyof typeof catalog.loader] ?? tag

/**
 * 把 `{name}` 换成参数。
 *
 * 找不到的占位符原样留着：留着能一眼看出是哪个参数没给，而换成空白只会让句子
 * 读起来缺一块，还查不出原因。
 */
export function format(template: string, args: Record<string, string> = {}) {
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => args[name] ?? whole)
}

/** 后端那条 id 对应的一句话。 */
export function describe(id: string, args: Record<string, string> = {}): Message {
  const message = catalog.backend[id as BackendMessage]
  if (!message) {
    // 只会发生在后端比界面新的构建里。显示 id 而不是空白——看得出是缺文案，
    // 而不是「什么都没有」。
    return { title: id, detail: '' }
  }
  // 加载器名是术语，值本身也要翻译，所以先过一遍。
  const resolved = { ...args }
  for (const key of ['instanceLoader', 'modLoader'] as const) {
    if (resolved[key]) resolved[key] = loaderName(resolved[key])
  }
  return {
    title: format(message.title, resolved),
    detail: format(message.detail, resolved),
  }
}
