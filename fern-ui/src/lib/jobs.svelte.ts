/**
 * 正在跑的耗时任务。
 *
 * 后端每件耗时的事都有一个 id，从头到尾贴在它自己的事件上。这里只是把那条流
 * 收成一张表——**没有人往这里写「我开始了一个任务」**，作业是后端宣告的，
 * `started` 一到卡片就出现。这样就不存在第二个真相来源：作业结束了而界面还挂
 * 着、游戏退了而绿点还在，那类 bug 全是从「界面自己也记一份」来的。
 *
 * 进度是两轴的，这里原样保留：`index/of` 是第几步，字节数是横轴。字节是整个
 * 作业的一本账（几条下载流的合计，单调向上），当前这步内部走了多少用
 * `stageDone/stageTotal` 存的基线做差算出来——`fraction()` 靠它，不必假设
 * 一步一条流。
 *
 * 阶段名（`stage`）只随 `stage` 事件变；`note` 是随做随换的注脚（「读取资源
 * 索引」「拍摄快照」），说完就撤。上一版把注脚当阶段名顶上去、再也不撤，
 * 「下载了三分钟还写着读取资源索引」就是那么来的。
 *
 * 成功的作业直接消失：新实例出现在曲库里、模组出现在列表里，本身就是最好的
 * 完成通知。**只有失败留下**，留到有人处理为止——上一版失败是彻底静默的，
 * 错误存在某个已经被销毁的组件的局部变量里。
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { describe } from './i18n'
import { inTauri } from './instances.svelte'
import { contributes, PRIORITY, type Presence } from './island.svelte'

/** 阶段内一条并行支线：有名字、有此刻的注脚，完成后消失。 */
export interface JobTrack {
  track: number
  label: string
  note: string
}

export interface Job {
  id: string
  title: string
  /** 这件事干在谁身上：实例 id、项目 id，可以都有。 */
  subjects: string[]
  /** 这一步在做什么。只有后端的 step() 能改它。 */
  stage: string
  /** 此刻的注脚。空字符串表示没什么可补充的。 */
  note: string
  index: number
  /** 共几步。0 表示后端还没说——那就别显示分母，不要编一个。 */
  of: number
  /** 整个作业的字节合计（单调向上）。 */
  done: number
  total: number
  speed: number
  /** 进入当前步时合计停在哪，步内进度按增量算。 */
  stageDone: number
  stageTotal: number
  /** 并行支线。顺序执行的作业没有。 */
  tracks: JobTrack[]
  /** 有值就是失败了。失败的作业不会自己消失。 */
  error: string
}

/** 后端的文案：要么是 id 加参数（句子在文案表里），要么是一段现成的文本。 */
type JobText = string | { id: string; params?: Record<string, string> }

type JobEvent =
  | { type: 'started'; payload: { id: string; title: string; subjects: string[] } }
  | { type: 'stage'; payload: { id: string; label: JobText; index: number; of: number } }
  | { type: 'track'; payload: { id: string; track: number; label: JobText } }
  | { type: 'note'; payload: { id: string; track: number; message: JobText } }
  | { type: 'track_done'; payload: { id: string; track: number } }
  | { type: 'bytes'; payload: { id: string; done: number; total: number; speed: number } }
  | { type: 'done'; payload: { id: string; error: string | null } }

type LauncherEvent = { type: 'job'; payload: JobEvent } | { type: string; payload: unknown }

const textOf = (text: JobText) =>
  typeof text === 'string' ? text : describe(text.id, text.params ?? {}).title

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(0)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

/**
 * 两轴合成一个数，给只放得下一个进度条的地方用。
 *
 * 每一步平摊 `1/of`，走完的步数记满，当前这步按**这一步内新增的字节**补一段
 * ——合计是整个作业的账，直接用它会把上一步下完的字节又算一遍。没有字节数的
 * 步（装加载器）在它自己跑完之前就停在原地不动——这是诚实的：那一步内部确实
 * 没有任何可报的进度，假装它在走才是骗人。
 *
 * 后端还没说总步数时返回 undefined，让调用方画不定量的样子。
 */
export function fraction(job: Job): number | undefined {
  if (job.of <= 0) return undefined
  const stageSpan = job.total - job.stageTotal
  const stageGain = Math.max(0, job.done - job.stageDone)
  const within = stageSpan > 0 ? Math.min(1, stageGain / stageSpan) : 0
  return Math.min(1, (job.index - 1 + within) / job.of)
}

