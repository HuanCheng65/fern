/**
 * 诊断给出的那颗按钮，以及它上面写什么。
 *
 * 崩溃分析、启动前预检查和文件对账用的是同一套动作，所以枚举只有这一份。
 * 后端给的是一个封闭枚举，不是自由文本——每一种背后都要有真实的界面动作。
 *
 * **做不了的动作不给按钮。** 后端可能比界面新，那时候宁可只显示一句诊断，也
 * 不要一颗点了没反应的按钮。`label()` 返回空就是没有按钮。
 *
 * 真正去执行的那一半（`perform`）留在产品里：它要装模组、改设置、开目录，
 * 每一件都连着 Tauri。这里只管「这颗按钮该不该有、该写什么」。
 */

export type FixAction =
  | { kind: 'install-mod'; query: string }
  | { kind: 'remove-mod'; file: string }
  | { kind: 'use-java'; major: number }
  | { kind: 'set-memory'; mb: number }
  | { kind: 'open-path'; path: string }
  | { kind: 'open-url'; url: string }
  | { kind: 'restore-mods'; snapshot: string }

/** 这颗按钮上写什么。返回空表示这一条现在做不了，不该有按钮。 */
export function label(action: FixAction | undefined): string {
  if (!action) return ''
  switch (action.kind) {
    case 'install-mod':
      return '去安装'
    case 'remove-mod':
      return '删除'
    case 'restore-mods':
      return '恢复模组'
    default:
      return ''
  }
}

/**
 * 按下去会丢东西的动作，先把后果说出来，等第二次确认。返回空表示不必确认。
 *
 * 在快照页做同样的恢复要经过「选范围、读后果句、点恢复」，诊断卡片上这颗
 * 按钮没有理由更轻率——相同的事情使用相同的方式完成。
 */
export function consequence(action: FixAction | undefined): string {
  if (!action) return ''
  switch (action.kind) {
    case 'restore-mods':
      return '会把模组还原到快照那一刻，之后新装的会被删除；恢复前会自动拍一张快照。'
    default:
      return ''
  }
}

/**
 * 这颗按钮要动的是哪一样东西。
 *
 * 给已经把问题说清楚了的地方用——崩溃面板的标题就是那句诊断，动作那一行再把
 * 同一句话重复一遍是噪音，它该说的是「要删的是这个文件」。
 */
export function target(action: FixAction | undefined): string {
  if (!action) return ''
  switch (action.kind) {
    case 'install-mod':
      return action.query
    case 'remove-mod':
      return action.file
    case 'restore-mods':
      return action.snapshot
    default:
      return ''
  }
}
