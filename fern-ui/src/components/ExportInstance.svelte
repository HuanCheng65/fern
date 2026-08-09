<script lang="ts">
  /**
   * 把这个实例带走。
   *
   * 两种格式，回答的是两个不同的问题，所以选择摆在最前面而不是藏在一个下拉里：
   *
   * - **整合包**（`.mrpack`）——给别人。别的启动器也认得，代价是模组以下载
   *   地址记录，对方下不到就是下不到。
   * - **搬迁包**（`.fernpack`）——给自己的另一台机器。模组文件本身在包里，
   *   所以它是那个「装得下就一定装得回去」的格式。
   *
   * 每一种下面都写清它**不**包含什么。导出最常见的失望是「拿到手才发现少了
   * 东西」，而那句话本该在按下去之前就说。
   */
  import { save } from '@tauri-apps/plugin-dialog'
  import Overlay from 'fern-kit/Overlay.svelte'
  import Choice from './Choice.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { notices } from '../lib/notices.svelte'
  import { exportFernpack, exportMrpack, fileStem, type Exported } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'

  interface Props {
    instanceId: string
    instanceName: string
    onclose: () => void
  }

  let { instanceId, instanceName, onclose }: Props = $props()

  type Format = 'mrpack' | 'fernpack'

  let format = $state<Format>('mrpack')
  let withSaves = $state(true)
  let withMods = $state(true)
  let busy = $state(false)
  let error = $state('')

  const extension = $derived(format === 'mrpack' ? 'mrpack' : 'fernpack')

  async function run() {
    const destination = await save({
      defaultPath: `${fileStem(instanceName)}.${extension}`,
      filters: [{ name: format === 'mrpack' ? 'Modrinth 整合包' : 'Fern 搬迁包', extensions: [extension] }],
    })
    if (!destination) return

    busy = true
    error = ''
    try {
      const result: Exported =
        format === 'mrpack'
          ? await exportMrpack(instanceId, destination)
          : await exportFernpack(instanceId, { saves: withSaves, mods: withMods }, destination)
      notices.say({
        title: '已导出',
        detail:
          `${result.files} 个文件 · ${formatBytes(result.bytes)}` +
          (result.linked === undefined ? '' : `，其中 ${result.linked} 个模组以下载地址记录`),
      })
      onclose()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }
</script>

<Overlay label="导出实例" width="500px" {onclose}>
  <header>
    <h2>导出「{instanceName}」</h2>
  </header>

  <div class="body">
    <Choice
      options={[
        { value: 'mrpack' as const, label: '整合包' },
        { value: 'fernpack' as const, label: '搬迁包' },
      ]}
      value={format}
      onchange={(value) => (format = value)}
      label="导出格式"
    />

    {#if format === 'mrpack'}
      <p class="about">
        <strong>Modrinth 整合包（.mrpack）</strong>
        <span>Prism、HMCL、PCL 等启动器都能导入，配置和资源包会一并打包。</span>
        <span class="t-quiet">
          模组只记下载地址，包内不含 jar 文件。地址要联网按文件哈希从 Modrinth 查得，查不到的模组会直接打进包里。整合包不含存档。
        </span>
      </p>
    {:else}
      <p class="about">
        <strong>Fern 搬迁包（.fernpack）</strong>
        <span>包含模组文件本身，换机器时用。只有 Fern 能打开。</span>
      </p>

      <div class="options">
        <label>
          <input type="checkbox" bind:checked={withSaves} />
          <span>包含存档</span>
        </label>
        <label>
          <input type="checkbox" bind:checked={withMods} />
          <span>
            包含模组文件
            {#if !withMods}
              <small class="t-quiet">不含 jar 的包在另一台机器上需要重新下载模组。</small>
            {/if}
          </span>
        </label>
      </div>
    {/if}

    {#if error}<div class="alert">{error}</div>{/if}
  </div>

  <footer>
    <Button variant="ghost" onclick={onclose}>取消</Button>
    <Button variant="primary" disabled={busy} onclick={() => void run()}>
      {busy ? '正在导出' : '选择位置并导出'}
    </Button>
  </footer>
</Overlay>

<style>
  header {
    padding: var(--s5) var(--s5) var(--s4);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  h2 {
    margin: 0;
    color: var(--ink);
    font-size: var(--t-h3);
    font-weight: 560;
    overflow-wrap: anywhere;
  }

  .body {
    display: grid;
    gap: var(--s4);
    padding: var(--s5);
    overflow-y: auto;
  }

  .about {
    display: grid;
    gap: var(--s1);
    margin: 0;
    font-size: var(--t-small);
    line-height: 1.7;
  }

  .about strong {
    color: var(--ink);
    font-weight: 500;
  }

  .about span {
    color: var(--ink-2);
  }

  .options {
    display: grid;
    gap: var(--s3);
  }

  .options label {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    font-size: var(--t-small);
    color: var(--ink-2);
    cursor: pointer;
  }

  .options small {
    display: block;
    margin-top: 2px;
    font-size: var(--t-micro);
  }

  .options input {
    margin-top: 2px;
    accent-color: var(--accent);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
    padding: var(--s4) var(--s5) var(--s5);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }
</style>
