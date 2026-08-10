<script lang="ts">
  /**
   * 实例列表里的一张卡。
   *
   * 封面是这张卡的主视觉，名字和版本在下面，启动键悬停才出现——「我就想立刻玩
   * 这个」的那条路径，不必先进详情。
   *
   * 当前实例标在名字旁边，不标在封面上。上一版在封面外沿描了一圈强调色，一道硬边
   * 贴着一张生成的图，读起来是「这张图被选中了」；而要说的是「这个实例是当前的」，
   * 那句话属于名字，不属于画。
   */
  import { Play } from 'lucide-svelte'
  import Cover from '../ui/Cover.svelte'

  interface Props {
    name: string
    /** 封面的种子。产品里是实例的 cover 字段，通常就是实例名。 */
    cover: string
    /** 名字下面那一行，例如 `1.20.1 · Fabric`。 */
    detail?: string
    /** 是不是当前实例。 */
    current?: boolean
    /** 正在启动或已占用时，启动键不出现。 */
    busy?: boolean
    onopen?: () => void
    onlaunch?: () => void
  }

  let { name, cover, detail, current = false, busy = false, onopen, onlaunch }: Props = $props()
</script>

<div class="card">
  <button class="face" onclick={() => onopen?.()} title="打开 {name}">
    <Cover seed={cover} quality={0.55} />
  </button>

  {#if onlaunch}
    <button
      class="go"
      aria-label="启动 {name}"
      title="启动"
      disabled={busy}
      onclick={() => onlaunch()}
    >
      <Play size={14} fill="currentColor" strokeWidth={0} />
    </button>
  {/if}

  <button class="text" onclick={() => onopen?.()}>
    <span class="line">
      <strong>{name}</strong>
      {#if current}<span class="now">当前</span>{/if}
    </span>
    {#if detail}<small class="t-mono">{detail}</small>{/if}
  </button>
</div>

<style>
  .card {
    position: relative;
    display: grid;
    gap: var(--s2);
  }

  .face {
    display: block;
    width: 100%;
    aspect-ratio: 4 / 3;
    padding: 0;
    overflow: hidden;
    border-radius: var(--r2);
    background: var(--tint-1);
    transition:
      transform var(--t-base) var(--ease),
      box-shadow var(--t-base) var(--ease);
  }

  .card:hover .face {
    transform: translateY(-2px);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-width: 0;
  }

  .now {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
    font-size: var(--t-micro);
    line-height: 1.7;
  }

  .go {
    position: absolute;
    top: var(--s2);
    right: var(--s2);
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: rgba(10, 14, 16, 0.6);
    color: #f3f6f6;
    opacity: 0;
    transform: scale(0.9);
    -webkit-backdrop-filter: blur(8px);
    backdrop-filter: blur(8px);
    transition:
      opacity var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .card:hover .go,
  .go:focus-visible {
    opacity: 1;
    transform: none;
  }

  .go:disabled {
    display: none;
  }

  .text {
    display: grid;
    gap: 1px;
    padding: 0;
    min-width: 0;
    text-align: left;
  }

  .text strong {
    min-width: 0;
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }
</style>
