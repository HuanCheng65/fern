<script lang="ts">
  /**
   * 要手动下载的那几个文件。
   *
   * CurseForge 上有些作者关掉了第三方下载。我们不绕过它：把人送到那个文件的
   * 网页上，下完回来，后端在下载文件夹里按文件名找、按 sha1 认；浏览器问过
   * 存哪的话，就让用户自己指那个文件。
   *
   * 装模组的计划和导入整合包都用这一块——同一件事，说法得一样。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { open } from '@tauri-apps/plugin-dialog'
  import { ArrowUpRight } from 'lucide-svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  export interface ManualFile {
    title: string
    fileName: string
    page?: string
    found: boolean
  }

  interface Props {
    files: ManualFile[]
    /** 正在重新查找。 */
    checking?: boolean
    onrecheck: () => void
    /** 用户在选择器里指了文件。 */
    onpick: (paths: string[]) => void
  }

  let { files, checking = false, onrecheck, onpick }: Props = $props()

  const missing = $derived(files.filter((file) => !file.found).length)

  async function pick() {
    const chosen = await open({
      multiple: true,
      title: '选择已下载的文件',
      filters: [{ name: '模组、资源包或光影', extensions: ['jar', 'zip'] }],
    })
    if (!chosen) return
    onpick(Array.isArray(chosen) ? chosen : [chosen])
  }
</script>

<div class="manual">
  <p class="t-quiet">
    以下 {files.length} 个文件的作者仅允许从 CurseForge 网站下载。下载完成后选择「重新检查」，将在下载文件夹中查找；文件保存在其他位置时，可直接选择该文件。
  </p>
  <ul>
    {#each files as file (`${file.title}/${file.fileName}`)}
      <li class:found={file.found}>
        <span class="dot"></span>
        <span class="who">
          {file.title}
          <small class="t-mono">{file.fileName}</small>
        </span>
        <span class="state">{file.found ? '已找到' : '未找到'}</span>
        {#if !file.found && file.page}
          <Button variant="link" onclick={() => void invoke('open_external', { url: file.page })}>
            打开下载页<ArrowUpRight size={13} strokeWidth={1.9} />
          </Button>
        {/if}
      </li>
    {/each}
  </ul>
  {#if missing > 0}
    <div class="actions">
      <Button loading={checking} onclick={onrecheck}>重新检查</Button>
      <Button variant="ghost" onclick={() => void pick()}>选择文件</Button>
    </div>
  {/if}
</div>

<style>
  .manual {
    display: grid;
    gap: var(--s2);
  }
  p {
    margin: 0;
    font-size: var(--t-small);
  }
  ul {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    font-size: var(--t-small);
  }
  .dot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--danger);
  }
  .found .dot {
    background: var(--accent);
  }
  .who {
    min-width: 0;
    color: var(--ink-2);
  }
  .who small {
    margin-left: var(--s1);
    color: var(--ink-3);
  }
  .state {
    margin-left: auto;
    color: var(--ink-3);
    font-size: var(--t-micro);
  }
  li:not(.found) .state {
    color: var(--danger);
  }
  .actions {
    display: flex;
    gap: var(--s2);
  }
</style>
