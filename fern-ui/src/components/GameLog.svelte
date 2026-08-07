<script lang="ts">
  /**
   * 游戏日志。
   *
   * 平时不该出现——这一屏存在的理由只有一个：出事了要能看见发生了什么，
   * 以及能整段复制给别人。所以它是浮层不是场景，从命令面板进。
   *
   * 等级只用颜色区分，不加图标也不加标签：一屏几百行，每行前面挂个图标是
   * 噪音；而警告和错误本来就该靠颜色一眼扫出来。
   */
  import { Copy, X } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import { launch, type LogLevel } from '../lib/launch.svelte'

  interface Props {
    onclose: () => void
  }

  let { onclose }: Props = $props()

  /** 只看有问题的。默认全看——过滤是找问题时才用的动作。 */
  let onlyProblems = $state(false)
  let copied = $state(false)

  const shown = $derived(
    onlyProblems
      ? launch.log.filter((line) => line.level === 'warn' || line.level === 'error')
      : launch.log,
  )

  const counts = $derived({
    warn: launch.log.filter((line) => line.level === 'warn').length,
    error: launch.log.filter((line) => line.level === 'error').length,
  })

  const levelClass = (level: LogLevel) =>
    level === 'error' ? 'error' : level === 'warn' ? 'warn' : level === 'info' ? '' : 'quiet'

  async function copyAll() {
    try {
      await navigator.clipboard.writeText(shown.map((line) => line.message).join('\n'))
      copied = true
      setTimeout(() => (copied = false), 1400)
    } catch {
      // 剪贴板被拒绝时内容本身可以选中，手动复制照样完成这件事。
    }
  }
</script>

<Overlay label="游戏日志" width="820px" {onclose}>
  <header>
    <div>
      <h2 class="t-h2">游戏日志</h2>
      <p class="t-quiet">
        {launch.log.length} 行{counts.error > 0 ? ` · ${counts.error} 条错误` : ''}{counts.warn > 0
          ? ` · ${counts.warn} 条警告`
          : ''}
      </p>
    </div>
    <button class="btn btn--icon" aria-label="关闭" onclick={onclose}><X size={16} /></button>
  </header>

  {#if launch.log.length === 0}
    <p class="empty t-quiet">这次还没有收到游戏的输出。游戏跑起来之后这里会有内容。</p>
  {:else}
    <div class="lines scroll">
      {#each shown as line, index (index)}
        <p class="line t-mono {levelClass(line.level)}">{line.message}</p>
      {/each}
      {#if shown.length === 0}
        <p class="empty t-quiet">没有警告或错误。</p>
      {/if}
    </div>
  {/if}

  <footer>
    <button class="btn btn--link" onclick={() => (onlyProblems = !onlyProblems)}>
      {onlyProblems ? '显示全部' : '只看警告和错误'}
    </button>
    <button class="btn btn--ghost" disabled={shown.length === 0} onclick={() => void copyAll()}>
      <Copy size={14} strokeWidth={1.9} />{copied ? '已复制' : '复制'}
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
  }

  header p {
    margin: var(--s1) 0 0;
  }

  .lines {
    min-height: 0;
    max-height: 62vh;
    padding: 0 var(--s5);
  }

  .line {
    margin: 0;
    padding: 1px 0;
    color: var(--ink-2);
    font-size: var(--t-micro);
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    /* 整段复制出去要能对得上原样，所以可以选中。 */
    user-select: text;
  }

  .line.quiet {
    color: var(--ink-4);
  }

  .line.warn {
    color: #e0b341;
  }

  .line.error {
    color: #e8705f;
  }

  .empty {
    margin: 0;
    padding: var(--s6) var(--s5);
    text-align: center;
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
