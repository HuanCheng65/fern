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

import { match } from 'pinyin-pro'

export type SubjectType = 'instance' | 'account' | 'place' | 'project' | 'world' | 'server'

/** 一个可以被指名的东西。 */
export interface Subject {
  type: SubjectType
  id: string
  title: string
  /** 标题下的一行小字。用来区分同名的东西，没有就不画。 */
  hint?: string
  /**
   * 参与匹配、但不显示的词：缩写、别名、英文名。
   *
   * 给出它就等于声明**这个对象的 hint 是给眼睛看的**（例如「设置 · 外观」
   * 这种位置面包屑），匹配改用这里的词。默认不给，那时 hint 本身就是内容
   * （版本号、作者、服务器地址），照常参与匹配。
   *
   * 分开这两件事之前，二者都塞在 hint 里：于是关键词直接漏到界面上，而且打
   * 一个分区名会把那一节的每一行都捞出来——它们的 hint 里都写着那个分区。
   */
  terms?: string
  /** 生成式封面的种子。没有就画一个类型图标。 */
  seed?: string
  /**
   * 上一次**真的**用到它是什么时候（毫秒）。
   *
   * 面板自己的那份习惯只记得「在面板里选过什么」，所以一台刚装好的机器上它
   * 一片空白，排序退回注册顺序——而应用早就知道你昨天玩的是哪个实例。有真
   * 数据就别等着重新学一遍。没有这个数字的东西不填，不编。
   */
  seen?: number
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

const openHooks: Array<() => void> = []

/**
 * 面板打开时做一次的事。
 *
 * 给那些「要读一次磁盘才知道」的来源用。放在打开这一刻而不是每次按键：它们
 * 变化的频率是分钟级的，而按键是毫秒级的。
 */
export function onOpen(hook: () => void) {
  openHooks.push(hook)
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
  world: '存档',
  server: '服务器',
}

/**
 * 一段文本和一个查询有多像，**归一化到 0..1**。
 *
 * 一条路走完全部情况：拉丁子序列、直接打汉字、全拼、首字母，以及它们的混搭
 * （`scfserver` 命中「生存服 Server」）。空格和符号在查询里可有可无——人凭
 * 记忆报一个名字的时候，不会记得那里到底有没有连字符。
 *
 * `match` 只回答「命中落在哪几个位置」，像不像由位置算出来，分三个问题：
 *
 *   紧凑   命中是连着的吗、是不是落在词首——`sc` 打中「生存」的两个字头，
 *          比它在一串字母里东捡一个西捡一个更像
 *   靠前   第一个命中离开头多远
 *   占满   命中占了这段文本的多少——同样紧凑时，「圆角」整个就是答案，
 *          而「恢复默认外观」里的那两个字只是它的一部分
 *
 * 归一化不是洁癖：分数要跨字段（标题 vs 别名）和跨长度比较，累加出来的原始
 * 点数做不到这件事——一个十二字的标题命中四个字，点数天然高过一个两字标题
 * 被整个打中，而后者显然更像。
 *
 * 返回 undefined 表示没命中。空查询是「没问」，一律满分，排序交给别的项。
 *
 * 命中的位置一并带出来：这一列信息算出来了就不该扔——界面靠它把标题里被打中
 * 的那几个字加重，于是「打 scf 为什么出来生存服」当场看得见，不必猜。
 */
interface Look {
  points: number
  /** 命中落在 `text` 的哪几个位置。 */
  at: number[]
}

function look(text: string, query: string): Look | undefined {
  const needle = query.trim()
  if (!needle) return { points: 1, at: [] }
  let hit: number[] | null
  try {
    hit = match(text, needle)
  } catch {
    // 拼音那一层出了意外也不该让整个面板空掉，退回最朴素的包含判断。
    const at = text.toLowerCase().indexOf(needle.toLowerCase())
    if (at < 0) return undefined
    return { points: 0.5, at: Array.from(needle, (_, n) => at + n) }
  }
  if (!hit || hit.length === 0) return undefined

  const span = Math.max(text.length, 1)
  let points = 0
  for (let n = 0; n < hit.length; n += 1) {
    const at = hit[n]!
    const joined = n > 0 && hit[n - 1] === at - 1
    points += 1 + (joined ? 1 : 0) + (startsAWord(text, at) ? 1 : 0)
  }
  // 满分是「每个字符都连着上一个、且都落在词首」。第一个字符连不上任何东西。
  const tight = points / (hit.length * 3 - 1)
  const lead = 1 - hit[0]! / span
  const covers = hit.length / span
  return { points: 0.6 * tight + 0.25 * lead + 0.15 * covers, at: hit }
}

/** 副文本的分打个折。命中标题和命中别名不是一回事。 */
const ASIDE = 0.6

/** 动作稍稍让位于对象：面板的主角是东西，动词是对它们做的事。 */
const ACTION = 0.9

/**
 * 副文本按词再各算一遍，取最好的。
 *
 * 别名是一袋并列的词，不是一句话：`bmclapi` 整个就是其中一个词，不该因为它
 * 旁边还写着另外五个而掉分——按整袋算的话，「占了多少」和「离头多远」都会被
 * 那五个词稀释，稀释到比一个跨词凑出来的假命中还低。整袋也照算一次并取大者，
 * 因为首字母本来就是跨词的：`gc` 是 garbage collector 两个词的头。
 *
 * 赢的是哪一个词也带出来：别名不显示在界面上，一行凭一个看不见的词进了列表
 * 是最让人困惑的情况，把那个词补在位置后面就不困惑了。
 */
function alias(text: string, query: string): { points: number; word?: string } | undefined {
  const whole = look(text, query)
  // 一个词命中，整袋必然也命中（词是整袋的子串，子序列匹配对它成立）。所以
  // 整袋都没命中就不必再逐词试——这是每次按键里最省的一刀：绝大多数条目本来
  // 就什么都不匹配，而别名袋子有六七个词。
  if (!whole) return undefined
  let best = { points: whole.points, word: undefined as string | undefined }
  for (const word of text.split(/\s+/)) {
    if (!word) continue
    const one = look(word, query)
    if (one && one.points > best.points) best = { points: one.points, word }
  }
  return best
}

/** 一行的匹配结果：分数、标题上的命中位置、以及挣来这个分的别名。 */
export interface Rank {
  points: number
  /** 只记标题里的位置。别名不在界面上，没有可以加重的字。 */
  at: number[]
  /** 分是别名挣来的时候，是哪个词。 */
  via?: string
}

/**
 * 一个对象和一个**词**有多像，取它最像的那个字段。
 *
 * 字段各匹配各的，而不是拼成一长串——拼起来有两个后果：跨字段的子序列会凭空
 * 成立（`gc` 在「颜色 · accent color」上拼得出来，两个字母分别来自标题和别名），
 * 以及命中落在标题上还是落在别名上一样重，而人心里显然不是这么排的。
 */
function word(title: string, aside: string | undefined, query: string): Rank | undefined {
  const head = look(title, query)
  const tail = aside ? alias(aside, query) : undefined
  const asideward = (tail?.points ?? 0) * ASIDE
  if (!head && !tail) return undefined
  if (head && head.points >= asideward) return { points: head.points, at: head.at }
  return { points: asideward, at: [], via: tail?.word }
}

/**
 * 一个对象和**整个查询**有多像。
 *
 * 空格是「而且」：人报一个名字报不全的时候，会再补一个限定词——「生存 fabric」
 * 「1.20 存档」。每个词各自去找最像它的字段，词与词落在不同字段正是这么写的
 * 意义所在（前一个是名字，后一个是版本行）。全部命中才算命中，分数取平均。
 *
 * 整串仍然先试一次并取大者：空格在拼音那条路上是有意义的分隔符（`sheng cun`、
 * `rl craft`），拆开就把「这两段是连着的」这条信息丢了。
 */
function like(title: string, aside: string | undefined, query: string): Rank | undefined {
  const needle = query.trim()
  if (!needle) return { points: 1, at: [] }
  const whole = word(title, aside, needle)
  const parts = needle.split(/\s+/)
  if (parts.length < 2) return whole

  let sum = 0
  const at: number[] = []
  const via: string[] = []
  for (const part of parts) {
    const hit = word(title, aside, part)
    if (!hit) return whole
    sum += hit.points
    at.push(...hit.at)
    if (hit.via && !via.includes(hit.via)) via.push(hit.via)
  }
  const split: Rank = {
    points: sum / parts.length,
    at: [...new Set(at)].sort((left, right) => left - right),
    via: via.join(' ') || undefined,
  }
  return whole && whole.points >= split.points ? whole : split
}

/** 把一段文本按命中切开，交给界面加重。 */
export function pieces(text: string, at: number[]): { text: string; hit: boolean }[] {
  if (at.length === 0) return [{ text, hit: false }]
  const on = new Set(at)
  const parts: { text: string; hit: boolean }[] = []
  for (let n = 0; n < text.length; n += 1) {
    const hit = on.has(n)
    const last = parts[parts.length - 1]
    if (last && last.hit === hit) last.text += text[n]!
    else parts.push({ text: text[n]!, hit })
  }
  return parts
}

/**
 * 这个位置是不是一个词的开头。
 *
 * 每个汉字都算——中文没有空格，一个字就是一个能被首字母指代的单位。拉丁那边
 * 看前一个字符是不是字母或数字，所以 `Sodium-Extra` 的 `E` 和 `Fabric API`
 * 的 `A` 都算。
 */
function startsAWord(text: string, at: number): boolean {
  if (at === 0) return true
  if (/[\u4e00-\u9fff]/.test(text[at]!)) return true
  return /[^\p{L}\p{N}]/u.test(text[at - 1]!)
}

const FRECENCY_KEY = 'fern.palette.frecency'
const RECALL_KEY = 'fern.palette.recall'
const HALF_LIFE_DAYS = 14

interface Habit {
  count: number
  at: number
}

/** 一次使用值多少：次数取对数（第十次不该是第一次的十倍），再按半衰期折旧。 */
function decay(habit: Habit): number {
  const days = (Date.now() - habit.at) / 86_400_000
  return Math.log2(1 + habit.count) * Math.pow(0.5, days / HALF_LIFE_DAYS)
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

/**
 * `$state` 而不是普通变量：空态列的就是它，执行完一条之后重新打开必须能看见
 * 变化，否则这份学习到用户眼里就是没生效。
 */
let habits = $state(readHabits())

function weight(key: string): number {
  const habit = habits[key]
  return habit ? decay(habit) : 0
}

/**
 * 打过的字 → 在那串字下面选过什么，各选过几次。
 *
 * 上面那份 `habits` 记的是「这个东西我常用」，与查询无关。但人对一个东西的
 * 叫法是**属于那个人的**：打十次 `gc` 选十次垃圾回收器，第十一次打 `gc`，它
 * 仍然只能靠通用分数排队——「gc 对我而言就是它」这条信息没被记住。
 *
 * 记次数而不是记绑定，理由和输入法一样：同一串读音下面本来就可能有好几个候选，
 * 绑定假设只有一个正确答案，答错了就没有退路；次数排得出先后，也允许第二个
 * 候选慢慢爬上来。
 */
type Recall = Record<string, Record<string, Habit>>

function readRecall(): Recall {
  try {
    return JSON.parse(localStorage.getItem(RECALL_KEY) ?? '{}') as Recall
  } catch {
    return {}
  }
}

let recall = $state(readRecall())

/** 记多少：每串字留几个候选、一共留几串。它是习惯的缓存，不是输入历史。 */
const RECALL_PICKS = 5
const RECALL_QUERIES = 120

/** 同一件事的两种写法要落在同一个键上。 */
const asked = (query: string) => query.trim().toLowerCase().replace(/\s+/g, ' ')

/**
 * 此刻这串字下面，每个候选积攒了多少。
 *
 * 一次查询算一遍，不是一行算一遍——这是个查询级的量，挂在行上算就是把同一份
 * 遍历做了几十遍。
 */
function recalled(query: string): Map<string, number> {
  const out = new Map<string, number>()
  const now = asked(query)
  if (!now) return out
  for (const [before, picks] of Object.entries(recall)) {
    // 打到一半也算数：`g` 用得上 `gc` 攒下的那些。但不该像打完那样确定，
    // 所以按打了多少折一下。
    if (!before.startsWith(now)) continue
    const share = now.length / before.length
    for (const [key, habit] of Object.entries(picks)) {
      const value = share * decay(habit)
      if (value > (out.get(key) ?? 0)) out.set(key, value)
    }
  }
  return out
}

/** 用过的东西往上抬多少；在这串字下面选过的，抬得更多。 */
const HABIT = 0.3
const RECALL = 0.5
/** 抬到头为止。学得再久也不该让一个半像的东西压过一个正打中的。 */
const RECALL_CAP = 2

/**
 * 习惯是**乘**上去的，不是加上去的。
 *
 * 加法要求两个量同一个量纲，而它们不是：一个说「有多像」，一个说「用得有多
 * 勤」。加起来的后果是习惯能把一个根本不像的东西顶到第一行——那正是「明明打
 * 的是这几个字，出来的却是别的」。乘法只放大一个已经成立的匹配：常用的东西
 * 在势均力敌时胜出，但救不回一个不像的。
 */
const lift = (key: string, memory: Map<string, number>, seen?: number) =>
  1 +
  // 取大者而不是相加：面板里选过它和真的玩过它，说的是同一件事，记两遍就把
  // 常用的东西抬了两次。
  HABIT * Math.max(weight(key), fresh(seen)) +
  RECALL * Math.min(memory.get(key) ?? 0, RECALL_CAP)

/** 上次真的用到它是多久以前。同一条半衰期，这样它和面板里的习惯可比。 */
function fresh(seen: number | undefined): number {
  if (!seen) return 0
  return Math.pow(0.5, (Date.now() - seen) / 86_400_000 / HALF_LIFE_DAYS)
}

function remember(key: string, query: string) {
  habits = { ...habits, [key]: { count: (habits[key]?.count ?? 0) + 1, at: Date.now() } }
  const now = asked(query)
  if (now) {
    const picks = { ...(recall[now] ?? {}) }
    picks[key] = { count: (picks[key]?.count ?? 0) + 1, at: Date.now() }
    recall = prune({ ...recall, [now]: keepTop(picks, RECALL_PICKS) })
  }
  try {
    localStorage.setItem(FRECENCY_KEY, JSON.stringify(habits))
    localStorage.setItem(RECALL_KEY, JSON.stringify(recall))
  } catch {
    // 无痕模式或者写满了：这次生效，下次打开重新开始学。
  }
}

/** 一串字下面只留分最高的那几个候选。 */
function keepTop(picks: Record<string, Habit>, keep: number): Record<string, Habit> {
  const entries = Object.entries(picks)
  if (entries.length <= keep) return picks
  entries.sort(([, left], [, right]) => decay(right) - decay(left))
  return Object.fromEntries(entries.slice(0, keep))
}

/** 串数封顶，扔掉最久没碰过的那些。一份不断长大的缓存迟早会撑爆 localStorage。 */
function prune(all: Recall): Recall {
  const keys = Object.keys(all)
  if (keys.length <= RECALL_QUERIES) return all
  const freshness = (query: string) =>
    Math.max(...Object.values(all[query] ?? {}).map((habit) => habit.at), 0)
  keys.sort((left, right) => freshness(right) - freshness(left))
  return Object.fromEntries(keys.slice(0, RECALL_QUERIES).map((key) => [key, all[key]!]))
}

/** `at` 与 `via` 是给界面解释这一行为什么在这儿用的，见 `Rank`。 */
export type Row =
  | { kind: 'subject'; key: string; subject: Subject; points: number; at: number[]; via?: string }
  | { kind: 'action'; key: string; action: Action; points: number; at: number[]; via?: string }

/**
 * 下钻时输入框左边挂着的那一枚。两个方向各一种：
 *
 *   subjects  动词优先——「校验哪个实例？」，以及切换器进来时锁定的类型
 *   actions   名词优先——「对这个实例能做什么？」
 *
 * 语法的两半都要有入口。只做前者的话，类型系统建了一半就停在那里：你能找到
 * 动作再挑对象，却不能选中一个对象再问它能干什么，而后者才是人看着列表时更
 * 自然的想法。
 */
export type Scope =
  | { kind: 'subjects'; type: SubjectType; label: string; action?: Action }
  | { kind: 'actions'; subject: Subject; label: string }

/** 空态给几条。它是预测，不是目录——长了就又变成一份要扫的清单。 */
const SUGGESTIONS = 7

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
    // 这串字下面选过什么，一次查询算一遍。
    const memory = recalled(query)

    if (scope?.kind === 'actions') {
      const subject = scope.subject
      return this.actions
        .filter((action) => action.accepts === subject.type)
        .map((action) => this.asRow(action, query, memory))
        .filter((row) => row.points >= 0)
        .sort((left, right) => right.points - left.points)
    }

    if (scope) {
      return this.subjects
        .filter((subject) => subject.type === scope.type)
        .map((subject) => this.asRow(subject, query, memory))
        .filter((row) => row.points >= 0)
        .sort((left, right) => right.points - left.points)
    }

    // 空态不是搜索，是「面板打开了，我还没想好」。这时候该给的是预测而不是
    // 目录——把十二个位置和一排动作倒出来，只会把真正常用的实例挤到第二屏。
    if (!query) return this.suggest()

    const rows: Row[] = []
    for (const subject of this.subjects) {
      if (subject.scoped) continue
      const row = this.asRow(subject, query, memory)
      if (row.points >= 0) rows.push(row)
    }
    for (const action of this.actions) {
      const row = this.asRow(action, query, memory)
      if (row.points >= 0) rows.push(row)
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
        at: [],
      })
    }
    return rows
  })

  /**
   * 打一行的分。没命中记 -1，由调用方筛掉。
   *
   * 三个分支都从这一个口子出来：下钻列对象、下钻列动作、顶层混排。分开写就是
   * 三份会慢慢走散的规则——「切换器里为什么不按常用排」正是那么来的。
   */
  private asRow(item: Subject | Action, query: string, memory: Map<string, number>): Row {
    if ('accepts' in item) {
      const key = `action:${item.id}`
      const hit = like(item.title, item.hint, query)
      // 动作的副文本就是它的 hint，看得见，不必再说一遍。
      return {
        kind: 'action',
        key,
        action: item,
        points: hit ? hit.points * ACTION * lift(key, memory) : -1,
        at: hit?.at ?? [],
      }
    }
    const key = `${item.type}:${item.id}`
    const hit = like(item.title, item.terms ?? item.hint, query)
    return {
      kind: 'subject',
      key,
      subject: item,
      points: hit ? hit.points * lift(key, memory, item.seen) : -1,
      at: hit?.at ?? [],
      // 只有别名看不见才需要交代。命中落在 hint 上时它就写在那一行里，
      // 再补一遍就成了「1.20.1 · fabric · fabric」。
      via: item.terms === undefined ? undefined : hit?.via,
    }
  }

  /**
   * 空态那几条：最近用过的排前面，不够就用实例补齐。
   *
   * 用实例补齐而不是补动作：这个面板的主角是东西，而一台新装的机器上还没有
   * 任何习惯可学，那时列出实例至少是有用的。而「还没学到」不等于「什么都不
   * 知道」：真的玩过的时间也算数，所以第一次打开面板就该是昨天那个实例在前。
   */
  private suggest(): Row[] {
    const pool: Row[] = [
      ...this.subjects
        .filter((subject) => !subject.scoped)
        .map((subject) => ({
          kind: 'subject' as const,
          key: `${subject.type}:${subject.id}`,
          subject,
          points: Math.max(weight(`${subject.type}:${subject.id}`), fresh(subject.seen)),
          at: [],
        })),
      ...this.actions.map((action) => ({
        kind: 'action' as const,
        key: `action:${action.id}`,
        action,
        points: weight(`action:${action.id}`),
        at: [],
      })),
    ]
    const recent = pool
      .filter((row) => row.points > 0)
      .sort((left, right) => right.points - left.points)
      .slice(0, SUGGESTIONS)
    const seen = new Set(recent.map((row) => row.key))
    const instances = pool.filter(
      (row) => row.kind === 'subject' && row.subject.type === 'instance' && !seen.has(row.key),
    )
    return [...recent, ...instances].slice(0, SUGGESTIONS + 3)
  }

  /** 高亮的这一行能被做什么。名词优先那一半的入口。 */
  askActions(row: Row): boolean {
    if (row.kind !== 'subject') return false
    if (!this.actions.some((action) => action.accepts === row.subject.type)) return false
    this.open({ kind: 'actions', subject: row.subject, label: row.subject.title })
    return true
  }

  open(scope: Scope | null = null) {
    this.query = ''
    this.scope = scope
    this.cursor = 0
    this.ask()
    for (const hook of openHooks) hook()
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
    remember(row.key, this.query)
    const scope = this.scope

    // 「对这个对象做什么」——宾语已经定了，这里挑的是动词。
    if (scope?.kind === 'actions' && row.kind === 'action') {
      row.action.run(scope.subject)
      return true
    }

    if (row.kind === 'subject') {
      if (scope?.kind === 'subjects' && scope.action) {
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
    this.open({ kind: 'subjects', type: action.accepts, label: action.title, action })
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
