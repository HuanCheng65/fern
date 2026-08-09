<script lang="ts">
  /**
   * 实例设置——实例详情页的一个 tab。
   *
   * 它不是浮层：这些开关属于某一个实例，只有站在那个实例的页面上才有意义。
   * 从全局浮层里改「这个实例的内存」，改完还得自己记住刚才改的是谁。
   *
   * 这一屏只放「这个实例和别的实例不一样」的东西。全局的偏好在设置页，向导
   * 里问过的更不该在这儿再问一遍。
   *
   * 每一项都默认「跟随」或「自动」，而且那一档要把结果写出来——「自动」两个
   * 字本身不解释任何事情，用户看到「自动 · 4096 MB」才知道要不要动它。
   * 这是文档 §4.3 和 §6.1 的同一条要求：这一层平时该是隐形的，只有想接管的
   * 人才需要看见它。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ChevronRight } from 'lucide-svelte'
  import AccountFace from './AccountFace.svelte'
  import Choice from './Choice.svelte'
  import Loading from './Loading.svelte'
  import MemoryMeter from './MemoryMeter.svelte'
  import { accounts, originOf } from '../lib/accounts.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'
  import { javaLabel, javaMismatch, type JavaRuntime } from '../lib/java'
  import { nav } from '../lib/nav.svelte'
  import { preflight } from '../lib/preflight.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  /** 一次分配的理由。`topic` 决定它是依据、实测还是约束。 */
  interface ExplanationItem {
    topic: 'basis' | 'history' | 'limit'
    text: string
  }

  interface AllocationDecision {
    xmxMb: number
    source: 'manual' | 'userJvmArgs' | 'adaptive' | 'static'
    gc: 'untouched' | 'zgc' | 'g1'
    explanation: ExplanationItem[]
    arguments: string[]
    tight: boolean
  }

  /** 真跑出来的那几个数。尺上的刻度要的是数，不是句子。 */
  interface MemoryHistory {
    sessions: number
    lastPeakMb: number
    liveSetMb: number
    /** 只有算法说得出的那句结论（「水位健康，维持 8 GB」）。 */
    note: string
  }

  interface InstanceRuntime {
    automaticMemoryMb: number
    allocation: AllocationDecision
    /** 历史不够就是 null，那时不画实测刻度。 */
    measured: MemoryHistory | null
    physicalMemoryMb: number
    requirement: { minimum: number; maximum: number | null }
    java: JavaRuntime | null
    modsCount: number
    /** 全局默认。「跟随全局」四个字本身不解释任何事，得说得出它是什么。 */
    defaults: {
      memoryCeilingMb: number | null
      garbageCollector: GcChoice | null
      resolution: { width: number; height: number } | null
      jvmArguments: string
    }
    memoryCeilingMb: number
  }

  type GcChoice = 'auto' | 'g1' | 'z'

  interface InstanceSettings {
    javaPath: string | null
    maxMemoryMb: number | null
    resolution: { width: number; height: number } | null
    garbageCollector: GcChoice | null
    processPriority: 'low' | 'normal' | 'high' | null
  }

  interface Props {
    instanceId: string
    instanceName: string
    /** 实例没了或者变成了另一个，页面得离开这里。 */
    ongone: (replacement?: string) => void
  }

  let { instanceId, instanceName, ongone }: Props = $props()

  let renamed = $state('')
  let confirmingDelete = $state(false)
  let managing = $state(false)

  // 改名成功后父组件会传入新名字，这时候输入框该跟着走。
  $effect(() => {
    renamed = instanceName
  })

  let runtime = $state<InstanceRuntime | null>(null)
  let runtimes = $state<JavaRuntime[]>([])
  let settings = $state<InstanceSettings>({
    javaPath: null,
    maxMemoryMb: null,
    resolution: null,
    garbageCollector: null,
    processPriority: null,
  })
  /** 高级项默认收起：绝大多数人不该看到它们。 */
  let advanced = $state(false)
  let loading = $state(true)
  let error = $state('')

  /**
   * 滑杆的上限就是设置里那条线，和后端封顶用的是同一个数。
   *
   * 所以这根滑杆推到头之后没有旁路——想给更多就是想多分一点机器给游戏，
   * 该去改那条线，而不是在这里绕过它。
   */
  const ceiling = $derived(runtime?.memoryCeilingMb ?? 4096)
  /** 全局选的那个回收器，「跟随全局」要说得出是哪一个。 */
  const globalGc = $derived<GcChoice>(runtime?.defaults.garbageCollector ?? 'auto')
  const GC_LABEL: Record<GcChoice, string> = { auto: '自动', g1: 'G1', z: 'ZGC' }
  const memoryAuto = $derived(settings.maxMemoryMb === null)
  const memoryValue = $derived(settings.maxMemoryMb ?? runtime?.automaticMemoryMb ?? 2048)

  /**
   * 身份只有两档：不钉住就跟着当前账户走，钉住了就永远是那一个。
   *
   * 中间不再有「记住上次」——它和「固定」只在一条路径上不同（从别处直接启动
   * 这个实例时），而两档在界面上分不出来的东西不值得占一档。
   */
  const pinned = $derived(instances.list.find((item) => item.id === instanceId)?.accountId ?? null)


  /** MB 变成一句话。整数不带小数点——`8 GB` 比 `8.0 GB` 更像一个决定。 */
  const gigabytes = (mb: number) => {
    const value = mb / 1024
    return Math.abs(value - Math.round(value)) < 0.05
      ? `${Math.round(value)} GB`
      : `${value.toFixed(1)} GB`
  }

  /**
   * 解释按 topic 各自落位，不再串成一长条。
   *
   * 「依据」是算法看了什么，「约束」是什么东西挡住了它——两类的语气和去处都
   * 不同。而 `history` 那一类不在这里出：实测的数字变成了尺上的刻度，那句结论
   * 由 `measured.note` 给，一件事只说一遍。
   */
  const explanation = (topic: 'basis' | 'limit') =>
    (runtime?.allocation.explanation ?? [])
      .filter((item) => item.topic === topic)
      .map((item) => item.text)
      .join('、')
  const basis = $derived(explanation('basis'))
  const limits = $derived(explanation('limit'))

  /** 参数里已经写死了 -Xmx：这时候后端报的分配是 0，不能拿它当一个值显示。 */
  const byArguments = $derived(runtime?.allocation.source === 'userJvmArgs')
  const automaticMb = $derived(runtime?.automaticMemoryMb ?? 2048)
  /** 尺上那条线现在在哪。自动时是算出来的那份，手动时是填的那个。 */
  const shownMb = $derived(
    memoryAuto ? (runtime?.allocation.xmxMb ?? automaticMb) : memoryValue,
  )

  /**
   * 尺上的幽灵刻度。
   *
   * 「上次峰值」是一个事实，什么时候都成立；「自动会给多少」只在你已经接管了
   * 的时候才有意义——自动那一档里，填充的边缘本身就是它。
   *
   * 没有历史就没有那道刻度。读不到就不画。
   */
  const memoryMarks = $derived(
    [
      runtime?.measured && {
        at: runtime.measured.lastPeakMb,
        label: `上次峰值 ${gigabytes(runtime.measured.lastPeakMb)}`,
      },
      !memoryAuto && { at: automaticMb, label: `自动 ${gigabytes(automaticMb)}` },
    ].filter((mark): mark is { at: number; label: string } => Boolean(mark)),
  )

  /**
   * Java 这一节和内存对称：默认只有一行结论，想接管的人才展开那张单子。
   *
   * 上一版把这台机器上**所有** Java 平铺成一列单选，于是一个要 21+ 的实例上，
   * Java 8 和 Java 21 是并排的两个选项——选了前者必定起不来，然后预检查再来
   * 骂一次。已知会失败的选项不该和好选项长得一样。
   */
  let picking = $state(false)
  let showUnfit = $state(false)
  let installing = $state(false)

  const requirement = $derived(runtime?.requirement ?? { minimum: 8, maximum: null })
  const javaAuto = $derived(settings.javaPath === null)
  /** 现在真正会被用上的那一份。自动时由后端选，手动时是钉住的那条路径。 */
  const chosen = $derived(
    javaAuto ? runtime?.java : runtimes.find((item) => item.path === settings.javaPath),
  )
  const fitting = $derived(runtimes.filter((item) => !javaMismatch(item, requirement)))
  const unfit = $derived(runtimes.filter((item) => javaMismatch(item, requirement)))

  async function installJava() {
    installing = true
    try {
      await invoke('install_java', {
        major: requirement.minimum,
        title: `安装 Java ${requirement.minimum}`,
        subjects: [`java-${requirement.minimum}`],
      })
      await setJava(null)
    } catch (cause) {
      error = String(cause)
    } finally {
      installing = false
    }
  }

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    try {
      const [info, list, profiles] = await Promise.all([
        invoke<InstanceRuntime>('instance_runtime', { instanceId }),
        invoke<JavaRuntime[]>('list_java_runtimes'),
        invoke<{ id: string; settings: InstanceSettings }[]>('list_instances'),
      ])
      runtime = info
      runtimes = list
      const mine = profiles.find((item) => item.id === instanceId)
      if (mine?.settings) settings = { ...settings, ...mine.settings }
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  /** 每次改动直接落盘。设置面板没有「保存」键——改了就是改了。 */
  async function persist() {
    if (!inTauri()) return
    try {
      await invoke('update_instance_settings', { instanceId, settings })
      error = ''
    } catch (cause) {
      error = String(cause)
    }
  }

  /**
   * 拖尺的时候这里每一帧都会来一次，不能每一帧都写一次盘。
   *
   * 界面立刻跟手（改的是本地那份状态），落盘等手停下来。松手之前就算窗口被关掉，
   * 丢的也只是最后二百毫秒里的一次微调。
   */
  let settling: ReturnType<typeof setTimeout> | undefined
  function setMemory(value: number | null) {
    settings.maxMemoryMb = value
    clearTimeout(settling)
    settling = setTimeout(() => void persist(), 200)
  }

  /**
   * 换 Java 会改变别处正在说的话，所以改完要把那些话作废。
   *
   * 「自动」那一档写着算出来的结果，预检查里那一条说的是「这个实例会用 Java
   * 21，而某个模组要 25」——两句都基于旧的选择。不重算，用户改完只会看到界面
   * 一动不动，然后以为没生效。
   */
  async function setJava(path: string | null) {
    settings.javaPath = path
    await persist()
    preflight.refresh(instanceId)
    await load()
  }

  async function rename() {
    const name = renamed.trim()
    if (!name || name === instanceName) return
    try {
      await instances.rename(instanceId, name)
      error = ''
    } catch (cause) {
      error = String(cause)
    }
  }

  async function duplicate() {
    try {
      const copy = await instances.duplicate(instanceId, `${instanceName} 副本`)
      ongone(copy)
    } catch (cause) {
      error = String(cause)
    }
  }

  async function remove() {
    try {
      await instances.remove(instanceId)
      ongone()
    } catch (cause) {
      error = String(cause)
    }
  }

  void load()
</script>

{#if loading}
  <Loading note="读取实例信息" />
{:else}
  <div class="body">
      <!--
        身份放在第一节：这一屏其余几项都是机器的事，只有这一项是人的事。

        默认「跟随当前账户」，钉住是一次明确的表态——启动本身不再悄悄写它。
        真正要钉的场景很具体：某个私服要用那个站的号，而别处照常用正版。
      -->
      <section>
        <!-- 右边原本有一句「跟着设置里的当前账户变」：下面那一档自己写着
             「跟随当前账户 · 当前是 Steve」，同一件事说两遍。 -->
        <div class="row-head">
          <span class="label">启动身份</span>
        </div>
        <div class="choices">
          <button class="pick" class:on={pinned === null} onclick={() => void instances.setAccount(instanceId, null)}>
            <strong>跟随当前账户</strong>
            <small class="t-mono">
              {accounts.active ? `当前是 ${accounts.active.playerName}` : '尚未添加账户'}
            </small>
          </button>
          {#each accounts.list as account (account.id)}
            <button
              class="pick face-row"
              class:on={pinned === account.id}
              onclick={() => void instances.setAccount(instanceId, account.id)}
            >
              <AccountFace {account} size={26} />
              <span>
                <strong>{account.playerName}</strong>
                <!-- 名单里永远写全出处：同名的正版号和离线号只差这一截。 -->
                <small class="t-mono">{originOf(account)}</small>
              </span>
            </button>
          {/each}
        </div>
      </section>

      <!--
        内存是一根尺，不是三行字（设计文档 §8）。分配、上限、物理内存本来就是
        三个嵌套的区间，画出来一眼就完；判断依据仍然摊开，只是退到尺下面一行。
      -->
      <section>
        <div class="row-head">
          <span class="label">内存</span>
          <span class="t-mono amount">
            {gigabytes(shownMb)}{#if memoryAuto}<small>自动</small>{/if}
          </span>
        </div>

        {#if byArguments}
          <!--
            用户自己在参数里写了 -Xmx。那时候我们一个字都不该插嘴，更不该画一根
            推不动的尺——上一版这里会显示「自动 · 0 GB」，因为后端在这种情况下
            报的分配就是 0。
          -->
          <p class="reason">
            堆大小由额外 JVM 参数中的 <code class="t-mono">-Xmx</code> 决定。
            <Button variant="link" onclick={() => nav.show('settings', 'game/jvm')}>
              前往设置
            </Button>
          </p>
        {:else}
          <MemoryMeter
            label="内存"
            physicalMb={runtime?.physicalMemoryMb ?? 0}
            ceilingMb={ceiling}
            valueMb={shownMb}
            marks={memoryMarks}
            onchange={memoryAuto ? undefined : setMemory}
            onceiling={() => nav.show('settings', 'game/memory')}
          />

          {#if memoryAuto && basis}<p class="reason t-quiet">{basis}</p>{/if}
          {#if memoryAuto && runtime?.measured}
            <p class="reason t-quiet">{runtime.measured.note}</p>
          {/if}
          {#if limits}<p class="reason t-quiet">{limits}</p>{/if}

          <!-- 左边原本有一句「按这个实例的内容与实测用量决定」：数字旁边的
               「自动」和这颗按钮已经把状态说完了。 -->
          <div class="row-foot end">
            <Button variant="link" onclick={() => setMemory(memoryAuto ? shownMb : null)}>
              {memoryAuto ? '手动指定' : `恢复自动（${gigabytes(automaticMb)}）`}
            </Button>
          </div>
        {/if}
      </section>

      <section>
        <div class="row-head">
          <span class="label">Java</span>
          <span class="t-mono amount">
            {chosen ? `Java ${chosen.major}` : '将自动下载'}{#if javaAuto}<small>自动</small>{/if}
          </span>
        </div>
        <p class="reason t-quiet">
          {#if chosen}
            {chosen.version} · {javaLabel(chosen)}
          {:else}
            这台机器上没有满足要求的版本，启动时会自动下载 Java {requirement.minimum}。
          {/if}
        </p>
        {#if chosen && !chosen.native}
          <!-- 能跑，但明显更慢，而这一点在别的任何地方都看不出来。 -->
          <p class="reason warn">{chosen.arch} 版本，与本机架构不一致，性能会下降</p>
        {/if}

        <div class="row-foot">
          <span class="t-quiet">
            这个版本要求 Java {requirement.minimum}{requirement.maximum
              ? `–${requirement.maximum}`
              : ' 或更高'}
          </span>
          <Button variant="link" onclick={() => (picking = !picking)}>
            {picking ? '收起' : '改用其他版本'}
          </Button>
        </div>

        {#if picking}
          <div class="choices">
            <button class="pick" class:on={javaAuto} onclick={() => void setJava(null)}>
              <strong>自动</strong>
              <small class="t-mono">按版本要求选择，缺失时在启动前下载</small>
            </button>
            {#each fitting as item (item.path)}
              <button
                class="pick"
                class:on={settings.javaPath === item.path}
                onclick={() => void setJava(item.path)}
              >
                <strong>Java {item.major} · {item.version}</strong>
                <small class="t-mono">{javaLabel(item)}</small>
              </button>
            {/each}

            {#if fitting.length === 0}
              <div class="install">
                <Button variant="ghost" disabled={installing} onclick={() => void installJava()}>
                  {installing ? '正在下载…' : `下载 Java ${requirement.minimum}`}
                </Button>
              </div>
            {/if}

            <!--
              不兼容的收在后面，而且要说出为什么。摆在前面和好选项并排，等于
              请人踩一个我们已经知道的坑。
            -->
            {#if unfit.length > 0}
              <div class="fold">
                <Button variant="link" onclick={() => (showUnfit = !showUnfit)}>
                  {showUnfit ? '收起不兼容的版本' : `显示不兼容的版本（${unfit.length}）`}
                </Button>
              </div>
              {#if showUnfit}
                {#each unfit as item (item.path)}
                  <button
                    class="pick"
                    class:on={settings.javaPath === item.path}
                    onclick={() => void setJava(item.path)}
                  >
                    <strong>Java {item.major} · {item.version}</strong>
                    <small class="warn">{javaMismatch(item, requirement)}</small>
                  </button>
                {/each}
              {/if}
            {/if}
          </div>
        {/if}
      </section>

      <section>
        <div class="advanced">
          <Button variant="link" tone="quiet" onclick={() => (advanced = !advanced)}>
            <ChevronRight size={13} strokeWidth={2} class={advanced ? 'turned' : ''} />高级
          </Button>
        </div>
        {#if advanced}
          <!--
            三档而不是两档：「跟随全局」和「G1」不是一回事——前者是「我不管」，
            以后改了全局它跟着变；后者是「就要 G1」。把它们合成一个选项，等于
            让一次沉默变成一次表态。
          -->
          <div class="row-head adv">
            <span class="label">垃圾回收器</span>
            <span class="t-quiet">
              自动会按 Java 版本挑：21 以上给分代 ZGC，更老的给 G1。
            </span>
          </div>
          <Choice
            label="垃圾回收器"
            value={settings.garbageCollector ?? 'inherit'}
            onchange={(next) => {
              settings.garbageCollector = next === 'inherit' ? null : (next as GcChoice)
              void persist()
            }}
            options={[
              { value: 'inherit', label: `跟随全局（${GC_LABEL[globalGc]}）` },
              { value: 'auto', label: '自动' },
              { value: 'g1', label: 'G1' },
              { value: 'z', label: 'ZGC' },
            ]}
          />

          <div class="row-head adv">
            <span class="label">进程优先级</span>
            <span class="t-quiet">降低优先级可减少对其他程序的影响。</span>
          </div>
          <Choice
            label="进程优先级"
            value={settings.processPriority ?? 'normal'}
            onchange={(next) => {
              settings.processPriority = next === 'normal' ? null : next
              void persist()
            }}
            options={[
              { value: 'low', label: '低' },
              { value: 'normal', label: '正常' },
              { value: 'high', label: '高' },
            ]}
          />
        {/if}
      </section>

      <section>
        <Button variant="link" tone="quiet" onclick={() => (managing = !managing)}>
          <ChevronRight size={13} strokeWidth={2} class={managing ? 'turned' : ''} />管理
        </Button>
        {#if managing}
          <div class="row-head adv">
            <span class="label">名称</span>
          </div>
          <div class="rename">
            <input class="input" bind:value={renamed} maxlength="64" />
            <Button
              variant="ghost"
              disabled={!renamed.trim() || renamed.trim() === instanceName}
              onclick={() => void rename()}>
              重命名
            </Button>
          </div>

          <div class="row-foot manage">
            <span class="t-quiet">复制不含存档与日志</span>
            <Button variant="ghost" onclick={() => void duplicate()}>复制实例</Button>
          </div>

          <div class="row-foot manage">
            <span class="t-quiet">
              {confirmingDelete ? '存档、模组与配置将一并删除，不可撤销。' : '删除此实例'}
            </span>
            {#if confirmingDelete}
              <span class="confirm">
                <Button variant="ghost" onclick={() => (confirmingDelete = false)}>
                  取消
                </Button>
                <Button tone="danger" onclick={() => void remove()}>确认删除</Button>
              </span>
            {:else}
              <Button variant="ghost" onclick={() => (confirmingDelete = true)}>删除</Button>
            {/if}
          </div>
        {/if}
    </section>
  </div>
{/if}

{#if error}<div class="alert pad">{error}</div>{/if}

<style>
  .pad {
    margin: 0;
  }

  .body {
    max-width: 620px;
  }

  section {
    padding: var(--s4) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  section:last-child {
    box-shadow: none;
  }

  .row-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s3);
  }

  .label {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  /*
   * 结论就是那个数，所以它按数来排——文档三说的「数字直接当视觉元素用」。
   * 「自动」退成一枚跟在后面的小字：它是这个数的来历，不是和它并列的信息。
   */
  .amount {
    display: inline-flex;
    align-items: baseline;
    gap: var(--s2);
    color: var(--ink);
    font-size: var(--t-h2);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }

  .amount small {
    color: var(--ink-4);
    font-size: var(--t-micro);
    letter-spacing: 0;
  }

  .row-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin-top: var(--s2);
  }

  /* 左边那句话删掉之后，只剩一个动作，让它靠右。 */
  .row-foot.end {
    justify-content: flex-end;
  }

  /* 尺下面那几行理由。一类一行，不串成一长条。 */
  .reason {
    margin: var(--s2) 0 0;
    max-width: 64ch;
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .reason code {
    font-size: var(--t-micro);
  }

  .reason.warn,
  .pick small.warn {
    color: var(--danger);
  }

  .install,
  .fold {
    justify-self: start;
    margin-top: var(--s2);
  }

  /* 箭头转 90 度表示展开，和崩溃报告里那处是同一套。 */
  .advanced :global(svg) {
    transition: transform var(--t-base) var(--ease);
  }

  .advanced :global(svg.turned) {
    transform: rotate(90deg);
  }

  .row-head.adv {
    margin-top: var(--s4);
  }

  .row-head.adv + :global(.choice) {
    margin-top: var(--s2);
  }

  .rename {
    display: flex;
    gap: var(--s2);
    margin-top: var(--s2);
  }

  .rename .input {
    flex: 1;
    min-width: 0;
  }

  .row-foot.manage {
    margin-top: var(--s4);
  }

  .confirm {
    display: flex;
    gap: var(--s2);
  }

  /* 删除是唯一不可撤销的动作，给它唯一的红。 */

  .choices {
    display: grid;
    gap: 2px;
    margin-top: var(--s3);
  }

  /* 选项直接坐在面板上，靠底色区分选中——不套卡片，和实例列表一致。 */
  .pick {
    display: grid;
    gap: 2px;
    padding: var(--s2) var(--s3);
    border-radius: var(--r1);
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .pick:hover {
    background: var(--tint-1);
  }

  .pick.on {
    color: var(--ink);
    background: var(--tint-2);
  }

  .pick strong {
    font-size: var(--t-body);
    font-weight: 500;
  }

  .pick small {
    color: var(--ink-4);
    font-size: var(--t-micro);
    overflow-wrap: anywhere;
  }

  /* 带脸的那几行：脸在左，两行字在右，和别的选项共用同一个盒子。 */
  .pick.face-row {
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: var(--s3);
  }

  .pick.face-row span {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

</style>
