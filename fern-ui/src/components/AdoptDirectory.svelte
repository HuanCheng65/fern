<script lang="ts">
  /**
   * 把一个已有的 `.minecraft` 接进来。
   *
   * 大多数人用启动器的方式是把它和 `.minecraft` 放在一起，那个目录里已经有
   * 版本、有存档、有几百个 Mod。上一版的 Fern 只认自己私有目录里的实例，
   * 这样的用户第一步就得放弃已有的一切。
   *
   * **不移动任何文件。** 添加只是写入一份指向那个目录的实例描述，所以这一页
   * 是可以随便试的：添加错了把实例删掉即可，那个目录不受影响。这句话要写在
   * 界面上——「导入」在别的启动器里通常意味着复制几十 GB。
   *
   * 目录用系统选择器选，不让用户手打路径。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { open } from '@tauri-apps/plugin-dialog'
  import { FolderOpen } from 'lucide-svelte'
  import { instances, inTauri } from '../lib/instances.svelte'
  import { nav } from '../lib/nav.svelte'
  import { notices } from '../lib/notices.svelte'

  interface ExternalVersion {
    id: string
    gameVersion: string
    loader: string
    loaderVersion?: string
    isolation: 'shared' | 'perVersion'
    attached: boolean
    saves: number
    mods: number
  }

  /** 一个没能成为版本的目录，以及原因。 */
  interface SkippedVersion {
    name: string
    reason: string
  }

  interface ExternalScan {
    /** 真正读的那个目录。选中的是它的上一层时，两者不同。 */
    root: string
    versions: ExternalVersion[]
    skipped: SkippedVersion[]
  }

  interface Props {
    /** 一开始就指向某个目录（首次启动时发现的那一个）。 */
    initial?: string
  }

  let { initial = '' }: Props = $props()

  let directory = $state('')
  let versions = $state<ExternalVersion[] | null>(null)
  let skipped = $state<SkippedVersion[]>([])
  let busy = $state('')
  let error = $state('')
  /**
   * 使用 Fern 的共享资源与依赖库。
   *
   * 默认使用：多个实例共享一份 assets 能省下数 GB。关闭后使用该目录自带的
   * 那一份，占用更多磁盘空间，但该目录仍可被原启动器单独使用。
   */
  let shared = $state(true)

  const ISOLATION_LABEL = {
    shared: '存档与其他版本共用',
    perVersion: '独立存档与模组',
  }

  const LOADER_LABEL: Record<string, string> = {
    vanilla: '原版',
    fabric: 'Fabric',
    quilt: 'Quilt',
    neo_forge: 'NeoForge',
    forge: 'Forge',
  }

  async function choose() {
    if (!inTauri()) return
    const picked = await open({ directory: true, multiple: false, title: '选择游戏目录' })
    if (typeof picked !== 'string') return
    await scan(picked)
  }

  async function scan(path: string) {
    busy = 'scan'
    error = ''
    versions = null
    skipped = []
    try {
      const result = await invoke<ExternalScan>('scan_game_directory', { path })
      versions = result.versions
      skipped = result.skipped
      // 选中的目录里正好有一个 `.minecraft` 时读的是它，后续的添加也用它。
      directory = result.root
    } catch (cause) {
      error = String(cause)
      directory = path
    } finally {
      busy = ''
    }
  }

  async function attach(version: ExternalVersion) {
    busy = version.id
    error = ''
    try {
      const created = await invoke<{ id: string; name: string }>('attach_game_version', {
        path: directory,
        versionId: version.id,
        sharedLibraries: shared,
      })
      await instances.load()
      notices.say({
        title: `已添加 ${created.name}`,
        detail: '游戏文件保留在原位置。',
        action: {
          label: '打开',
          run: () => {
            instances.select(created.id)
            nav.enter('instances', created.id)
          },
        },
      })
      // 重新扫描：刚添加的那一个现在应显示为已添加。
      await scan(directory)
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = ''
    }
  }

  // 首次启动时已经发现了一个目录，直接扫给用户看，不必再让他选一遍。
  $effect(() => {
    if (initial && !directory && inTauri()) void scan(initial)
  })
