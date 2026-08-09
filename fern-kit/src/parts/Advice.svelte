<script lang="ts">
  /**
   * 一条诊断：说清是什么，能修就给一颗按钮。
   *
   * 崩溃分析和启动前预检查共用它——两者给出的是同一种东西（一句话加一个可选
   * 的动作），只是发生的时刻不同。
   *
   * 它不知道那颗按钮按下去会发生什么，只知道该不该有：`label()` 说了算。真要
   * 做的事由调用方给（`onfix`），做的过程中的忙碌态和失败话术归这里。
   */
  import { AlertTriangle, CircleAlert } from 'lucide-svelte'
  import Button from '../ui/Button.svelte'
  import { label, type FixAction } from './advice'

  interface Props {
    title: string
    detail: string
    /** blocking 大概率起不来，warning 只是可能有问题。 */
    tone?: 'blocking' | 'warning'
    action?: FixAction
    /** 真的去做那件事。不给也照样画按钮——按钮在不在是 label 决定的。 */
    onfix?: () => Promise<void> | void
  }

  let { title, detail, tone = 'blocking', action, onfix }: Props = $props()

  let busy = $state(false)
  let error = $state('')

  const actionLabel = $derived(label(action))

  async function run() {
    if (!onfix) return
    busy = true
    error = ''
    try {
      await onfix()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }
</script>

<div class="advice" class:warning={tone === 'warning'}>
  <span class="icon">
    {#if tone === 'warning'}
      <AlertTriangle size={15} strokeWidth={1.9} />
    {:else}
      <CircleAlert size={15} strokeWidth={1.9} />
    {/if}
  </span>
  <div class="text">
    <strong>{title}</strong>
    {#if detail}<p>{detail}</p>{/if}
    {#if error}<p class="failed">{error}</p>{/if}
  </div>
  {#if actionLabel}
    <div class="fix">
      <Button variant="ghost" disabled={busy} onclick={() => void run()}>
        {busy ? '处理中' : actionLabel}
      </Button>
    </div>
  {/if}
</div>

<style>
  .advice {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    padding: var(--s3) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .advice:last-child {
    box-shadow: none;
  }

  .icon {
    display: grid;
    place-items: center;
    flex: none;
    margin-top: 1px;
    color: var(--danger, var(--ink-2));
  }

  .advice.warning .icon {
    color: var(--ink-3);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .text strong {
    display: block;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .text p {
    margin: 2px 0 0;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .failed {
    color: var(--danger, var(--ink-2));
  }

  .fix {
    flex: none;
  }
</style>