/** 机器数：一共多少字节、多快。没有就没有。 */
export function measure(job: Job): string {
  if (job.total <= 0) return ''
  const speed = job.speed > 0 ? ` · ${formatBytes(job.speed)}/s` : ''
  return `${formatBytes(job.done)} / ${formatBytes(job.total)}${speed}`
}

/**
 * 阶段名旁边那一小行该说什么：这一步有字节在动就报数，没有就说注脚。
 *
 * 只做二选一。下载时字节比「检查并下载 N 个文件」有信息量；下载完之后字节
 * 停住了，还挂着一行不动的数字，等于把「卡住没反馈」又演一遍——那时该说话
 * 的是「拍摄快照」「检查模组」这些注脚。
 */
export function aside(job: Job): string {
  return job.done > job.stageDone ? measure(job) : job.note || measure(job)
}

let rehearsals = 0

class JobStore {
  live = $state<Job[]>([])
  /** 失败的，留到有人点掉。 */
  failed = $state<Job[]>([])

  #unlisten: UnlistenFn | undefined

  async connect() {
    if (!inTauri() || this.#unlisten) return
    this.#unlisten = await listen<LauncherEvent>('launcher-event', ({ payload }) => {
      if (payload.type === 'job') this.#onJob(payload.payload as JobEvent)
    })
  }

  disconnect() {
    this.#unlisten?.()
    this.#unlisten = undefined
  }

  /** 这个实例／项目上现在有什么在跑。 */
  forSubject(subject: string): Job | undefined {
    return this.live.find((job) => job.subjects.includes(subject))
  }

  /** 这个实例／项目上有没有失败的事情还没处理。 */
  failureFor(subject: string): Job | undefined {
    return this.failed.find((job) => job.subjects.includes(subject))
  }

  dismiss(id: string) {
    this.failed = this.failed.filter((job) => job.id !== id)
  }

  dismissAll() {
    this.failed = []
  }

  /**
   * 浏览器预览里演一遍。
   *
   * 没有后端就没有事件，岛和进度条会永远是空的——而开发时盯着看的正是它们。
   * 这不是把假数据混进真数据：只有 `inTauri()` 为假时才走得到这里。
   */
  rehearse(title: string, subjects: string[]): Promise<void> {
    const id = `rehearsal-${(rehearsals += 1)}`
    const stages = ['读取版本信息', '补全游戏文件', '准备 Java']
    this.#onJob({ type: 'started', payload: { id, title, subjects } })
    return new Promise((resolve) => {
      let step = 0
      // 字节是一本账：跨步单调向上，和真后端一个口径。
      let done = 0
      const perStage = 300 * 1024 * 1024
      const timer = setInterval(() => {
        if (done % perStage === 0) {
          step += 1
          this.#onJob({
            type: 'stage',
            payload: { id, label: stages[step - 1], index: step, of: stages.length },
          })
        }
        done += perStage / 6
        this.#onJob({
          type: 'bytes',
          payload: { id, done, total: perStage * step, speed: 12 * 1024 * 1024 },
        })
        if (done < perStage * step) return
        if (step < stages.length) return
        clearInterval(timer)
        this.#onJob({ type: 'done', payload: { id, error: null } })
        resolve()
      }, 260)
    })
  }

  #patch(id: string, change: Partial<Job>) {
    const index = this.live.findIndex((job) => job.id === id)
    if (index < 0) return
    this.live[index] = { ...this.live[index], ...change }
  }

  #onJob(event: JobEvent) {
    switch (event.type) {
      case 'started':
        this.live.push({
          id: event.payload.id,
          title: event.payload.title,
          subjects: event.payload.subjects,
          stage: '',
          note: '',
          index: 0,
          of: 0,
          done: 0,
          total: 0,
          speed: 0,
          stageDone: 0,
          stageTotal: 0,
          tracks: [],
          error: '',
        })
        break
      case 'stage': {
        const job = this.live.find((item) => item.id === event.payload.id)
        // 换一步就记下基线，这一步的进度从零算起；上一步的注脚也说完了。
        this.#patch(event.payload.id, {
          stage: textOf(event.payload.label),
          index: event.payload.index,
          of: event.payload.of,
          note: '',
          stageDone: job?.done ?? 0,
          stageTotal: job?.total ?? 0,
        })
        break
      }
      case 'track': {
        const job = this.live.find((item) => item.id === event.payload.id)
        if (!job) break
        this.#patch(event.payload.id, {
          tracks: [
            ...job.tracks,
            { track: event.payload.track, label: textOf(event.payload.label), note: '' },
          ],
        })
        break
      }
      case 'note': {
        const job = this.live.find((item) => item.id === event.payload.id)
        if (!job) break
        const message = textOf(event.payload.message)
        this.#patch(event.payload.id, {
          note: message,
          tracks: job.tracks.map((track) =>
            track.track === event.payload.track ? { ...track, note: message } : track,
          ),
        })
        break
      }
      case 'track_done': {
        const job = this.live.find((item) => item.id === event.payload.id)
        if (!job) break
        // 完成的支线消失，和「成功的作业直接消失」同一条纪律。
        this.#patch(event.payload.id, {
          tracks: job.tracks.filter((track) => track.track !== event.payload.track),
        })
        break
      }
      case 'bytes':
        this.#patch(event.payload.id, {
          done: event.payload.done,
          total: event.payload.total,
          speed: event.payload.speed,
        })
        break
      case 'done': {
        const job = this.live.find((item) => item.id === event.payload.id)
        this.live = this.live.filter((item) => item.id !== event.payload.id)
        // 成功了就没什么可说的：结果自己会出现在该出现的地方。
        if (job && event.payload.error) {
          this.failed = [...this.failed, { ...job, error: event.payload.error }]
        }
        break
      }
    }
  }
}

