<script lang="ts">
  /**
   * 通知层：右下角那一摞说完就走的短句。见 lib/notices.svelte.ts。
   *
   * 位置选右下不是随手定的：岛在顶栏右段，两者都靠右形成一条视线通道，而上下
   * 分开保证它们永远不会互相遮挡——岛说的是正在发生的事，这里说的是刚发生完
   * 的事，同时出现是常态（装完一个模组的那一刻，另一个还在装）。
   *
   * 不用 Overlay：那是**打断**当前场景的东西，带景深遮罩、要人处理完才走。
   * 通知不打断任何事，它连指针都不该挡住——除了它自己那块。
   */
  import { Check, TriangleAlert, X } from 'lucide-svelte'
  import { DURATION, scaled } from '../lib/motion'
  import { notices } from '../lib/notices.svelte'
  import { fly } from 'svelte/transition'
  import { flip } from 'svelte/animate'
  import Button from 'fern-kit/ui/Button.svelte'
</script>

{#if notices.list.length > 0}
  <div class="dock" role="status" aria-live="polite">
    {#each notices.list as notice (notice.id)}
      <div
        class="note {notice.tone}"
        animate:flip={{ duration: scaled(DURATION.base) }}
        in:fly={{ y: 12, duration: scaled(DURATION.base) }}
        out:fly={{ y: 8, duration: scaled(DURATION.fast) }}
        onmouseenter={() => notices.hold(notice.id)}
        onmouseleave={() => notices.release(notice.id)}
        role="presentation"
      >
        <span class="glyph">
          {#if notice.tone === 'warn'}
            <TriangleAlert size={14} strokeWidth={2.2} />
          {:else}
            <Check size={14} strokeWidth={2.6} />
          {/if}
        </span>

        <div class="words">
          <strong>{notice.title}</strong>
          {#if notice.detail}<p class="detail">{notice.detail}</p>{/if}
        </div>

        {#if notice.action}
          <Button
            variant="link"
            class="act"
            onclick={() => {
              notice.action?.run()
              notices.dismiss(notice.id)
            }}>
            {notice.action.label}
          </Button>
        {/if}
        <button class="close" aria-label="关闭" onclick={() => notices.dismiss(notice.id)}>
          <X size={12} strokeWidth={2.2} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .dock {
    position: fixed;
    right: var(--s5);
    bottom: var(--s5);
    z-index: 39;
    display: grid;
    gap: var(--s2);
    justify-items: end;
    /* 容器不吃指针，只有每一条自己吃：通知不该在整个右下角挡住底下的东西。 */
    pointer-events: none;
  }

  .note {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    max-width: 380px;
    padding: var(--s3) var(--s4);
    border-radius: var(--r2);
    background: var(--panel);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      0 10px 30px rgba(4, 6, 8, 0.28);
    -webkit-backdrop-filter: blur(20px) saturate(1.2);
    backdrop-filter: blur(20px) saturate(1.2);
    pointer-events: auto;
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    margin-top: 1px;
    color: var(--accent);
  }

  /* 色板里只有一个警示色（tokens.css 是唯一来源，不在这里造第二个）。 */
  .note.warn .glyph {
    color: var(--danger);
  }

  .words {
    min-width: 0;
  }

  .words strong {
    display: block;
    color: var(--ink);
    font-size: var(--t-small);
    font-weight: 500;
  }

  .detail {
    margin: 2px 0 0;
    color: var(--ink-3);
    font-size: var(--t-micro);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .act {
    flex: none;
    margin-top: -1px;
  }

  .close {
    flex: none;
    margin-top: 1px;
    color: var(--ink-4);
    transition: color var(--t-fast) var(--ease);
  }

  .close:hover {
    color: var(--ink-2);
  }
</style>
