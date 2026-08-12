/**
 * 导航（见 docs/frond-design-system.md）。
 *
 * ## 一个地址，一个栈
 *
 * 位置是一条**路径**。第一段决定这是哪个空间——五个场景之一，或者设置那张
 * 模态：
 *
 * ```text
 * ['launch']                        启动
 * ['instances']                     实例
 * ['instances', 'abc123']           某个实例
 * ['settings']                      设置
 * ['settings', 'new', 'microsoft']  设置 › 添加账户 › 微软账户
 * ```
 *
 * 只有三个动作：
 *
 * ```text
 * push(path)   去一个新地方。压栈。
 * back()       弹栈。回到上一步。
 * up()         去掉最后一段。回到上一级，不压栈。
 * ```
 *
 * `go` / `open` / `enter` 是 push 的三个常用形状，不是三条另外的规则。
 *
 * ## 上一版为什么不行
 *
 * 它是「一个地址 + 三个挂件」：`detail`、`tab`、以及浮层里那个 `focus` 字符串。
 * 每个挂件各自带了一份自己的「返回」——`back()` 清 detail、`popFocus()` 切成
 * 两段、设置面板自己 `slice(0, 2)`、外壳里再放一张标题映射表。四份各写各的，
 * 于是各有各的错：
 *
 * - `focus` 实际有四段（`account/list/new/microsoft`），而 `popFocus` 只认三段，
 *   按一次 Esc 会**跳过一级**直接掉回名单。
 * - 每次跳转都写 `location.hash`，包括「返回」——所以返回是**压栈**而不是弹栈。
 *   实测：点五个 tab 加一次返回，历史长六层，再按浏览器后退会回到刚离开的
 *   那一页。
 * - 浮层不进地址，可是 ⌘K 能把人直接送进 `设置 › 账户 › 名单 › 某个账户`。
 *   存在能被送达、却没有地址的地方——刷新即失，后退接不住。
 *
 * ## 什么算一段
 *
 * 一段是**一屏**。横向的东西不占段：详情里的 tab、设置里的分区和行锚点，都是
 * 参数。所以「去掉最后一段」在任何空间里都等于「回到上一级」，不必按空间写
 * 特例——这正是上面那四份 up 消失的原因。
 *
 * ## 名字归各自那一屏
 *
 * 顶栏要写「实例 › 我的世界」，而只有渲染那一屏的人知道它叫什么：实例名是异步
 * 读出来的，项目标题藏在 slug 背后。所以每一屏自己登记 `nav.name('我的世界')`，
 * 外壳不认识实例，也不该有一张表。
 */

import { platform } from './frame.svelte'
import { commands, provides } from 'fern-kit/parts/palette'

export type SceneId = 'launch' | 'instances' | 'supply' | 'multiplayer' | 'wardrobe'

/**
 * 顺序即语义：相邻的两个场景是最热的流转。启动↔实例（换个实例玩）、
 * 实例↔补给（给实例装东西）连成主轴放左边，越往右越接近社交与身份。
 */
export const SCENES: { id: SceneId; label: string }[] = [
  { id: 'launch', label: '启动' },
  { id: 'instances', label: '实例' },
  { id: 'supply', label: '补给' },
  { id: 'multiplayer', label: '联机' },
  { id: 'wardrobe', label: '衣柜' },
]

/**
 * 设置是一张**模态路由**：它有地址、进栈、能被后退接住，但盖在场景上面，
 * 顶栏留在它下面。
 *
 * 文档里那句「设置尤其要抵制做成第六个场景」仍然成立，而它成立的方式不是
 * 不给地址——不给地址只会让它变成一个没人管得住的第六个场景。它盖着而不是
 * 并列着，这才是它不是场景的地方。
 */
export const SETTINGS = 'settings'

export type Space = SceneId | typeof SETTINGS

const isScene = (value: string): value is SceneId => SCENES.some((item) => item.id === value)
const isSpace = (value: string): value is Space => value === SETTINGS || isScene(value)

/**
 * 随手开、随手关的那几层。它们不是「地方」——回退历史里不该出现「打开过命令
 * 面板」——所以不进地址，也不进栈。同时只开一个：叠起来就没人知道 Esc 关的是
 * 哪一层。
 */
export type OverlayId = '' | 'palette' | 'island' | 'log'

export type Params = Record<string, string>

function encode(path: string[], params: Params) {
  const query = new URLSearchParams(params).toString()
  const route = path.map(encodeURIComponent).join('/')
  return `#/${route}${query ? `?${query}` : ''}`
}

