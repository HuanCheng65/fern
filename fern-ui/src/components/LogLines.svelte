<script lang="ts">
  /**
   * 一段游戏日志。
   *
   * 浮层（命令面板进来的那个）和实例详情页的日志 tab 用的是同一段渲染——
   * 等级配色、可选中、过滤和复制的行为在两个地方必须一模一样，否则用户会
   * 以为看到的是两份不同的东西。
   *
   * 等级只用颜色区分，不加图标也不加标签：一屏几百行，每行前面挂个图标是
   * 噪音；而警告和错误本来就该靠颜色一眼扫出来。
   */
  import { Copy } from 'lucide-svelte'
  import type { GameLogLine, LogLevel } from '../lib/launch.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface Props {
    lines: GameLogLine[]
    /** 空的时候说什么。两个调用方的语境不同。 */
    emptyNote?: string
  }

  let { lines, emptyNote = '本次运行尚无输出。' }: Props = $props()

  /** 只看有问题的。默认全看——过滤是找问题时才用的动作。 */
  let onlyProblems = $state(false)
  let copied = $state(false)

  const shown = $derived(
    onlyProblems
      ? lines.filter((line) => line.level === 'warn' || line.level === 'error')
      : lines,
  )

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

{#if lines.length === 0}
  <p class="empty t-quiet">{emptyNote}</p>
{:else}
  <div class="lines scroll">
    {#each shown as line, index (index)}
      <p class="line t-mono {levelClass(line.level)}">{line.message}</p>
    {/each}
    {#if shown.length === 0}
      <p class="empty t-quiet">无警告或错误。</p>
    {/if}
  </div>

  <div class="bar">
    <Button variant="link" onclick={() => (onlyProblems = !onlyProblems)}>
      {onlyProblems ? '显示全部' : '仅显示警告与错误'}
    </Button>
    <Button variant="ghost" disabled={shown.length === 0} onclick={() => void copyAll()}>
      <Copy size={14} strokeWidth={1.9} />{copied ? '已复制' : '复制'}
    </Button>
  </div>
{/if}

<style>
  .lines {
    min-height: 0;
    flex: 1;
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
    padding: var(--s6) 0;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding-top: var(--s3);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }
</style>
