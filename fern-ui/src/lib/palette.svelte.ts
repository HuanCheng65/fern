/**
 * 命令面板的注册表。
 *
 * 面板的本质不是「一串命令加模糊搜索」，而是**一套带文本界面的寻址系统**：
 * 图形界面的契约是「能做的事都看得见」，而这个契约在两个方向上会破——东西
 * 看得见但在四次点击之外（成本在走，不在找），以及东西太多以至于没有任何
 * 排版能让目标可见。面板用命名替代导航，同时解决这两个。
 *
 * 由此得到的是一套**语法**，不是一张列表：
 *
 *   名词优先   [对象] → 对它能做什么
 *   动词优先   [动词] → 对谁做
 *
 * 两个方向都必须成立，因为人两种想法都会有。落到实现上只有一条规则：
 * **对象有类型，动作声明它接受什么类型的对象。** 这一条替掉了此前散落在
 * 外壳里的全部特判（「有当前实例时才列出启动」之类）。
 *
 * 注册代码写在暴露这个动作的模块里，和那个按钮挨着，而不是集中成一张表。
 * 这样才守得住那条纪律：**面板里出现的每个动作，界面上都有一个看得见的
 * 入口**——面板是加速器，不是功能的藏身处。
 *
 * 两条它不做的事：不显示进度（发起之后交给岛），不承载输入（要填表就跳到
 * 真正的界面去）。后者是防止它长成第二个应用的闸门。
 */

export type SubjectType = 'instance' | 'account' | 'place' | 'project'

/** 一个可以被指名的东西。 */
export interface Subject {
  type: SubjectType
  id: string
  title: string
  /** 右侧的一行小字。用来区分同名的东西，没有就不画。 */
  hint?: string
  /** 生成式封面的种子。没有就画一个类型图标。 */
  seed?: string
  /**
   * 只在下钻里出现，不平铺在顶层。
   *
   * 给那些「不是这个面板的主角」的对象用：一份五个账户的名单会把实例和动作
   * 挤下去，而人打开面板十有八九是要换实例。
   */
  scoped?: boolean
  /** 回车的默认含义：去到它。 */
  run: () => void
}

/** 一件可以做的事。 */
export interface Action {
  id: string
  title: string
  hint?: string
  /** 显示在右边的快捷键。没有就不画，不要为了对齐编一个出来。 */
  keys?: string
  /**
   * 接受什么类型的对象。`'none'` 是不需要宾语的全局动作。
   *
   * 需要宾语时：当前上下文给得出默认宾语就一步到位，给不出就下钻去问。
   */
  accepts: SubjectType | 'none'
  /** 此刻的默认宾语。返回 undefined 表示要问。 */
  subject?: () => Subject | undefined
  run: (subject?: Subject) => void
}

/** 要联网才答得出的那一类来源。 */
export type RemoteSource = (query: string, signal: AbortSignal) => Promise<Subject[]>

const subjectSources: Array<() => Subject[]> = []
const actionSources: Array<() => Action[]> = []
const remoteSources: RemoteSource[] = []

/** 贡献一批对象。在拥有这些对象的模块里调用。 */
export function provides(source: () => Subject[]) {
  subjectSources.push(source)
}

/**
 * 贡献一批要联网才拿得到的对象。
 *
 * 面板必须即时，但「不联网」这条界限画错了：模组在这个应用里就是一等对象，
 * 凭什么排除。真正的约束是——
 *
 *   **面板永远不能让你为「你已经知道自己要什么」的那件事等待。**
 *
 * 这是可满足的，靠两条：本地结果立即渲染；远端结果只**向下追加**，永不重排
 * 已经画出来的部分。所以慢结果不会把你正要按回车的那一行挪走。保证写在这一层
 * 而不是交给每个来源自觉，因为它一旦被破坏就是最难察觉的那种坏。
 */
export function providesRemote(source: RemoteSource) {
  remoteSources.push(source)
}

/** 贡献一批动作。在暴露这些动作的模块里调用。 */
export function commands(source: () => Action[]) {
  actionSources.push(source)
}

export const TYPE_LABEL: Record<SubjectType, string> = {
  instance: '实例',
  account: '账户',
  place: '前往',
  project: '补给',
}

