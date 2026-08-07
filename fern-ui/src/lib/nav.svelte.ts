/**
 * 导航（见 docs/UI_DESIGN.md 四）。
 *
 * 整个应用是三层叠加，不是一棵树：
 *
 *   场景层   五个空间上并列的「地方」，横向平移切换，互相之间没有从属关系。
 *   纵深层   每个场景最多向内推一级（实例 → 实例详情，补给 → 项目详情），
 *            详情内部用 tab 横向切，tab 不再嵌套。所以最深三步，任何位置
 *            一次返回就回到场景首页。
 *   浮层     命令面板、设置、账户、任务队列。它们服务于全局状态，不属于
 *            任何场景——设置尤其要抵制做成第六个场景。
 *
 * 场景是可寻址的，且接受上下文参数：模组页点「添加模组」要跳到补给并预设
 * 过滤条件，补给装东西要知道装给谁。⌘K 是这套路由的键盘化身——所有跳转都
 * 能用一条路由表达，导航结构和命令面板同构，一处定义两处使用。
 *
 * 浮层不进地址：它们不是「地方」，回退历史里不该出现「打开过设置」。
 */

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

const isScene = (value: string): value is SceneId => SCENES.some((item) => item.id === value)

/** 同时只开一个。浮层之间不叠罗汉——叠起来就没人知道 Esc 关的是哪一层。 */
export type OverlayId = '' | 'settings' | 'palette' | 'tasks' | 'log'

export type Params = Record<string, string>

function encode(scene: SceneId, detail: string, params: Params) {
  const query = new URLSearchParams(params).toString()
  const path = detail ? `${scene}/${encodeURIComponent(detail)}` : scene
  return `#/${path}${query ? `?${query}` : ''}`
}

class NavStore {
  scene = $state<SceneId>('launch')
  /** 场景内的一级纵深：实例 id、项目 slug、房间号。空串表示在场景首页。 */
  detail = $state('')
  /** 详情页内部的 tab。属于详情，换详情就清掉。 */
  tab = $state('')
  params = $state<Params>({})
  overlay = $state<OverlayId>('')

  /** 镜头往哪边走，决定新场景从哪一侧滑进来。 */
  direction = $state(1)
  /**
   * 场景内容滚到顶栏底下了。顶栏默认完全透明地浮在封面上，只有这时才浮现
   * 毛玻璃——启动场景永不滚动，所以那里的顶栏永远是纯粹悬浮的文字。
   */
  scrolled = $state(false)

  index = $derived(SCENES.findIndex((item) => item.id === this.scene))
  /** 0 = 场景首页，1 = 详情。结构上只有这两级。 */
  depth = $derived(this.detail ? 1 : 0)

  private syncing = false

  private commit() {
    this.syncing = true
    const next = encode(this.scene, this.detail, this.params)
    if (location.hash !== next) location.hash = next
    // hashchange 是异步的，标志要留到它跑完。
    queueMicrotask(() => (this.syncing = false))
  }

  private aim(scene: SceneId) {
    const to = SCENES.findIndex((item) => item.id === scene)
    this.direction = to >= this.index ? 1 : -1
    // 换了地方就重新判断毛玻璃：新场景是从顶上开始的，不该继承上一屏的滚动。
    this.scrolled = false
  }

  /** 横跳到某个场景的首页。 */
  go(scene: SceneId, params: Params = {}) {
    this.aim(scene)
    this.scene = scene
    this.detail = ''
    this.tab = ''
    this.params = params
    this.overlay = ''
    this.commit()
  }

  /** 往当前场景的深处推一级。 */
  open(detail: string, params: Params = {}) {
    this.scrolled = false
    this.detail = detail
    this.tab = ''
    this.params = params
    this.overlay = ''
    this.commit()
  }

  /** 横跳并直接落到详情——跨场景跳转基本都是这一种。 */
  enter(scene: SceneId, detail: string, params: Params = {}) {
    this.aim(scene)
    this.scene = scene
    this.detail = detail
    this.tab = ''
    this.params = params
    this.overlay = ''
    this.commit()
  }

  /** tab 也进地址：详情页的分区是可寻址的，⌘K 才能直接把人送到「模组」那一栏。 */
  setTab(tab: string) {
    this.tab = tab
    this.params = { ...this.params, tab }
    this.commit()
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
    this.params = rest
    this.commit()
  }

  /** 收回场景首页。只有一级纵深，所以返回永远只有这一种去处。 */
  back() {
    if (!this.detail) return
    this.scrolled = false
    this.detail = ''
    this.tab = ''
    this.params = {}
    this.commit()
  }

  /** 左右方向键就是镜头。在详情里时也照走——场景词位置固定，横跳能力保留。 */
  step(delta: number) {
    const next = SCENES[(this.index + delta + SCENES.length) % SCENES.length]!
    this.go(next.id)
  }

  show(overlay: OverlayId) {
    this.overlay = overlay
  }

  toggle(overlay: OverlayId) {
    this.overlay = this.overlay === overlay ? '' : overlay
  }

  dismiss() {
    this.overlay = ''
  }

  /** 地址栏是可以被手改和被外部链接改的，当外部输入读。 */
  read() {
    if (this.syncing) return
    const raw = location.hash.replace(/^#\/?/, '')
    const [path = '', query = ''] = raw.split('?')
    const [scene = '', detail = ''] = path.split('/')
    if (!isScene(scene)) {
      this.commit()
      return
    }
    this.aim(scene)
    this.scene = scene
    this.detail = detail ? decodeURIComponent(detail) : ''
    this.params = Object.fromEntries(new URLSearchParams(query))
    this.tab = this.params.tab ?? ''
  }

  connect() {
    this.read()
    const onHash = () => this.read()
    /**
     * 滚动事件不冒泡，但捕获阶段抓得到——场景内部那些 `.scroll` 容器不必
     * 各自上报。浮层里的列表也会滚，所以只认舞台里的。
     */
    const onScroll = (event: Event) => {
      const target = event.target
      if (!(target instanceof Element) || !target.closest('.stage')) return
      this.scrolled = target.scrollTop > 4
    }
    window.addEventListener('hashchange', onHash)
    window.addEventListener('scroll', onScroll, true)
    return () => {
      window.removeEventListener('hashchange', onHash)
      window.removeEventListener('scroll', onScroll, true)
    }
  }
}

export const nav = new NavStore()
