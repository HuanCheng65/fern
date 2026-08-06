<script lang="ts">
  /**
   * 启动场景（见 docs/UI_DESIGN.md 五）。
   *
   * 首页只回答一个问题：现在玩哪个。所以这一屏上有名字的东西只有三样——
   * 实例名、启动键、出事时的错误。其余全部退到二级。
   *
   * 内容压在左下角，右边和上边整片留给背景。这不是没排满，是画框的意思：
   * 界面自己不画花纹，装饰由生成式封面承担。
   */
  import { ChevronDown, FolderOpen, Play, RefreshCw, X } from 'lucide-svelte'
  import { instances } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { prefs } from '../lib/prefs.svelte'

  interface Props {
    onswitch: () => void
    oncreate: () => void
    onopenDirectory: () => void
  }

  let { onswitch, oncreate, onopenDirectory }: Props = $props()

  const current = $derived(instances.current)
</script>

<section class="launch">
  {#if current}
    <div class="copy">
      <button class="name" onclick={onswitch} title="切换实例">
        <span>{current.name}</span>
        <ChevronDown size={26} strokeWidth={1.6} />
      </button>

      <p class="meta t-mono">Minecraft {current.gameVersion} · {current.loader}</p>

      <div class="go-row">
        <button
          class="btn btn--primary go"
          class:busy={launch.busy}
          onclick={() => void launch.launch(current.id, prefs.playerName)}
          disabled={launch.busy}
        >
          <span
            class="fill"
            class:pulse={launch.busy && launch.progress < 0}
            style:width={launch.progress >= 0 ? `${launch.progress}%` : '100%'}
          ></span>
          <span class="go-text">
            {#if launch.busy}
              {launch.label || '准备中'}
            {:else}
              <Play size={16} fill="currentColor" strokeWidth={0} />启动游戏
            {/if}
          </span>
        </button>

        {#if launch.busy && launch.detail}
          <span class="detail t-mono">{launch.detail}</span>
        {/if}
      </div>

      {#if launch.error}
        <div class="alert error">
          <span>{launch.error}</span>
          <button class="btn btn--icon" aria-label="关闭" onclick={() => launch.dismissError()}>
            <X size={14} />
          </button>
        </div>
      {/if}

      <div class="secondary">
        <button class="btn btn--link" onclick={onopenDirectory}>
          <FolderOpen size={13} strokeWidth={1.9} />游戏目录
        </button>
        <button class="btn btn--link" onclick={() => void launch.repair(current.id)}>
          <RefreshCw size={13} strokeWidth={1.9} />校验文件
        </button>
      </div>
    </div>
  {:else}
    <div class="copy">
      <h1 class="t-display">创建第一个实例</h1>
      <div class="go-row">
        <button class="btn btn--primary" onclick={oncreate} disabled={instances.loading}>
          选择版本
        </button>
      </div>
      {#if instances.error}
        <div class="alert error"><span>{instances.error}</span></div>
      {/if}
    </div>
  {/if}
</section>

<style>
  /* 内容坐在左下角，上方和右方是留白。 */
  .launch {
    display: flex;
    align-items: flex-end;
    height: 100%;
    padding-bottom: var(--s2);
  }

  .copy {
    width: min(620px, 100%);
  }

  /* 实例名同时是切换器的入口——文档里说点实例名呼出切换器。 */
  .name {
    display: flex;
    align-items: center;
    gap: var(--s3);
    max-width: 100%;
    padding: 0;
    color: var(--ink);
    font-size: var(--t-display);
    font-weight: 620;
    line-height: 1.02;
    letter-spacing: -0.035em;
    text-align: left;
    transition: color var(--t-fast) var(--ease);
  }

  .name span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name :global(svg) {
    flex: none;
    color: var(--ink-4);
    transition:
      color var(--t-fast) var(--ease),
      transform var(--t-base) var(--spring);
  }

  .name:hover :global(svg) {
    color: var(--accent);
    transform: translateY(2px);
  }

  .meta {
    margin: var(--s3) 0 0;
    color: var(--ink-3);
  }

  .go-row {
    display: flex;
    align-items: center;
    gap: var(--s4);
    margin-top: var(--s5);
  }

  /* 启动是英雄交互，进度就长在按钮上，不另起一个进度条区域。 */
  .go {
    position: relative;
    isolation: isolate;
    min-width: 190px;
    overflow: hidden;
  }

  .go.busy {
    cursor: progress;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    z-index: -1;
    background: rgba(0, 0, 0, 0.24);
    transition: width var(--t-slow) var(--ease);
  }

  /* 进度未知时不停在 0%，让一道暗光自己走一趟。 */
  .fill.pulse {
    background: linear-gradient(90deg, transparent, rgba(0, 0, 0, 0.26) 50%, transparent);
    animation: sweep 1.6s var(--ease) infinite;
  }

  @keyframes sweep {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(100%);
    }
  }

  .go-text {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
  }

  .detail {
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }

  .error {
    display: flex;
    align-items: flex-start;
    gap: var(--s3);
    max-width: 62ch;
    margin-top: var(--s4);
  }

  .error span {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .secondary {
    display: flex;
    gap: var(--s5);
    margin-top: var(--s5);
  }

  .secondary .btn {
    gap: 6px;
    color: var(--ink-3);
  }

  .secondary .btn:hover {
    color: var(--ink);
  }

  @media (max-width: 720px) {
    .go-row {
      flex-wrap: wrap;
      gap: var(--s3);
    }
  }
</style>
