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
   * 带什么由内容清单说了算：世界逐个勾，其余按分区勾。上一版一股脑全带，
   * 既选不了「五个世界只带一个」，也没法在分享整合包时把写着服务器地址的
   * config 摘出去。空分区不出现——一个永远勾不出东西的选项只会让人怀疑
   * 导出坏了。
   */
  import { save } from '@tauri-apps/plugin-dialog'
  import Dialog from 'fern-kit/ui/Dialog.svelte'
  import SegmentedControl from 'fern-kit/ui/SegmentedControl.svelte'
  import { format as fill, ui } from '../lib/i18n'
  import { inTauri } from '../lib/instances.svelte'
  import { formatBytes } from '../lib/jobs.svelte'
  import { notices } from '../lib/notices.svelte'
  import {
    exportFernpack,
    exportInventory,
    exportMrpack,
    fileStem,
    type ExportContents,
    type ExportInventory,
    type Exported,
  } from '../lib/backup'
  import Button from 'fern-kit/ui/Button.svelte'

  interface Props {
    instanceId: string
    instanceName: string
    onclose: () => void
  }

  let { instanceId, instanceName, onclose }: Props = $props()

  type Format = 'mrpack' | 'fernpack'

  let format = $state<Format>('mrpack')
  let inventory = $state<ExportInventory | undefined>(undefined)
  /** 勾选状态。世界是名字的集合，其余是分区开关。 */
  let saves = $state<string[]>([])
  let mods = $state(true)
  let config = $state(true)
  let resourcepacks = $state(true)
  let shaderpacks = $state(true)
  let schematics = $state(true)
  let screenshots = $state(true)
  let busy = $state(false)
  let error = $state('')

  const extension = $derived(format === 'mrpack' ? 'mrpack' : 'fernpack')

  /**
   * 默认值按格式的用途给：搬迁是「全带走」，整合包是「一套玩法」——
   * 原理图和截图是这个人自己的东西，默认不给别人。
   */
  function applyDefaults(inv: ExportInventory, next: Format) {
    saves = next === 'fernpack' ? [...inv.saves] : []
    mods = true
    config = true
    resourcepacks = true
    shaderpacks = true
    schematics = next === 'fernpack'
    screenshots = next === 'fernpack'
  }

  $effect(() => {
    if (!inTauri()) return
    void exportInventory(instanceId).then((inv) => {
      inventory = inv
      applyDefaults(inv, format)
    })
  })

  function switchFormat(next: Format) {
    format = next
    if (inventory) applyDefaults(inventory, next)
  }

  const toggleSave = (name: string) =>
    (saves = saves.includes(name) ? saves.filter((it) => it !== name) : [...saves, name])

  const contents = (): ExportContents => ({
    saves,
    mods,
    config,
    resourcepacks,
    shaderpacks,
    schematics,
    screenshots,
  })

  async function run() {
    const destination = await save({
      defaultPath: `${fileStem(instanceName)}.${extension}`,
      filters: [
        {
          name: format === 'mrpack' ? ui.export.mrpackTitle : ui.export.fernpackTitle,
          extensions: [extension],
        },
      ],
    })
    if (!destination) return

    busy = true
    error = ''
    try {
      const result: Exported =
        format === 'mrpack'
          ? await exportMrpack(instanceId, contents(), destination)
          : await exportFernpack(instanceId, contents(), destination)
      notices.say({
        title: ui.export.done,
        detail:
          fill(ui.export.doneDetail, {
            count: String(result.files),
            size: formatBytes(result.bytes),
          }) +
          (result.linked === undefined
            ? ''
            : fill(ui.export.doneLinked, { count: String(result.linked) })),
      })
      onclose()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }
</script>

<Dialog label={ui.export.dialog} width="500px" {onclose}>
  <header>
    <h2>{fill(ui.export.title, { name: instanceName })}</h2>
  </header>

  <div class="body">
    <SegmentedControl
      options={[
        { value: 'mrpack' as const, label: ui.export.mrpack },
        { value: 'fernpack' as const, label: ui.export.fernpack },
      ]}
      value={format}
      onchange={switchFormat}
      aria-label={ui.export.formatAria}
    />

    {#if format === 'mrpack'}
      <p class="about">
        <strong>{ui.export.mrpackTitle}</strong>
        <span>{ui.export.mrpackAbout}</span>
        <span class="t-quiet">{ui.export.mrpackMods}</span>
      </p>
    {:else}
      <p class="about">
        <strong>{ui.export.fernpackTitle}</strong>
        <span>{ui.export.fernpackAbout}</span>
      </p>
    {/if}

    {#if inventory}
      <div class="carry">
        <span class="t-quiet group">{ui.export.carry}</span>

        {#if format === 'fernpack'}
          {#each inventory.saves as world (world)}
            <label>
              <input
                type="checkbox"
                checked={saves.includes(world)}
                onchange={() => toggleSave(world)}
              />
              <span>{fill(ui.export.world, { name: world })}</span>
            </label>
          {/each}
          {#if inventory.mods > 0}
            <label>
              <input type="checkbox" bind:checked={mods} />
              <span>
                {fill(ui.export.mods, { count: String(inventory.mods) })}
                {#if !mods}
                  <small class="t-quiet">{ui.export.modsOff}</small>
                {/if}
              </span>
            </label>
          {/if}
        {/if}

        {#if inventory.config > 0}
          <label>
            <input type="checkbox" bind:checked={config} />
            <span>
              {fill(ui.export.config, { count: String(inventory.config) })}
              {#if format === 'mrpack'}
                <small class="t-quiet">{ui.export.configHint}</small>
              {/if}
            </span>
          </label>
        {/if}
        {#if inventory.resourcepacks > 0}
          <label>
            <input type="checkbox" bind:checked={resourcepacks} />
            <span>{fill(ui.export.resourcepacks, { count: String(inventory.resourcepacks) })}</span>
          </label>
        {/if}
        {#if inventory.shaderpacks > 0}
          <label>
            <input type="checkbox" bind:checked={shaderpacks} />
            <span>{fill(ui.export.shaderpacks, { count: String(inventory.shaderpacks) })}</span>
          </label>
        {/if}
        {#if inventory.schematics > 0}
          <label>
            <input type="checkbox" bind:checked={schematics} />
            <span>{fill(ui.export.schematics, { count: String(inventory.schematics) })}</span>
          </label>
        {/if}
        {#if inventory.screenshots > 0}
          <label>
            <input type="checkbox" bind:checked={screenshots} />
            <span>{fill(ui.export.screenshots, { count: String(inventory.screenshots) })}</span>
          </label>
        {/if}
      </div>
    {/if}

    {#if error}<div class="alert">{error}</div>{/if}
  </div>

  <footer>
    <Button variant="ghost" onclick={onclose}>{ui.export.cancel}</Button>
    <Button variant="primary" loading={busy} onclick={() => void run()}>
      {ui.export.run}
    </Button>
  </footer>
</Dialog>

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

  .carry {
    display: grid;
    gap: var(--s3);
  }

  .carry .group {
    font-size: var(--t-micro);
    letter-spacing: 0.02em;
  }

  .carry label {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    font-size: var(--t-small);
    color: var(--ink-2);
    cursor: pointer;
  }

  .carry small {
    display: block;
    margin-top: 2px;
    font-size: var(--t-micro);
  }

  .carry input {
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
