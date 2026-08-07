<script lang="ts">
  /**
   * 游戏崩了之后说的那句话。
   *
   * 顺序是「人话 → 怎么办 → 原始报告」，而且只有前两样默认可见。用户在这个
   * 时刻要的是「我该做什么」，不是一屏栈——栈是给他愿意深究、或者要贴给别人
   * 看的时候准备的，所以折叠着，但一定要在。
   *
   * 认不出原因时照实说认不出，不编一个听起来很像那么回事的诊断。写错的诊断
   * 比没有诊断更浪费时间：用户会顺着错的方向排查很久。
   */
  import { ChevronRight, FolderOpen } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import type { CrashReport } from '../lib/launch.svelte'

  interface Props {
    report: CrashReport
    onclose: () => void
    onopenLogs: () => void
  }

  let { report, onclose, onopenLogs }: Props = $props()

  let showRaw = $state(false)

  const exit = $derived(
    report.exitCode === null ? '进程被系统终止' : `退出码 ${report.exitCode}`,
  )
</script>

<Overlay label="游戏异常退出" width="600px" {onclose}>
  <header>
    <h2 class="t-h2">{report.diagnosis?.title ?? '游戏异常退出'}</h2>
    <p class="t-quiet">{exit}</p>
  </header>

  <p class="detail">
    {report.diagnosis?.detail ??
      '未匹配到已知的崩溃原因。以下为日志末尾内容，可用于进一步排查。'}
  </p>

  {#if report.reportPath}
    <p class="t-mono path">{report.reportPath}</p>
  {/if}

  <button class="btn btn--link raw" onclick={() => (showRaw = !showRaw)}>
    <ChevronRight size={13} strokeWidth={2} class={showRaw ? 'turned' : ''} />
    {showRaw ? '收起原始日志' : '查看原始日志'}
  </button>

  {#if showRaw}
    <pre class="scroll excerpt t-mono">{report.excerpt || '未捕获到日志'}</pre>
  {/if}

  <footer>
    <button class="btn btn--ghost" onclick={onopenLogs}>
      <FolderOpen size={13} strokeWidth={1.9} />日志目录
    </button>
    <button class="btn btn--primary" onclick={onclose}>关闭</button>
  </footer>
</Overlay>

<style>
  header {
    padding: var(--s5) var(--s5) 0;
  }

  header h2 {
    margin: 0;
  }

  header p {
    margin: var(--s1) 0 0;
  }

  .detail {
    margin: var(--s3) 0 0;
    padding: 0 var(--s5);
    color: var(--ink-2);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .path {
    margin: var(--s3) 0 0;
    padding: 0 var(--s5);
    color: var(--ink-4);
    overflow-wrap: anywhere;
  }

  .raw {
    margin: var(--s4) var(--s5) 0;
    color: var(--ink-3);
  }

  .raw:hover {
    color: var(--ink);
  }

  /* 箭头转 90 度表示展开，比换一个图标更安静。 */
  .raw :global(svg) {
    transition: transform var(--t-base) var(--ease);
  }

  .raw :global(svg.turned) {
    transform: rotate(90deg);
  }

  .excerpt {
    max-height: 40vh;
    margin: var(--s3) var(--s5) 0;
    padding: var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
    padding: var(--s5);
  }
</style>
