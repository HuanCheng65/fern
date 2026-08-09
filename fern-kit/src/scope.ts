/**
 * 运行时改设计系统的变量时，改到哪个元素上。
 *
 * 变量声明在 `.fern` / `.fern-dark` 上，不在 `:root` 上（styles/index.css 讲了
 * 为什么）。代价是注入不能再写 `document.documentElement`：那些声明是直接长在
 * 作用域元素上的，从 html 继承下来的值压不过它——写了不报错，只是不生效。
 *
 * 背景层的实时色板就这么悄悄断过一次：背景一直在变，`--c4` 却钉在出厂那一帧，
 * 于是强调色再也不跟着背景走了。所以这条路只有一条，写在这里，谁要改变量都
 * 从这儿拿元素。
 */

let found: HTMLElement | null = null

export function scopeRoot(): HTMLElement {
  // 这个元素一辈子不换，但热更新会换掉整棵树，所以顺手确认它还挂着。
  if (found?.isConnected) return found
  found = document.querySelector<HTMLElement>('.fern') ?? document.body
  return found
}
