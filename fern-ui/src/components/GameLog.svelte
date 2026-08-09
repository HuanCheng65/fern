<script lang="ts">
  /**
   * 游戏日志（浮层）。
   *
   * 平时不该出现——这一屏存在的理由只有一个：出事了要能看见发生了什么，
   * 以及能整段复制给别人。所以它是浮层不是场景，从命令面板或顶栏的状态块进。
   *
   * 实例详情页里也有同一段日志，那边是 tab。两处的渲染共用 LogLines，
   * 免得过滤和配色在两个地方各长各的。
   */
  import { X } from 'lucide-svelte'
  import Dialog from 'fern-kit/ui/Dialog.svelte'
  import LogLines from './LogLines.svelte'
  import { launch } from '../lib/launch.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface Props {
    onclose: () => void
  }

  let { onclose }: Props = $props()

  const counts = $derived({
    warn: launch.log.filter((line) => line.level === 'warn').length,
    error: launch.log.filter((line) => line.level === 'error').length,
  })
</script>

<Dialog label="游戏日志" width="820px" {onclose}>
  <header>
    <div>
      <h2 class="t-h2">游戏日志</h2>
      <p class="t-quiet">
        共 {launch.log.length} 行{counts.error > 0 ? ` · ${counts.error} 条错误` : ''}{counts.warn > 0
          ? ` · ${counts.warn} 条警告`
          : ''}
      </p>
    </div>
    <Button variant="icon" aria-label="关闭" onclick={onclose}><X size={16} /></Button>
  </header>

  <div class="body">
    <LogLines lines={launch.log} />
  </div>
</Dialog>

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

  .body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: 62vh;
    padding: 0 var(--s5) var(--s4);
  }
</style>
