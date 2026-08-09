/**
 * Java 运行时在界面上的说法。
 *
 * 只此一份。之前实例设置和设置页各写了一个描述函数——一个说「Java 21 · 厂商 ·
 * JDK · 由 Fern 下载 · 非原生架构」，另一个说「厂商 · JDK · 系统自带 · 180 MB」。
 * 同一个对象两套渲染，两边迟早说得不一样，而没有任何东西会报错。
 *
 * 两处的职责分工：**设置页回答「这台机器上有什么」，实例设置回答「这一个用哪
 * 一份」。** 前者是库存与维护，后者是一次选择——而绝大多数人不该做这个选择。
 */

export interface JavaRuntime {
  path: string
  home: string
  major: number
  version: string
  vendor: string
  arch: string
  /** 由 Fern 下载并管理，删得掉。 */
  managed: boolean
  /** 用户手动登记的位置，只能取消登记，不动磁盘。 */
  added: boolean
  image: 'jdk' | 'jre'
  /** 与启动器同架构。不是的话能跑，但明显更慢。 */
  native: boolean
  sizeBytes: number
}

/** 按大版本分的一组，以及这一组里「自动」会挑中的那一份。 */
export interface JavaGroup {
  major: number
  requiredBy: string[]
  runtimes: JavaRuntime[]
  /** 会被选中的那一份的 home。这一组一份都没装时是 null。 */
  preferred: string | null
}

export const megabytes = (bytes: number) =>
  bytes > 0 ? `${Math.round(bytes / (1024 * 1024))} MB` : ''

/**
 * 一份安装的自我介绍。
 *
 * 版本号由调用方当标题写，所以这里不重复它——这一行只回答「它是哪一份」：
 * 谁发行的、是套件还是运行时、从哪儿来的、多大。
 *
 * 路径不在里面。它是最长、最没有区分度的一段，只有两份别的都一样时才用得上，
 * 所以它属于档案页，不属于名单。
 */
export const javaLabel = (runtime: JavaRuntime) =>
  [
    runtime.vendor || '未知发行版',
    runtime.image === 'jdk' ? 'JDK' : 'JRE',
    runtime.managed ? '由 Fern 下载' : runtime.added ? '手动添加' : '系统自带',
    megabytes(runtime.sizeBytes),
  ]
    .filter(Boolean)
    .join(' · ')

/**
 * 为什么这一份不能用在这个实例上。
 *
 * 能用就返回空串。已知会失败的选项不该和好选项并排摆着让人挑——它得说出自己
 * 为什么不行，否则用户选了它，然后由预检查再来骂一次。
 */
export function javaMismatch(
  runtime: JavaRuntime,
  requirement: { minimum: number; maximum: number | null },
): string {
  if (runtime.major < requirement.minimum) return `这个版本要求 Java ${requirement.minimum} 或更高`
  if (requirement.maximum !== null && runtime.major > requirement.maximum) {
    return `这个版本最高支持到 Java ${requirement.maximum}`
  }
  return ''
}
