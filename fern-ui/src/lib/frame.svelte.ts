/**
 * 窗口边框。
 *
 * 「无边框」在三个平台上不是同一件事，所以这里也不做成同一件事：
 *
 *   macOS   保留系统标题栏（titleBarStyle: Overlay + hiddenTitle）。这已经是
 *           无边框了——内容铺满整个窗口，只是交通灯浮在左上角。它换来的是
 *           真的交通灯、真的全屏动画、真的圆角和投影、真的窗口菜单。自己画
 *           三个圆点只会在每一项上都更差。
 *   Windows decorations: false + shadow: true。DWM 会给无边框窗口画圆角和
 *           1px 边线，我们只补三个按钮和拖拽边。
 *   Linux   decorations: false + transparent: true。没有 DWM，圆角和边线由
 *           我们自己画。
 *
 * 平台差异写在 tauri.<platform>.conf.json 里，这边只负责认出自己在哪。
 */

import { getCurrentWindow } from '@tauri-apps/api/window'
import { inTauri } from './instances.svelte'

export type Platform = 'macos' | 'windows' | 'linux'

function detect(): Platform {
  if (typeof navigator === 'undefined') return 'linux'
  const ua = navigator.userAgent
  if (/Mac/i.test(ua)) return 'macos'
  if (/Windows/i.test(ua)) return 'windows'
  return 'linux'
}

export const platform: Platform = detect()

/** 窗口的边框由我们画吗。macOS 交给系统，浏览器预览里根本没有窗口。 */
export const frameless = () => inTauri() && platform !== 'macos'

/** 圆角要不要我们自己画。Windows 上 DWM 已经画了，再画一次会出现双重圆角。 */
export const selfRounded = () => frameless() && platform === 'linux'

class FrameStore {
  maximized = $state(false)

  async connect() {
    if (!frameless()) return
    const win = getCurrentWindow()
    const sync = async () => {
      this.maximized = await win.isMaximized()
    }
    await sync()
    // 双击标题栏、系统快捷键、贴边分屏都会改变最大化状态，只能靠 resize 兜底。
    const unlisten = await win.onResized(() => void sync())
    return () => unlisten()
  }

  minimize() {
    void getCurrentWindow().minimize()
  }

  toggleMaximize() {
    void getCurrentWindow().toggleMaximize()
  }

  close() {
    void getCurrentWindow().close()
  }
}

export const frame = new FrameStore()
