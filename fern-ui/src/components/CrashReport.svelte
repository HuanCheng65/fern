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
  import { FolderOpen } from 'lucide-svelte'
  import Overlay from 'fern-kit/Overlay.svelte'
  import CrashPanel from 'fern-kit/CrashPanel.svelte'
  import { perform } from '../lib/advice'
  import { describe } from '../lib/i18n'
  import type { CrashReport } from '../lib/launch.svelte'

  interface Props {
    report: CrashReport
    onclose: () => void
    onopenLogs: () => void
  }

  let { report, onclose, onopenLogs }: Props = $props()

  const exit = $derived(
    report.exitCode === null ? '进程被系统终止' : `退出码 ${report.exitCode}`,
  )
  const found = $derived(
    report.diagnoses.map((diagnosis) => ({
      ...diagnosis,
      ...describe(`crash.${diagnosis.id}`, diagnosis.args),
    })),
  )
</script>

<Overlay label="游戏异常退出" width="600px" {onclose}>
  <CrashPanel
    {found}
    {exit}
    suspects={report.suspects}
    reportPath={report.reportPath}
    hsErrPath={report.hsErrPath}
    excerpt={report.excerpt}
    onfix={() => perform(found[0].action!, report.instanceId)}
  />

  <footer>
    <button class="btn btn--ghost" onclick={onopenLogs}>
      <FolderOpen size={13} strokeWidth={1.9} />日志目录
    </button>
    <button class="btn btn--primary" onclick={onclose}>关闭</button>
  </footer>
</Overlay>

<style>
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s3);
    padding: var(--s5);
  }
</style>