/**
 * 子序列匹配，带质量分。
 *
 * 只判命中与否不够：同一个查询下，一个开头就对上的结果和一个跨了半句才凑齐
 * 的结果不该并列。连续命中和词首命中各自加权，于是「fabopt」在
 * 「Fabulously Optimized」上得分远高于在一段碰巧含有这些字母的描述上。
 *
 * 返回 undefined 表示没命中。
 */
export function score(text: string, query: string): number | undefined {
  if (!query) return 0
  const haystack = text.toLowerCase()
  const needle = query.toLowerCase().replace(/\s+/g, '')
  let index = 0
  let points = 0
  let streak = 0
  for (let position = 0; position < haystack.length && index < needle.length; position += 1) {
    if (haystack[position] !== needle[index]) {
      streak = 0
      continue
    }
    // 词首（开头，或者跟在分隔符后面）说明用户是按词打的，值更多分。
    const previous = position > 0 ? haystack[position - 1]! : ' '
    const atWordStart = position === 0 || /[\s\-_.·/]/.test(previous)
    points += 1 + streak + (atWordStart ? 3 : 0)
    streak += 1
    index += 1
  }
  return index === needle.length ? points : undefined
}

const FRECENCY_KEY = 'fern.palette.frecency'
const HALF_LIFE_DAYS = 14

interface Habit {
  count: number
  at: number
}

/**
 * 用过的东西更容易再被用到。
 *
 * 人要的东西分布极度偏斜且和时间相关，不学习的面板等于逼人永远打同样那几个
 * 字符。半衰期让旧习惯自己让位，而不是一直压着新的。
 *
 * 存在 localStorage 而不是 settings.json：这是一份使用习惯的缓存，不是配置，
 * 丢了没有任何损失，也不该跟着主题码分享出去。
 */
function readHabits(): Record<string, Habit> {
  try {
    return JSON.parse(localStorage.getItem(FRECENCY_KEY) ?? '{}') as Record<string, Habit>
  } catch {
    return {}
  }
}

let habits = readHabits()

function weight(key: string): number {
  const habit = habits[key]
  if (!habit) return 0
  const days = (Date.now() - habit.at) / 86_400_000
  return Math.log2(1 + habit.count) * Math.pow(0.5, days / HALF_LIFE_DAYS)
}

function remember(key: string) {
  const habit = habits[key]
  habits = { ...habits, [key]: { count: (habit?.count ?? 0) + 1, at: Date.now() } }
  try {
    localStorage.setItem(FRECENCY_KEY, JSON.stringify(habits))
  } catch {
    // 无痕模式：这次生效，下次打开重新开始学。
  }
}

export type Row =
  | { kind: 'subject'; key: string; subject: Subject; points: number }
  | { kind: 'action'; key: string; action: Action; points: number }

/** 下钻时输入框左边挂着的那一枚。 */
export interface Scope {
  /** 只看这一类对象。 */
  type: SubjectType
  label: string
  /** 选中之后交给谁。空表示只是过滤，选中就执行对象自己的默认动作。 */
  action?: Action
}

/** 远端结果慢到值得说一句的门槛。低于它就不必闪一下「搜索中」。 */
const DEBOUNCE = 220

class PaletteStore {
  query = $state('')
  scope = $state<Scope | null>(null)
  cursor = $state(0)
  /** 远端来源正在答。只用来在列表末尾说一句，不阻塞任何东西。 */
  searching = $state(false)

  readonly subjects = $derived(subjectSources.flatMap((source) => source()))
  readonly actions = $derived(actionSources.flatMap((source) => source()))

  /** 远端答回来的那些，连同它们对应的查询——查询变了就作废。 */
  #remote = $state<{ query: string; subjects: Subject[] }>({ query: '', subjects: [] })
  #timer: ReturnType<typeof setTimeout> | undefined
  #abort: AbortController | undefined

  readonly remote = $derived(
    this.#remote.query === this.query.trim() ? this.#remote.subjects : [],
  )

