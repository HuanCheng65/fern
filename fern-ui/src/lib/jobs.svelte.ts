/**
 * 正在跑的耗时任务。
 *
 * 后端每件耗时的事都有一个 id，从头到尾贴在它自己的事件上。这里只是把那条流
 * 收成一张表——**没有人往这里写「我开始了一个任务」**，作业是后端宣告的，
 * `started` 一到卡片就出现。这样就不存在第二个真相来源：作业结束了而界面还挂
 * 着、游戏退了而绿点还在，那类 bug 全是从「界面自己也记一份」来的。
 *
 * 进度是两轴的，这里原样保留：`index/of` 是第几步，`done/total` 是这一步内部
 * 的字节数。装加载器那一步根本没有字节数可报——把两轴压成一个百分比就只能靠
 * 编。要单个数字的地方用 `fraction()`，它是从两轴算出来的，不是另编的。
 *
 * 成功的作业直接消失：新实例出现在曲库里、模组出现在列表里，本身就是最好的
 * 完成通知。**只有失败留下**，留到有人处理为止——上一版失败是彻底静默的，
 * 错误存在某个已经被销毁的组件的局部变量里。
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { inTauri } from './instances.svelte'
import { contributes, PRIORITY, type Presence } from './island.svelte'

export interface Job {
  id: string
  title: string
  /** 这件事干在谁身上：实例 id、项目 id，可以都有。 */
  subjects: string[]
  /** 这一步在做什么。 */
  stage: string
  index: number
  /** 共几步。0 表示后端还没说——那就别显示分母，不要编一个。 */
  of: number
  done: number
  total: number
  speed: number
  /** 有值就是失败了。失败的作业不会自己消失。 */
  error: string
}

type JobEvent =
  | { type: 'started'; payload: { id: string; title: string; subjects: string[] } }
  | { type: 'stage'; payload: { id: string; label: string; index: number; of: number } }
  | { type: 'bytes'; payload: { id: string; done: number; total: number; speed: number } }
  | { type: 'done'; payload: { id: string; error: string | null } }

type LauncherEvent = { type: 'job'; payload: JobEvent } | { type: string; payload: unknown }

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(0)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

/**
 * 两轴合成一个数，给只放得下一个进度条的地方用。
 *
 * 每一步平摊 `1/of`，走完的步数记满，当前这步按字节数补一段。没有字节数的步
 * （装加载器）在它自己跑完之前就停在原地不动——这是诚实的：那一步内部确实没有
 * 任何可报的进度，假装它在走才是骗人。
 *
 * 后端还没说总步数时返回 undefined，让调用方画不定量的样子。
 */
export function fraction(job: Job): number | undefined {
  if (job.of <= 0) return undefined
  const within = job.total > 0 ? Math.min(1, job.done / job.total) : 0
  return Math.min(1, (job.index - 1 + within) / job.of)
}

/** 这一步的机器数：多少字节、多快。没有就没有。 */
export function measure(job: Job): string {
  if (job.total <= 0) return ''
  const speed = job.speed > 0 ? ` · ${formatBytes(job.speed)}/s` : ''
  return `${formatBytes(job.done)} / ${formatBytes(job.total)}${speed}`
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
      let done = 0
      const total = 900 * 1024 * 1024
      const timer = setInterval(() => {
        if (done === 0) {
          step += 1
          this.#onJob({
            type: 'stage',
            payload: { id, label: stages[step - 1], index: step, of: stages.length },
          })
        }
        done += total / 6
        this.#onJob({
          type: 'bytes',
          payload: { id, done: Math.min(done, total), total, speed: 12 * 1024 * 1024 },
        })
        if (done < total) return
        done = 0
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
          index: 0,
          of: 0,
          done: 0,
          total: 0,
          speed: 0,
          error: '',
        })
        break
      case 'stage':
        // 换一步就把上一步的字节数清掉，否则新的一步会顶着旧的数字开始。
        this.#patch(event.payload.id, {
          stage: event.payload.label,
          index: event.payload.index,
          of: event.payload.of,
          done: 0,
          total: 0,
          speed: 0,
        })
        break
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

const row = (job: Job) => ({
  id: job.id,
  label: job.title,
  detail: [steps(job), job.stage].filter(Boolean).join(' · '),
  meta: measure(job),
  fraction: fraction(job),
})

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
      rows: jobs.live.map(row),
      actions: [],
    })
  }

  return out
})
