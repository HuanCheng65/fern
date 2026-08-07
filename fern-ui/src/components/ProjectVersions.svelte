<script lang="ts">
  /**
   * 选一个版本装进去。
   *
   * 默认只列正式版。beta 和 alpha 是作者明确标出来的「还不稳」，摆在同一列里
   * 会让人在不知情的情况下装上——想尝鲜的人自己展开。
   *
   * 必需依赖由后端一并解析和下载，这里不重复列一遍：用户要的是「装上」，
   * 而不是先读一遍依赖图。装完会说清一共装了几个文件。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { Check, X } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import { inTauri } from '../lib/instances.svelte'

  interface Version {
    id: string
    name: string
    versionNumber: string
    versionType: string
    gameVersions: string[]
    loaders: string[]
    downloads: number
    datePublished: string
    fileName?: string
  }

  interface Props {
    project: string
    title: string
    instanceId: string
    onclose: () => void
  }

  let { project, title, instanceId, onclose }: Props = $props()

  let versions = $state<Version[]>([])
  let loading = $state(true)
  let error = $state('')
  let installing = $state('')
  let installed = $state<string[]>([])
  let showPrerelease = $state(false)

  const releases = $derived(versions.filter((item) => item.versionType === 'release'))
  const shown = $derived(showPrerelease ? versions : releases)
  const prereleaseCount = $derived(versions.length - releases.length)

  const day = (iso: string) => iso.slice(0, 10)

  async function load() {
    if (!inTauri()) {
      loading = false
      return
    }
    try {
      versions = await invoke<Version[]>('project_versions', { project, instanceId })
    } catch (cause) {
      error = String(cause)
    } finally {
      loading = false
    }
  }

  async function install(version: Version) {
    installing = version.id
    error = ''
    try {
      installed = await invoke<string[]>('install_from_modrinth', {
        instanceId,
        versionId: version.id,
      })
    } catch (cause) {
      error = String(cause)
    } finally {
      installing = ''
    }
  }

  void load()
</script>

<Overlay label="{title} 的版本" width="560px" {onclose}>
  <header>
    <div>
      <h2 class="t-h2">{title}</h2>
      <p class="t-quiet">选择要安装的版本</p>
    </div>
    <button class="btn btn--icon" aria-label="关闭" onclick={onclose}><X size={16} /></button>
  </header>

  {#if installed.length > 0}
    <div class="done">
      <p class="ok"><Check size={15} strokeWidth={2.4} />已安装 {installed.length} 个文件</p>
      <ul class="files t-mono">
        {#each installed as file (file)}<li>{file}</li>{/each}
      </ul>
      {#if installed.length > 1}
        <p class="t-quiet">其中包含自动解析的必需依赖。</p>
      {/if}
    </div>
  {:else if loading}
    <p class="t-quiet pad">读取中</p>
  {:else if shown.length === 0}
    <p class="t-quiet pad">
      {versions.length === 0 ? '没有适用于这个实例的版本。' : '没有正式版，展开预览版查看。'}
    </p>
  {:else}
    <div class="list scroll">
      {#each shown as version (version.id)}
        <div class="row">
          <span class="text">
            <strong>{version.versionNumber}</strong>
            <small class="t-mono">
              {day(version.datePublished)}
              {#if version.versionType !== 'release'}
                · <em class="pre">{version.versionType}</em>
              {/if}
            </small>
          </span>
          <button
            class="btn btn--ghost"
            disabled={installing !== ''}
            onclick={() => void install(version)}
          >
            {installing === version.id ? '安装中' : '安装'}
          </button>
        </div>
      {/each}
    </div>
  {/if}

  {#if error}<div class="alert pad">{error}</div>{/if}

  <footer>
    {#if prereleaseCount > 0 && installed.length === 0}
      <button class="btn btn--link" onclick={() => (showPrerelease = !showPrerelease)}>
        {showPrerelease ? '只看正式版' : `显示 ${prereleaseCount} 个预览版`}
      </button>
    {:else}
      <span></span>
    {/if}
    <button class="btn btn--primary" onclick={onclose}>
      {installed.length > 0 ? '完成' : '取消'}
    </button>
  </footer>
</Overlay>

<style>
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s5) var(--s4) var(--s3) var(--s5);
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

  .list {
    min-height: 0;
    max-height: 46vh;
    padding: 0 var(--s5);
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

  /* 预览版标一下就够，不用警告色——作者标了它就是给人试的。 */
  .pre {
    font-style: normal;
    color: var(--ink-3);
  }

  .done {
    padding: 0 var(--s5) var(--s3);
  }

  .ok {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin: 0;
    color: var(--ink);
    font-size: var(--t-body);
  }

  .ok :global(svg) {
    color: var(--accent);
  }

  .files {
    margin: var(--s3) 0 var(--s3);
    padding: 0;
    list-style: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    line-height: 1.7;
    overflow-wrap: anywhere;
  }

  .done p:last-child {
    margin: 0;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s3) var(--s5) var(--s4);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }
</style>