  /**
   * 去问一轮远端。防抖，且只认最后一次的答案。
   *
   * 「作废」而不是真的取消：Tauri 的 invoke 没有取消口子，请求会跑完，我们
   * 只是不采信。这一点必须做到——一个已经不对应当前输入的答案追加进来，比慢
   * 更糟，它会在用户眼皮底下往列表里塞不相干的东西。
   */
  ask() {
    clearTimeout(this.#timer)
    this.#abort?.abort()
    const query = this.query.trim()
    // 下钻的时候不问远端：那时候用户已经在挑一个具体类型的东西了。
    if (!query || this.scope || remoteSources.length === 0) {
      this.searching = false
      this.#remote = { query: '', subjects: [] }
      return
    }
    this.#timer = setTimeout(() => {
      const controller = new AbortController()
      this.#abort = controller
      this.searching = true
      Promise.all(
        remoteSources.map((source) =>
          source(query, controller.signal).catch(() => [] as Subject[]),
        ),
      ).then((batches) => {
        if (controller.signal.aborted) return
        this.searching = false
        this.#remote = { query, subjects: batches.flat() }
      })
    }, DEBOUNCE)
  }

  /**
   * 结果按分数排，组标题只是分隔线。
   *
   * 固定组序好记，但当最佳答案在第三组时它就是错的。可预测性由「同一个查询
   * 永远同一个顺序」保证；真正需要位置稳定的是刚打开还没输入的那一刻，那时
   * 走的是另一条分支。
   */
  readonly rows = $derived.by<Row[]>(() => {
    const query = this.query.trim()
    const scope = this.scope

    if (scope) {
      return this.subjects
        .filter((subject) => subject.type === scope.type)
        .map((subject) => ({
          kind: 'subject' as const,
          key: `${subject.type}:${subject.id}`,
          subject,
          points: score(`${subject.title} ${subject.hint ?? ''}`, query) ?? -1,
        }))
        .filter((row) => row.points >= 0)
        .sort((left, right) => right.points - left.points)
    }

    const rows: Row[] = []
    for (const subject of this.subjects) {
      if (subject.scoped) continue
      const key = `${subject.type}:${subject.id}`
      const points = score(`${subject.title} ${subject.hint ?? ''}`, query)
      if (points === undefined) continue
      rows.push({ kind: 'subject', key, subject, points: points + weight(key) * 4 })
    }
    for (const action of this.actions) {
      const key = `action:${action.id}`
      const points = score(`${action.title} ${action.hint ?? ''}`, query)
      if (points === undefined) continue
      // 动作稍稍让位于对象：面板的主角是东西，动词是对它们做的事。
      rows.push({ kind: 'action', key, action, points: points + weight(key) * 4 - 1 })
    }
    rows.sort((left, right) => right.points - left.points)

    // 远端结果一律排在本地之后，不参与上面的排序。这不是「它们不够重要」，
    // 是那条保证：已经画出来的行不许因为网络回来而移动位置。
    for (const subject of this.remote) {
      rows.push({
        kind: 'subject',
        key: `${subject.type}:${subject.id}`,
        subject,
        points: -1,
      })
    }
    return rows
  })

  open(scope: Scope | null = null) {
    this.query = ''
    this.scope = scope
    this.cursor = 0
    this.ask()
  }

  /** 查询变了就把光标收回第一行，否则它会停在一个已经不存在的位置上。 */
  reset() {
    this.cursor = 0
    this.ask()
  }

  move(delta: number) {
    const total = this.rows.length
    if (total === 0) return
    this.cursor = (this.cursor + delta + total) % total
  }

  /**
   * 执行一行。返回是否该关闭面板——下钻不关。
   *
   * 这是那条语法唯一的落点：动作有默认宾语就一步到位，没有就把自己变成一个
   * scope，回到列表去问「对谁做」。
   */
  run(row: Row): boolean {
    remember(row.key)
    if (row.kind === 'subject') {
      const scope = this.scope
      if (scope?.action) {
        scope.action.run(row.subject)
      } else {
        row.subject.run()
      }
      return true
    }

    const action = row.action
    if (action.accepts === 'none') {
      action.run()
      return true
    }
    const subject = action.subject?.()
    if (subject) {
      action.run(subject)
      return true
    }
    this.open({ type: action.accepts, label: action.title, action })
    return false
  }

  /** Esc 先摘 scope，再关面板。由外向内退。 */
  back(): boolean {
    if (!this.scope) return true
    this.scope = null
    this.query = ''
    this.cursor = 0
    this.ask()
    return false
  }
}

export const palette = new PaletteStore()