</script>

<div class="adopt">
  <p class="lead">
    选择一个 <code class="t-mono">.minecraft</code> 目录，Fern 会列出其中的版本。添加后可以照常补全文件、安装模组与启动；游戏文件保留在原位置，不会移动或复制，该目录仍可由原启动器使用。
  </p>

  <div class="picker">
    <button class="btn btn--ghost" disabled={busy === 'scan'} onclick={() => void choose()}>
      <FolderOpen size={14} strokeWidth={1.8} />{directory ? '更换目录' : '选择目录'}
    </button>
    {#if directory}
      <span class="chosen t-mono selectable">{directory}</span>
    {/if}
  </div>

  {#if busy === 'scan'}
    <p class="t-quiet">正在读取目录…</p>
  {:else if versions}
    {#if versions.length === 0}
      <p class="t-quiet">该目录中没有可用的版本。</p>
    {:else}
      <label class="shared">
        <input type="checkbox" bind:checked={shared} />
        <span>
          使用 Fern 的共享资源与依赖库
          <small>
            关闭后使用该目录自带的 assets 与 libraries，占用更多磁盘空间，该目录仍可由原启动器单独使用。
          </small>
        </span>
      </label>

      <ul class="versions">
        {#each versions as version (version.id)}
          <li class="row">
            <span class="text">
              <strong>{version.id}</strong>
              <small class="t-mono">
                {version.gameVersion}
                {#if version.loader !== 'vanilla'}
                  · {LOADER_LABEL[version.loader] ?? version.loader}{version.loaderVersion
                    ? ` ${version.loaderVersion}`
                    : ''}
                {/if}
                · {ISOLATION_LABEL[version.isolation]}
                {#if version.saves > 0}· {version.saves} 个存档{/if}
                {#if version.mods > 0}· {version.mods} 个模组{/if}
              </small>
            </span>
            {#if version.attached}
              <span class="t-quiet done">已添加</span>
            {:else}
              <button
                class="btn btn--ghost"
                disabled={busy !== ''}
                onclick={() => void attach(version)}
              >
                {busy === version.id ? '添加中' : '添加'}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <!--
      跳过的目录要说出来，而不是从列表里悄悄消失。用户是对着一个自己装了
      十几个版本的目录看这一屏的，少了哪个他一眼就看得出来，缺的是原因；
      一个都没扫出来时，这里就是唯一能解释发生了什么的地方。
    -->
    {#if skipped.length > 0}
      <details class="skipped" open={versions.length === 0}>
        <summary>{skipped.length} 个目录未被识别为版本</summary>
        <ul>
          {#each skipped as item (item.name)}
            <li>
              <span class="t-mono">{item.name}</span>
              <small>{item.reason}</small>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  {/if}

  {#if error}<div class="alert">{error}</div>{/if}
</div>

<style>
  .adopt {
    display: grid;
    gap: var(--s5);
  }

  .lead {
    margin: 0;
    max-width: 62ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.65;
  }

  .picker {
    display: flex;
    align-items: center;
    gap: var(--s3);
    min-width: 0;
  }

  .chosen {
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--t-micro);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .shared {
    display: flex;
    align-items: flex-start;
    gap: var(--s2);
    font-size: var(--t-small);
  }

  .shared span {
    display: grid;
    gap: 2px;
    color: var(--ink-2);
  }

  .shared small {
    max-width: 52ch;
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.55;
  }

  .versions {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row:last-child {
    box-shadow: none;
  }

  .text {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .done {
    flex: none;
    font-size: var(--t-small);
  }

  /* 收起来的次要信息：多数时候不看，扫不出东西时是唯一的线索。 */
  .skipped {
    font-size: var(--t-small);
  }

  .skipped summary {
    color: var(--ink-3);
    cursor: pointer;
  }

  .skipped ul {
    display: grid;
    gap: var(--s1);
    margin: var(--s2) 0 0;
    padding: 0;
    list-style: none;
  }

  .skipped li {
    display: flex;
    gap: var(--s2);
    min-width: 0;
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .skipped li span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .skipped li small {
    flex: none;
    color: var(--ink-4);
  }
</style>