export const jobs = new JobStore()

/**
 * 一被引用就开始听。
 *
 * 这个 store 是纯投影，它的存在本身就是那份订阅——让外壳记得替它调一次
 * `connect()`，只是给「忘了调」留了一个位置。`connect()` 自己挡住了重复订阅
 * 和浏览器预览。
 */
void jobs.connect()

/** 「第 3 步 / 共 4 步」。总步数还不知道时就不说——分母不该是编的。 */
const steps = (job: Job) => (job.of > 0 ? `第 ${job.index} 步 / 共 ${job.of} 步` : '')

const rows = (job: Job) => [
  {
    id: job.id,
    label: job.title,
    detail: [steps(job), job.stage, job.tracks.length > 0 ? '' : job.note]
      .filter(Boolean)
      .join(' · '),
    meta: measure(job),
    fraction: fraction(job),
  },
  // 并行的支线各占一行：谁在跑、各自到哪一步，一眼看得清。
  ...job.tracks.map((track) => ({
    id: `${job.id}·${track.track}`,
    label: track.label,
    detail: track.note,
  })),
]

/**
 * 岛上关于作业的两句话。
 *
 * 进行中的合成一条（「3 项」），失败的合成另一条——不是每个作业一颗卫星，
 * 否则装个整合包顺手再装两个模组，顶栏就长出一排点。展开之后才逐条列。
 */
contributes((): Presence[] => {
  const out: Presence[] = []

  if (jobs.failed.length > 0) {
    out.push({
      id: 'jobs-failed',
      priority: PRIORITY.alert,
      tone: 'alert',
      label: jobs.failed.length === 1 ? jobs.failed[0].title : `${jobs.failed.length} 项失败`,
      rows: jobs.failed.map((job) => ({
        id: job.id,
        label: job.title,
        detail: job.error,
        dismiss: () => jobs.dismiss(job.id),
      })),
      actions:
        jobs.failed.length > 1 ? [{ label: '全部清掉', run: () => jobs.dismissAll() }] : [],
    })
  }

  if (jobs.live.length > 0) {
    const known = jobs.live.map(fraction).filter((value) => value !== undefined)
    // 只有每一件都说得出进度时才敢报一个总数；有一件说不出，整体就是不定量。
    const overall =
      known.length === jobs.live.length
        ? known.reduce((sum, value) => sum + value, 0) / known.length
        : undefined
    const single = jobs.live.length === 1 ? jobs.live[0] : undefined
    out.push({
      id: 'jobs-live',
      priority: PRIORITY.work,
      tone: 'work',
      label: single ? single.stage || single.title : `${jobs.live.length} 项`,
      fraction: overall,
      rows: jobs.live.flatMap(rows),
      actions: [],
    })
  }

  return out
})
