<script lang="ts">
  /**
   * 实例设置。
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
  import Overlay from './Overlay.svelte'
  import { inTauri } from '../lib/instances.svelte'

  interface JavaRuntime {
    path: string
    major: number
    version: string
    vendor: string
    managed: boolean
  }

  interface InstanceRuntime {
    automaticMemoryMb: number
    physicalMemoryMb: number
    requirement: { minimum: number; maximum: number | null }
    java: JavaRuntime | null
    modsCount: number
  }

  interface InstanceSettings {
    javaPath: string | null
    maxMemoryMb: number | null
    resolution: { width: number; height: number } | null
  }

  interface Props {
    instanceId: string
    instanceName: string
    onclose: () => void
  }

  let { instanceId, instanceName, onclose }: Props = $props()

  let runtime = $state<InstanceRuntime | null>(null)
  let runtimes = $state<JavaRuntime[]>([])
  let settings = $state<InstanceSettings>({ javaPath: null, maxMemoryMb: null, resolution: null })
  let loading = $state(true)
  let error = $state('')

  /** 滑杆的上限：机器的一半，和后端的封顶是同一条线。 */
  const ceiling = $derived(Math.max(2048, Math.floor((runtime?.physicalMemoryMb ?? 8192) / 2)))
  const memoryAuto = $derived(settings.maxMemoryMb === null)
  const memoryValue = $derived(settings.maxMemoryMb ?? runtime?.automaticMemoryMb ?? 2048)

  const javaLabel = (item: JavaRuntime) =>
    `Java ${item.major}${item.vendor ? ` · ${item.vendor}` : ''}${item.managed ? ' · Fern 下载' : ''}`

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

  void load()
</script>

<Overlay label="{instanceName} 的设置" width="520px" {onclose}>
  <header>
    <h2 class="t-h2">{instanceName}</h2>
    <p class="t-quiet">只影响这一个实例</p>
  </header>

  {#if loading}
    <p class="t-quiet pad">读取中</p>
  {:else}
    <div class="body scroll">
      <section>
        <div class="row-head">
          <span class="label">内存</span>
          <span class="t-mono value">
            {#if memoryAuto}
              自动 · {runtime?.automaticMemoryMb ?? 2048} MB
            {:else}
              {memoryValue} MB
            {/if}
          </span>
        </div>
        <input
          class="slider"
          type="range"
          min="1024"
          max={ceiling}
          step="512"
          value={memoryValue}
          disabled={memoryAuto}
          oninput={(event) => setMemory(Number(event.currentTarget.value))}
        />
        <div class="row-foot">
          <span class="t-quiet">
            {#if runtime && runtime.modsCount > 0}
              {runtime.modsCount} 个模组 · 机器共 {Math.round(runtime.physicalMemoryMb / 1024)} GB
            {:else}
              机器共 {Math.round((runtime?.physicalMemoryMb ?? 0) / 1024)} GB，上限是它的一半
            {/if}
          </span>
          <button
            class="btn btn--link"
            onclick={() => setMemory(memoryAuto ? (runtime?.automaticMemoryMb ?? 2048) : null)}
          >
            {memoryAuto ? '手动指定' : '回到自动'}
          </button>
        </div>
      </section>

      <section>
        <div class="row-head">
          <span class="label">Java</span>
          <span class="t-mono value">
            需要 {runtime?.requirement.minimum ?? 8}{runtime?.requirement.maximum
              ? ` – ${runtime.requirement.maximum}`
              : ' 或更新'}
          </span>
        </div>
        <div class="choices">
          <button class="pick" class:on={settings.javaPath === null} onclick={() => setJava(null)}>
            <strong>自动</strong>
            <small class="t-mono">
              {runtime?.java ? `现在会用 Java ${runtime.java.major}` : '没有合适的，启动时会下载'}
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
    </div>
  {/if}

  {#if error}<div class="alert pad">{error}</div>{/if}

  <footer>
    <button class="btn btn--primary" onclick={onclose}>完成</button>
  </footer>
</Overlay>

<style>
  header {
    padding: var(--s5) var(--s5) var(--s4);
  }

  header h2 {
    margin: 0;
    overflow-wrap: anywhere;
  }

  header p {
    margin: var(--s1) 0 0;
  }

  .pad {
    margin: 0 var(--s5) var(--s4);
  }

  .body {
    min-height: 0;
    padding: 0 var(--s5);
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

  .slider:disabled {
    opacity: 0.4;
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

  footer {
    display: flex;
    justify-content: flex-end;
    padding: var(--s4) var(--s5) var(--s5);
  }
</style>