function decode(hash: string): { path: string[]; params: Params } | null {
  const raw = hash.replace(/^#\/?/, '')
  const [route = '', query = ''] = raw.split('?')
  const path = route.split('/').filter(Boolean).map(decodeURIComponent)
  if (!path[0] || !isSpace(path[0])) return null
  return { path, params: Object.fromEntries(new URLSearchParams(query)) }
}

/**
 * 设置那张模态内部的位置，写成一个字符串：`分区/行/目标/细节`。
 *
 * 前两段是**锚点不是屏**——分区是左边那列，行是页面里的一行，切它们并没有换
 * 一屏。所以它们落在参数里，只有真正换屏的那几段占路径：
 *
 * ```text
 * java/runtimes         →  #/settings?section=java&row=runtimes
 * account/list/x9f      →  #/settings/x9f?section=account&row=list
 * account/list/new/msa  →  #/settings/new/msa?section=account&row=list
 * ```
 *
 * 分成两个纯函数而不是让设置页自己拼地址：整个应用只有这一处知道这套语法，
 * 上一版就是因为它散在三个地方才分叉的。
 */
export function settingsRoute(focus: string): { path: string[]; params: Params } {
  const [section = '', row = '', ...rest] = focus.split('/').filter(Boolean)
  const params: Params = {}
  if (section) params.section = section
  if (row) params.row = row
  return { path: [SETTINGS, ...rest], params }
}

export function settingsFocus(path: string[], params: Params): string {
  return [params.section ?? '', params.row ?? '', ...path.slice(1)].filter(Boolean).join('/')
}

class NavStore {
  path = $state<string[]>(['launch'])
  params = $state<Params>({})
  overlay = $state<OverlayId>('')

  /**
   * 每一段叫什么，键是它的前缀。
   *
   * 场景词开局就装进去（它们的名字是常量），其余由各自那一屏在挂载时登记。
   * 走过的留着不清：退回来时名字要立刻在那儿，而不是等数据重新读一遍。
   */
  names = $state<Record<string, string>>({
    ...Object.fromEntries(SCENES.map((item) => [item.id, item.label])),
    [SETTINGS]: '设置',
  })

  /**
   * 模态盖住的是哪个场景。
   *
   * 直接从地址深链进设置时（外部链接、刷新）没有「下面那一层」，用启动兜底。
   */
  beneath = $state<SceneId>('launch')

  /** 镜头往哪边走，决定新场景从哪一侧滑进来。 */
  direction = $state(1)
  /**
   * 场景内容滚到顶栏底下了。顶栏默认完全透明地浮在封面上，只有这时才浮现
   * 毛玻璃——启动场景永不滚动，所以那里的顶栏永远是纯粹悬浮的文字。
   */
  scrolled = $state(false)

  space = $derived((this.path[0] ?? 'launch') as Space)
  /** 现在盖着一张模态。 */
  modal = $derived(this.space === SETTINGS)
  /** 顶栏和舞台该画哪个场景。模态盖着时是它下面那一个。 */
  scene = $derived(this.modal ? this.beneath : (this.space as SceneId))
  /** 当前位置的字符串形式，也是 `names` 的键。 */
  here = $derived(this.path.join('/'))

  /** 场景内的一级纵深：实例 id、项目 slug。空串表示在场景首页。 */
  detail = $derived(this.modal ? '' : (this.path[1] ?? ''))
  /** 详情页内部的 tab。它是横向的，所以是参数不是一段。 */
  tab = $derived(this.params.tab ?? '')
  /** 设置模态内部的位置。 */
  focus = $derived(this.modal ? settingsFocus(this.path, this.params) : '')

  index = $derived(SCENES.findIndex((item) => item.id === this.scene))
  /** 0 = 场景首页，1 = 详情。模态自己画自己的面包屑，不占顶栏那一格。 */
  depth = $derived(this.modal ? 0 : this.path.length - 1)

  /** 当前这一屏叫什么。 */
  title = $derived(this.names[this.here] ?? '')
  /** 上一级那一屏叫什么——顶栏箭头旁边写的就是它。 */
  parentTitle = $derived(this.names[this.path.slice(0, -1).join('/')] ?? '')

  /**
   * 我们自己走过的那条路，索引就是 `history.state.n`。
   *
   * 浏览器不肯把它的历史交出来读，所以自己留一份镜像。它只有一个用处：`up()`
   * 要知道「上一级」是不是正好就是「上一步」——是的话就弹栈，而不是再盖一层
   * 一模一样的。
   */
  #trail: string[] = []
  #at = 0

  #write(href: string, mode: 'push' | 'replace') {
    const n = mode === 'push' ? this.#at + 1 : this.#at
    try {
      if (mode === 'push') history.pushState({ n }, '', href)
      else history.replaceState({ n }, '', href)
    } catch {
      // 极少数环境不给 pushState（沙箱里的不透明来源）。退回到直接改 hash：
      // 位置仍然是对的，只是每一步都变成压栈，`back()` 退化成不动。
      if (location.hash !== href) location.hash = href
    }
    this.#at = n
    this.#trail.length = n
    this.#trail[n] = href
  }

  /** 落到一个位置上。只改状态，不碰历史。 */
  #land(path: string[], params: Params) {
    const space = path[0] as Space
    // 记住模态盖住的是谁。已经在模态里的话保持不变——在设置内部换屏不该把
    // 下面那一层换掉。
    if (space === SETTINGS) {
      if (this.path[0] !== SETTINGS && isScene(this.path[0] ?? '')) {
        this.beneath = this.path[0] as SceneId
      }
    } else {
      this.beneath = space as SceneId
    }
    this.path = path
    this.params = params
    // 走到别处就把随手开的那几层收掉：它们服务的是刚才那一屏。
    this.overlay = ''
  }

  /** 镜头朝哪转，以及新的一屏从顶上开始。 */
  #aim(path: string[]) {
    const space = path[0] as Space
    const scene = space === SETTINGS ? this.beneath : (space as SceneId)
    const to = SCENES.findIndex((item) => item.id === scene)
    if (to >= 0) this.direction = to >= this.index ? 1 : -1
    this.scrolled = false
  }

  /** 去一个新地方。压栈。 */
  push(path: string[], params: Params = {}) {
    this.#aim(path)
    this.#land(path, params)
    this.#write(encode(path, params), 'push')
  }

  /**
   * 换掉当前这一步。不压栈。
   *
   * 用在「还是同一个地方，只是换了个样子」：切 tab、用掉一次性参数、以及那些
   * 回头就是死链的跳转（刚建好的实例顶掉新建页、实例被删之后退出它的详情）。
   */
  replace(path: string[], params: Params = {}) {
    this.#aim(path)
    this.#land(path, params)
    this.#write(encode(path, params), 'replace')
  }

  /** 横跳到某个场景的首页。 */
  go(scene: SceneId, params: Params = {}) {
    this.push([scene], params)
  }

  /** 往当前场景的深处推一级。 */
  open(detail: string, params: Params = {}) {
    this.push([this.scene, detail], params)
  }

  /** 横跳并直接落到详情——跨场景跳转基本都是这一种。 */
  enter(scene: SceneId, detail: string, params: Params = {}) {
    this.push(detail ? [scene, detail] : [scene], params)
  }

  /**
   * 上一步。弹真实的栈。
   *
   * 是直接深链进来的（栈里我们一步都没走过）就没得弹：模态退到它盖住的场景，
   * 场景本来就是最外层，不动。
   */
  back() {
    if (this.#at > 0) history.back()
    else if (this.modal) this.replace([this.beneath])
  }

  /**
   * 上一级。去掉最后一段。
   *
   * 不压栈：上一级不是一个新地方。如果上一步正好就是上一级（绝大多数情况——
   * 人是从那儿点进来的），就直接弹栈，让历史保持干净。
   */
  up() {
    if (this.path.length > 1) {
      // tab 属于它上面那一屏，退出去就不作数了。
      const { tab: _leaving, ...rest } = this.params
      const parent = this.path.slice(0, -1)
      const href = encode(parent, rest)
      if (this.#at > 0 && this.#trail[this.#at - 1] === href) history.back()
      else this.replace(parent, rest)
      return
    }
    // 已经在这个空间的第一屏。模态就是关掉，场景没有更上一级。
    if (this.modal) this.back()
  }

  /** 左右方向键就是镜头。在详情里时也照走——场景词位置固定，横跳能力保留。 */
  step(delta: number) {
    const next = SCENES[(this.index + delta + SCENES.length) % SCENES.length]!
    this.go(next.id)
  }

  /** tab 也进地址：⌘K 才能直接把人送到「模组」那一栏。它是横向的，所以不压栈。 */
  setTab(tab: string) {
    this.replace(this.path, { ...this.params, tab })
  }

  /**
   * 用掉一个一次性参数。
   *
   * 跨场景跳转带来的参数（「为这个实例找东西」）应该只生效一次。留在地址里的话，
   * 用户随后改了选择，下一次重新求值又会被它盖回去。
   */
  consume(key: string) {
    if (!(key in this.params)) return
    const { [key]: _dropped, ...rest } = this.params
    this.replace(this.path, rest)
  }

  /** 登记当前这一屏叫什么。名字只有渲染它的那一屏知道。 */
  name(title: string) {
    if (!title || this.names[this.here] === title) return
    this.names = { ...this.names, [this.here]: title }
  }

  /**
   * 打开设置，并落在它内部的某个位置。
   *
   * 已经在设置里而且没往深处走，就是换一节或者退一屏——那不是一个新地方，
   * 不该在栈里再压一层。
   */
  settings(focus = '') {
    const { path, params } = settingsRoute(focus)
    if (this.modal && path.length <= this.path.length) this.replace(path, params)
    else this.push(path, params)
  }

  toggleSettings(focus = '') {
    if (this.modal) this.back()
    else this.settings(focus)
  }

  show(overlay: OverlayId) {
    this.overlay = overlay
  }

  toggle(overlay: OverlayId) {
    this.overlay = this.overlay === overlay ? '' : overlay
  }

  /**
   * 收掉随手开的那一层。
   *
   * 带上 `which` 就是「收掉我这一层」：只有它还开着的时候才收。命令面板需要
   * 这个——它执行的动作有一半是把人送到别处去的，而面板紧接着要关掉自己。
   * 无条件清空的话，刚打开的日志会在同一帧里被关掉。
   */
  dismiss(which?: OverlayId) {
    if (which && this.overlay !== which) return
    this.overlay = ''
  }

  /** 地址栏是可以被手改和被外部链接改的，当外部输入读。 */
  #read() {
    const here = decode(location.hash)
    if (!here) {
      // 读不懂就把当前位置写回去，别把人留在一个不存在的地址上。
      this.#write(encode(this.path, this.params), 'replace')
      return
    }
    const href = encode(here.path, here.params)
    // 后退时 popstate 和 hashchange 会各来一次，第二次什么都不用做。
    if (href === encode(this.path, this.params) && this.#trail[this.#at] === href) return
    const state = history.state as { n?: number } | null
    this.#at = typeof state?.n === 'number' ? state.n : 0
    this.#trail[this.#at] = href
    this.#aim(here.path)
    this.#land(here.path, here.params)
  }

  connect() {
    this.#read()
    // 第一帧要在栈里占住位置，否则第一次 back 会走出这个应用。
    this.#write(encode(this.path, this.params), 'replace')

    const onPop = () => this.#read()
    /**
     * 滚动事件不冒泡，但捕获阶段抓得到——布局不必各自上报。
     *
     * 只认**标了 `data-page-scroll` 的那一个容器**，不是舞台里的任何滚动条。
     * 一开始写的是「在舞台里就算」，于是模组列表、图库、筛选栏随便滚一下都会
     * 让顶栏浮出毛玻璃——毛玻璃回答的是「有内容贴到顶栏底下了吗」，那是页面
     * 主滚动容器的事，页面内部某个小列表滚到哪跟顶栏没关系。
     */
    const onScroll = (event: Event) => {
      const target = event.target
      if (!(target instanceof Element) || !target.matches('[data-page-scroll]')) return
      this.scrolled = target.scrollTop > 4
    }
    window.addEventListener('popstate', onPop)
    window.addEventListener('hashchange', onPop)
    window.addEventListener('scroll', onScroll, true)
    return () => {
      window.removeEventListener('popstate', onPop)
      window.removeEventListener('hashchange', onPop)
      window.removeEventListener('scroll', onScroll, true)
    }
  }
}

export const nav = new NavStore()

/**
 * 路由表本身就是一批对象：凡是能被寻址的地方都自动可搜。
 *
 * 设置的分区和每一行由设置页自己贡献（lib/settings-catalog.ts）——它们的
 * 标题就在那张表里，让导航层再抄一份没有意义。
 */
provides(() => [
  ...SCENES.filter((item) => item.id !== nav.scene).map((item) => ({
    type: 'place' as const,
    id: item.id,
    title: item.label,
    run: () => nav.go(item.id),
  })),
])

/** 只有一条：设置有快捷键，值得在面板里带着它出现一次。 */
commands(() => [
  {
    id: 'settings.open',
    title: '打开设置',
    keys: platform === 'macos' ? '⌘ ,' : 'Ctrl ,',
    accepts: 'none',
    run: () => nav.settings(),
  },
])
