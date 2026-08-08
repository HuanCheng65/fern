<script lang="ts">
  /**
   * 启动场景——正在播放（见 docs/UI_DESIGN.md 五）。
   *
   * 这一屏只展示一个东西：当前实例。它的群系封面就是整个舞台的背景，所以
   * 这里几乎没有 UI——实例名、一颗启动键、出事时的错误，仅此而已。这正是
   * 「界面隐去、世界向前」：启动场景本质上就是当前实例的封面艺术本身。
   *
   * 所有管理欲望都引导去实例场景：游戏目录、校验文件、模组、存档、设置全在
   * 那边的详情页里。打开启动器十秒就走的那九成会话，不该看见它们。
   *
   * 内容压在左下角，右边和上边整片留给背景。这不是没排满，是画框的意思。
   */
  import { ChevronDown, Play, X } from 'lucide-svelte'
  import AccountChip from '../components/AccountChip.svelte'
  import Stage from '../layouts/Stage.svelte'
  import { instances } from '../lib/instances.svelte'
  import { fraction, jobs, measure } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { preflight } from '../lib/preflight.svelte'

  interface Props {
    onswitch: () => void
    oncreate: () => void
  }

  let { onswitch, oncreate }: Props = $props()

  const current = $derived(instances.current)
  /**
   * 这颗按钮上的进度来自后端宣告的作业，不是本地攒的。
   *
   * 于是它对「谁发起的」免疫：从命令面板启动、从实例页启动、甚至上一次点完
   * 就切走了再回来——只要这个实例上有事在跑，按钮就还是那副样子。
   */
  const job = $derived(current ? jobs.forSubject(current.id) : undefined)
  const done = $derived(job ? fraction(job) : undefined)
  /** 这个实例现在跑到哪一段了。undefined 是没在跑。 */
  const phase = $derived(current ? launch.phaseOf(current.id) : undefined)
  const working = $derived(phase === 'preparing' || job !== undefined)
  /**
   * 启动之前能看出来的问题，只说大概率起不来的那些。
   *
   * 这一屏只有一颗按钮，不该变成一张检查清单——详细的几条在实例详情里。
   */
  const blocking = $derived(current ? preflight.blocking(current.id) : [])
  $effect(() => {
    if (current) void preflight.check(current.id)
  })
</script>

<Stage>
  {#if current}
      <button class="name" onclick={onswitch} title="切换实例">
        <span>{current.name}</span>
        <ChevronDown size={26} strokeWidth={1.6} />
      </button>

      <p class="meta t-mono">Minecraft {current.gameVersion} · {current.loader}</p>

      <div class="go-row">
        <!-- 游戏已经开着的时候不再提供「启动」：再点一下会起第二个进程，
             两份游戏抢同一个存档目录。 -->
        <button
          class="btn btn--primary go"
          class:busy={working}
          onclick={() => void launch.launch(current.id)}
          disabled={phase !== undefined || job !== undefined}
        >
          <span
            class="fill"
            class:pulse={working && done === undefined}
            style:width={done === undefined ? '100%' : `${done * 100}%`}
          ></span>
          <span class="go-text">
            {#if phase === 'running'}
              游戏运行中
            {:else if phase === 'starting'}
              正在启动
            {:else if job}
              {job.stage || job.title}
            {:else if working}
              准备中
            {:else}
              <Play size={16} fill="currentColor" strokeWidth={0} />启动游戏
            {/if}
          </span>
        </button>

        <!--
          结束只在游戏真的起来之后出现，而且说的是「强制」：这是 kill，没存
          的进度会丢。它存在的理由是游戏已经不响应了。
        -->
        {#if phase === 'running' || phase === 'starting'}
          <button class="btn btn--ghost" onclick={() => void launch.stop(current.id)}>
            强制结束
          </button>
        {/if}

        <!-- 用谁的身份，就站在启动键旁边——那是它唯一真正重要的时刻。 -->
        <AccountChip instanceId={current.id} />

        {#if job && measure(job)}
          <span class="detail t-mono">{measure(job)}</span>
        {/if}
      </div>

      <!--
        只说一句，不在这一屏展开：它不拦启动，但按下去多半会崩，用户有权在
        按之前知道。详细的几条在实例详情里。
      -->
      {#if blocking.length > 0}
        <button class="warn" onclick={() => nav.enter('instances', current.id)}>
          {blocking.length === 1
            ? blocking[0].title
            : `启动前有 ${blocking.length} 个问题`}<span class="t-quiet">查看</span>
        </button>
      {/if}

      {#if launch.error}
        <div class="alert error">
          <span>{launch.error}</span>
          <button class="btn btn--icon" aria-label="关闭" onclick={() => launch.dismissError()}>
            <X size={14} />
          </button>
        </div>
      {/if}
  {:else}
      <h1 class="t-display">创建第一个实例</h1>
      <div class="go-row">
        <button class="btn btn--primary" onclick={oncreate} disabled={instances.loading}>
          选择版本
        </button>
      </div>
      {#if instances.error}
        <div class="alert error"><span>{instances.error}</span></div>
      {/if}
  {/if}
</Stage>

<style>
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

  /*
   * 这一行上的每一个动作都和启动键一样高。
   *
   * 它们本来各按各的默认值：主按钮 44、次按钮 38、身份芯片跟着头像走。三个
   * 高度并排，读出来是三条底边，而这一行是整屏唯一的操作区——没有比这里更该
   * 对齐的地方。次要的东西靠颜色和分量退后，不靠矮一截。
   */
  .go-row .btn,
  .go-row :global(.chip) {
    min-height: var(--control-lg);
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

  /* 不是错误，是一条提醒——所以它安静，但点得开。 */
  .warn {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    margin-top: var(--s3);
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  .warn:hover {
    color: var(--ink);
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

  @media (max-width: 720px) {
    .go-row {
      flex-wrap: wrap;
      gap: var(--s3);
    }
  }
</style>
