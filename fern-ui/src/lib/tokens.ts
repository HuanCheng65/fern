/**
 * 运行时改 token 的时候，改到哪个元素上。
 *
 * token 定义在 `.fern-app` 上而不是 `:root` 上（fern-kit/src/tokens.css）：
 * 官网只把界面嵌进页面的一块里，不能把整张纸染黑。产品这边这个类挂在 body 上。
 *
 * 代价是注入不能再写 `document.documentElement`。那份定义是直接长在
 * `.fern-app` 这个元素上的，从 html 继承下来的值压不过它——写了不报错，
 * 只是不生效。背景层的实时色板就这么悄悄断过一次：背景一直在变，
 * `--c4` 却钉在出厂那一帧，于是强调色再也不跟着背景走了。
 *
 * 所以运行时要改 token，只有一条路：改到带 `.fern-app` 的那个元素上。
 */

let found: HTMLElement | null = null

export function tokenRoot(): HTMLElement {
  // 这个元素一辈子不换，但热更新会换掉整棵树，所以顺手确认它还挂着。
  if (found?.isConnected) return found
  found = document.querySelector<HTMLElement>('.fern-app') ?? document.body
  return found
}
