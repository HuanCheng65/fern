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
   * 两项都默认「自动」，而且自动这一档要把算出来的结果写出来——「自动」两个
   * 字本身不解释任何事情，用户看到「自动 · 4096 MB」才知道要不要动它。
   * 这是文档 §4.3 和 §6.1 的同一条要求：这一层平时该是隐形的，只有想接管的
   * 人才需要看见它。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ChevronRight } from 'lucide-svelte'
  import Choice from './Choice.svelte'
  import Loading from './Loading.svelte'
  import { inTauri, instances } from '../lib/instances.svelte'

  interface JavaRuntime {
    path: string
    major: number
    version: string
    vendor: string
    arch: string
    managed: boolean
    image: 'jdk' | 'jre'
    native: boolean
  }

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

  interface InstanceRuntime {
    automaticMemoryMb: number
    allocation: AllocationDecision
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
   * 分配结论摊开成一句话。
   *
   * 依据、实测、约束三类各自成句，中间用句号断开——「基于 186 个 Mod、光影。
   * 上次运行峰值 6.3 GB。」比把它们全用顿号串成一长条读得快。
   */
  const reasons = $derived.by(() => {
    const items = runtime?.allocation.explanation ?? []
    return (['basis', 'history', 'limit'] as const)
      .map((topic) =>
        items
          .filter((item) => item.topic === topic)
          .map((item) => item.text)
          .join('、'),
      )
      .filter(Boolean)
      .join('。')
  })

  /** MB 变成一句话。整数不带小数点——`8 GB` 比 `8.0 GB` 更像一个决定。 */
  const gigabytes = (mb: number) => {
    const value = mb / 1024
    return Math.abs(value - Math.round(value)) < 0.05
      ? `${Math.round(value)} GB`
      : `${value.toFixed(1)} GB`
  }

  /**
   * 同一个大版本可能同时装着 JDK 和 JRE，两行不能长得一模一样。
   *
   * 跑游戏两者没有区别，所以它只出现在标签里，不参与选择。
   */
  const javaLabel = (item: JavaRuntime) =>
    [
      `Java ${item.major}`,
      item.vendor,
      item.image === 'jdk' ? 'JDK' : 'JRE',
      item.managed ? '由 Fern 下载' : '',
      item.native ? '' : `${item.arch}，非原生架构`,
    ]
      .filter(Boolean)
      .join(' · ')

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

  function setMemory(value: number | null) {
    settings.maxMemoryMb = value
    void persist()
  }

  function setJava(path: string | null) {
    settings.javaPath = path
    void persist()
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
        内存这一节不以滑杆开场（设计文档 §8）。默认只有一行结论加它的理由：
        判断依据摊开、控件退后。滑杆是想接管的人才需要的东西，让它一直摆在
        那里，等于每次打开这一屏都问一遍「你要不要动内存」。
      -->
      <section>
        <div class="row-head">
          <span class="label">内存</span>
          <span class="t-mono value">
            {#if memoryAuto}
              自动 · {gigabytes(runtime?.allocation.xmxMb ?? runtime?.automaticMemoryMb ?? 2048)}
            {:else}
              {gigabytes(memoryValue)}
            {/if}
          </span>
        </div>
        {#if memoryAuto}
          <p class="reason t-quiet">{reasons}</p>
        {:else}
          <input
            class="slider"
            type="range"
            min="1024"
            max={ceiling}
            step="512"
            value={memoryValue}
            oninput={(event) => setMemory(Number(event.currentTarget.value))}
          />
        {/if}
        <div class="row-foot">
          <span class="t-quiet">
            物理内存 {gigabytes(runtime?.physicalMemoryMb ?? 0)}，游戏上限 {gigabytes(ceiling)}
          </span>
          <button
            class="btn btn--link"
            onclick={() =>
              setMemory(
                memoryAuto ? (runtime?.allocation.xmxMb ?? runtime?.automaticMemoryMb ?? 2048) : null,
              )}
          >
            {memoryAuto ? '手动指定' : `改回自动（${gigabytes(runtime?.automaticMemoryMb ?? 2048)}）`}
          </button>
        </div>
      </section>

      <section>
        <div class="row-head">
          <span class="label">Java</span>
          <span class="t-mono value">
            需要 Java {runtime?.requirement.minimum ?? 8}{runtime?.requirement.maximum
              ? ` – ${runtime.requirement.maximum}`
              : ' 或更高'}
          </span>
        </div>
        <div class="choices">
          <button class="pick" class:on={settings.javaPath === null} onclick={() => setJava(null)}>
            <strong>自动</strong>
            <small class="t-mono">
              {runtime?.java ? `当前将使用 Java ${runtime.java.major}` : '无匹配版本，启动时自动下载'}
            </small>
          </button>
          {#each runtimes as item (item.path)}
            <button
              class="pick"
              class:on={settings.javaPath === item.path}
              onclick={() => setJava(item.path)}
            >
              <strong>{javaLabel(item)}</strong>
              <small class="t-mono">{item.path}</small>
            </button>
          {/each}
        </div>
      </section>

      <section>
        <button class="btn btn--link advanced" onclick={() => (advanced = !advanced)}>
          <ChevronRight size={13} strokeWidth={2} class={advanced ? 'turned' : ''} />高级
        </button>
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
        <button class="btn btn--link advanced" onclick={() => (managing = !managing)}>
          <ChevronRight size={13} strokeWidth={2} class={managing ? 'turned' : ''} />管理
        </button>
        {#if managing}
          <div class="row-head adv">
            <span class="label">名称</span>
          </div>
          <div class="rename">
            <input class="input" bind:value={renamed} maxlength="64" />
            <button
              class="btn btn--ghost"
              disabled={!renamed.trim() || renamed.trim() === instanceName}
              onclick={() => void rename()}
            >
              重命名
            </button>
          </div>

          <div class="row-foot manage">
            <span class="t-quiet">复制不含存档与日志</span>
            <button class="btn btn--ghost" onclick={() => void duplicate()}>复制实例</button>
          </div>

          <div class="row-foot manage">
            <span class="t-quiet">
              {confirmingDelete ? '存档、模组与配置将一并删除，不可撤销。' : '删除此实例'}
            </span>
            {#if confirmingDelete}
              <span class="confirm">
                <button class="btn btn--ghost" onclick={() => (confirmingDelete = false)}>
                  取消
                </button>
                <button class="btn danger" onclick={() => void remove()}>确认删除</button>
              </span>
            {:else}
              <button class="btn btn--ghost" onclick={() => (confirmingDelete = true)}>删除</button>
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

  .value {
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }

  .row-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin-top: var(--s2);
  }

  .slider {
    display: block;
    width: 100%;
    margin-top: var(--s3);
    accent-color: var(--accent);
  }

  /* 结论下面那行理由。占滑杆原来的位置，所以两种状态的高度不会跳。 */
  .reason {
    margin: var(--s2) 0 0;
    max-width: 64ch;
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .slider:disabled {
    opacity: 0.4;
  }

  .advanced {
    color: var(--ink-3);
  }

  .advanced:hover {
    color: var(--ink);
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
  .btn.danger {
    color: #fff;
    background: #c42b1c;
  }

  .btn.danger:hover {
    background: #d8402f;
  }

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

</style>
