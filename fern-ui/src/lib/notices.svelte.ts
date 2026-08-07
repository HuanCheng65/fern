/**
 * 通知：一件已经发生完的事。
 *
 * 和岛的分工由 docs/UI_DESIGN.md 十二那条判据划开：**岛只承载进行时。** 说得出
 * 「正在……」的东西进岛（补全中、游戏运行中、房间连接中），说不出的进这里
 * ——已安装、已保存、已复制。两者不是同一种东西，也不该长在同一个地方：岛会
 * 一直挂着直到那件事结束，通知说完就走。
 *
 * 上一版没有这一层，于是每一屏各自造一个「成功了」的块：项目详情页在版本列表
 * 上方长出一段「已安装 5 个文件」，设置页在按钮旁边亮一行绿字。它们的共同
 * 问题是**位置绑死在触发它的那个控件旁边**——导航一走就没了，而那件事其实
 * 已经完成了；留在原地又会把布局顶开一块。
 *
 * 三条纪律：
 *
 * 1. **只说结论，不说过程。** 有进度的东西是作业，归岛。
 * 2. **自己消失。** 需要用户处理的东西不该做成通知——那是错误，归岛的
 *    alert 形态，它会一直留到有人点掉。
 * 3. **可以带一个动作，最多一个。** 「已安装 · 查看」。两个以上的选择说明这
 *    件事还没做完，那它就不该是通知。
 */

import { scaled, DURATION } from './motion'

export interface Notice {
  id: number
  /** 一句话结论。 */
  title: string
  /** 补一行细节。可以没有。 */
  detail?: string
  /** ok 说成了、warn 说成了但有话要说。真正的失败不进这里。 */
  tone: 'ok' | 'warn'
  action?: { label: string; run: () => void }
  /** 什么时候自己走。 */
  timer?: ReturnType<typeof setTimeout>
}

/** 停留多久。够读完一句话加一个动作，不够让人觉得它赖着不走。 */
const LINGER = 4200
/** 带动作的多留一会儿——那个动作要有时间被点到。 */
const LINGER_WITH_ACTION = 7000

/** 同时最多挂这么多条。再多就把最老的挤掉：一摞通知本身就是噪声。 */
const STACK = 3

let sequence = 0

class NoticeStore {
  list = $state<Notice[]>([])

  /**
   * 说一句。
   *
   * 返回 id，调用方一般用不上——通知的生命周期不该由调用方管，它说完就走。
   */
  say(notice: Omit<Notice, 'id' | 'tone' | 'timer'> & { tone?: Notice['tone'] }): number {
    const id = (sequence += 1)
    const entry: Notice = { id, tone: 'ok', ...notice }
    this.list = [...this.list.slice(-(STACK - 1)), entry]
    this.#arm(entry)
    return id
  }

  dismiss(id: number) {
    const found = this.list.find((item) => item.id === id)
    if (found?.timer) clearTimeout(found.timer)
    this.list = this.list.filter((item) => item.id !== id)
  }

  /**
   * 鼠标停在上面时不走。
   *
   * 一条正在被读的通知半路消失，比多留两秒烦人得多。
   */
  hold(id: number) {
    const found = this.list.find((item) => item.id === id)
    if (found?.timer) {
      clearTimeout(found.timer)
      found.timer = undefined
    }
  }

  release(id: number) {
    const found = this.list.find((item) => item.id === id)
    if (found) this.#arm(found)
  }

  #arm(notice: Notice) {
    const linger = notice.action ? LINGER_WITH_ACTION : LINGER
    // 动效关掉的时候不该连停留时间也关掉：那是可读性，不是动画。
    notice.timer = setTimeout(() => this.dismiss(notice.id), linger + scaled(DURATION.base))
  }
}

export const notices = new NoticeStore()
