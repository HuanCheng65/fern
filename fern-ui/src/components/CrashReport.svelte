<script lang="ts">
  /**
   * 游戏崩了之后说的那句话。
   *
   * 顺序是「发生了什么 → 能做什么 → 原始报告」，而且只有前两样默认可见。用户
   * 在这个时刻要的是下一步，不是一屏栈——栈是给他愿意深究、或者要贴给别人看的
   * 时候准备的，所以折叠着，但一定要在。
   *
   * 认不出原因时照实说认不出，不编一个听起来很像那么回事的诊断——写错的诊断
   * 比没有诊断更浪费时间，用户会顺着错的方向排查很久。这种时候「崩在哪个模组
   * 的代码里」往往是唯一的线索，所以嫌疑模组独立于诊断显示。
   */
  import { ChevronRight, FolderOpen } from 'lucide-svelte'
  import Overlay from 'fern-kit/Overlay.svelte'
  import Advice from './Advice.svelte'
  import { describe } from '../lib/i18n'
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
  const found = $derived(
    report.diagnoses.map((diagnosis) => ({
      ...diagnosis,
      ...describe(`crash.${diagnosis.id}`, diagnosis.args),
    })),
  )
  const suspects = $derived(report.suspects.slice(0, 3))
</script>

<Overlay label="游戏异常退出" width="600px" {onclose}>
  <header>
    <h2 class="t-h2">{found[0]?.title ?? '游戏异常退出'}</h2>
    <p class="t-quiet">{exit}</p>
  </header>

  <div class="body">
    {#if found.length > 0}
      <p class="detail">{found[0].detail}</p>
      {#if found[0].action}
        <div class="fix">
          <Advice
            title={found[0].title}
            detail=""
            action={found[0].action}
            instanceId={report.instanceId}
          />
        </div>
      {/if}
    {:else}
      <p class="detail">
        没有匹配到已知的崩溃原因。下面是日志末尾，可用于进一步排查或反馈。
      </p>
    {/if}

    {#if suspects.length > 0}
      <p class="suspects">
        崩溃发生在{#each suspects as suspect, index (suspect.modId)}{index > 0
            ? '、'
            : ' '}<strong>{suspect.name}{suspect.version ? ` ${suspect.version}` : ''}</strong
          >{/each} 的代码中。
      </p>
    {/if}

    <!-- 次要的那几条：可能同时成立，但不该抢第一条的位置。 -->
    {#if found.length > 1}
      <ul class="others">
        {#each found.slice(1) as item (item.id)}
          <li><strong>{item.title}</strong>{item.detail}</li>
        {/each}
      </ul>
    {/if}

    {#if report.reportPath}
      <p class="t-mono path selectable">{report.reportPath}</p>
    {/if}
    {#if report.hsErrPath}
      <p class="t-mono path selectable">{report.hsErrPath}</p>
    {/if}

    <button class="btn btn--link raw" onclick={() => (showRaw = !showRaw)}>
      <ChevronRight size={13} strokeWidth={2} class={showRaw ? 'turned' : ''} />
      {showRaw ? '收起原始日志' : '查看原始日志'}
    </button>

    {#if showRaw}
      <pre class="scroll excerpt t-mono">{report.excerpt || '没有捕获到日志'}</pre>
    {/if}
  </div>

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

  .body {
    padding: 0 var(--s5);
  }

  .detail {
    margin: var(--s3) 0 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .fix {
    margin-top: var(--s2);
  }

  .suspects {
    margin: var(--s3) 0 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    line-height: 1.65;
  }

  .suspects strong {
    font-weight: 500;
  }

  .others {
    display: grid;
    gap: var(--s2);
    margin: var(--s4) 0 0;
    padding: 0;
    list-style: none;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .others strong {
    margin-right: var(--s2);
    color: var(--ink-2);
    font-weight: 500;
  }

  .path {
    margin: var(--s3) 0 0;
    color: var(--ink-4);
    overflow-wrap: anywhere;
  }

  .raw {
    margin: var(--s4) 0 0;
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
    margin: var(--s3) 0 0;
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
