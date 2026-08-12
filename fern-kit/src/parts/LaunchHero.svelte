<script lang="ts">
  /**
   * 启动那一屏（见 docs/UI_DESIGN.md 五）。
   *
   * 这一屏只展示一个东西：当前实例。它的群系封面就是整个舞台的背景，所以这里几乎
   * 没有 UI——实例名、一颗启动键、出事时的错误，仅此而已。这正是「界面隐去、世界向
   * 前」：启动场景本质上就是当前实例的封面艺术本身。
   *
   * 内容压在左下角，右边和上边整片留给背景。这不是没排满，是画框的意思。
   *
   * **题头那句问候不是装饰。** 封面的环境种子本来就在按真实时间调色温，问候语是同
   * 一个信号的文字面——一屏之内画面和语言说同一件事。而它招呼的是**你即将扮演的那
   * 个身份**，不是键盘前的人。
   *
   * 拿到的全是纯值：谁在跑、跑到哪一段、有没有问题，都是产品那边折出来的。作业、
   * 预检查、账户体系不进这里。
   */
  import { ChevronDown, Play, Plus, X } from 'lucide-svelte'
  import AccountFace from './AccountFace.svelte'
  import Button from '../ui/Button.svelte'

  interface Identity {
    name: string
    face: { url: string; hat: boolean }
    /** 只在这个名字确实有第二个人在用时才写出来。例外才发声。 */
    origin?: string
  }

  interface Props {
    name: string
    /** 名字下面那一行，例如 `Minecraft 1.20.1 · Fabric`。 */
    detail: string
    /** 按下启动会用谁的身份。没有身份时传空。 */
    identity?: Identity
    /** 问候语，跟着真实时间走。 */
    salutation?: string
    /** 一个账户都没有——这一行就是这一屏此刻唯一该做的事。 */
    noAccount?: boolean
    /** 这个实例现在跑到哪一段。留空是没在跑。 */
    phase?: 'preparing' | 'starting' | 'running'
    /** 有作业在跑时，按钮上写的那一句。 */
    jobLabel?: string
    /** 0–1。留空表示进度未知，填充会自己走一趟。 */
    done?: number
    working?: boolean
    /** 作业的机器读数，等宽显示。 */
    measure?: string
    /** 启动前的问题，一句话。详细的几条在实例详情里。 */
    warn?: string
    error?: string
    onidentity?: () => void
    onaddaccount?: () => void
    onswitch?: () => void
    onmanage?: () => void
    onlaunch?: () => void
    onstop?: () => void
    onwarn?: () => void
    ondismiss?: () => void
  }

  let {
    name,
    detail,
    identity,
    salutation = '你好',
    noAccount = false,
    phase,
    jobLabel,
    done,
    working = false,
    measure,
    warn,
    error,
    onidentity,
    onaddaccount,
    onswitch,
    onmanage,
    onlaunch,
    onstop,
    onwarn,
    ondismiss,
  }: Props = $props()
</script>

<!-- 题头。它在实例名之上，读下来是「以这个身份 · 玩这个世界 · 启动」。 -->
{#if identity}
  <button class="hail" onclick={() => onidentity?.()} title="切换账户">
    <span class="salute">{salutation}，</span>
    <AccountFace face={identity.face} size={26} round />
    <span class="who">{identity.name}</span>
    {#if identity.origin}<span class="origin">· {identity.origin}</span>{/if}
    <ChevronDown size={15} strokeWidth={1.8} />
  </button>
{:else if noAccount}
  <button class="hail none" onclick={() => onaddaccount?.()}>
    <Plus size={15} strokeWidth={2} />尚未添加账户
  </button>
{/if}

<button class="name" onclick={() => onswitch?.()} title="切换实例">
  <span>{name}</span>
  <ChevronDown size={26} strokeWidth={1.6} />
</button>

<p class="meta t-mono">
  {detail}
  <!-- 这一屏把管理欲望引去实例详情，却一直没给出那扇门。就在这里。 -->
  <Button variant="link" onclick={() => onmanage?.()}>管理</Button>
</p>

<div class="go-row">
  <!-- 游戏已经开着的时候不再提供「启动」：再点一下会起第二个进程，两份游戏抢同一个
       存档目录。 -->
  <Button
    variant="primary"
    class="go"
    onclick={() => onlaunch?.()}
    loading={working}
    progress={working ? done : undefined}
    disabled={phase !== undefined || jobLabel !== undefined}
  >
    {#if phase === 'running'}
      游戏运行中
    {:else if phase === 'starting'}
      等待游戏窗口
    {:else if jobLabel}
      {jobLabel}
    {:else if working}
      准备中
    {:else}
      <Play size={16} fill="currentColor" strokeWidth={0} />启动游戏
    {/if}
  </Button>

  <!-- 结束只在游戏真的起来之后出现，而且说的是「强制」：这是 kill，没存的进度会丢。
       它存在的理由是游戏已经不响应了。 -->
  {#if phase === 'running' || phase === 'starting'}
    <Button variant="ghost" onclick={() => onstop?.()}>强制结束</Button>
  {/if}

  {#if measure}
    <span class="detail t-mono">{measure}</span>
  {/if}
</div>

<!-- 只说一句，不在这一屏展开：它不拦启动，但按下去多半会崩，用户有权在按之前知道。 -->
{#if warn}
  <button class="warn" onclick={() => onwarn?.()}>
    {warn}<span class="t-quiet">查看</span>
  </button>
{/if}

{#if error}
  <div class="alert error">
    <span>{error}</span>
    <Button variant="icon" aria-label="关闭" onclick={() => ondismiss?.()}>
      <X size={14} />
    </Button>
  </div>
{/if}

<style>
  /*
   * 题头。19px 而不是 12px——弄错身份的代价是进错服、白名单不认、存档里那不是你的
   * 背包，重量该跟代价走，不跟操作频次走。实例名仍然是它的两三倍，主角没变。
   */
  .hail {
    display: inline-flex;
    align-items: center;
    gap: var(--s1);
    max-width: 100%;
    margin-bottom: var(--s3);
    padding: 0;
    color: var(--ink-2);
    font-size: var(--t-h2);
    font-weight: 480;
    letter-spacing: -0.01em;
    transition: color var(--t-fast) var(--ease);
  }

  .hail:hover {
    color: var(--ink);
  }

  .salute {
    /* 逗号自己撑开了间距，再给一格就散了。 */
    margin-right: calc(var(--s1) * -1);
  }

  .who {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 出处比名字轻一档：它是用来分辨的，不是名字的一部分。 */
  .origin {
    color: var(--ink-3);
    font-size: var(--t-body);
  }

  .hail :global(svg) {
    flex: none;
    margin-left: var(--s1);
    color: var(--ink-4);
    transition:
      color var(--t-fast) var(--ease),
      transform var(--t-base) var(--spring);
  }

  .hail:hover :global(svg) {
    color: var(--accent);
    transform: translateY(2px);
  }

  /* 没有账户是这一屏的空态，不是一句提示：它比问候语更该被看见。 */
  .hail.none {
    color: var(--ink);
    font-size: var(--t-body);
  }

  .hail.none :global(svg) {
    margin-left: 0;
    color: var(--accent);
  }

  .hail.none:hover :global(svg) {
    transform: none;
  }

  /* 实例名同时是切换器的入口。 */
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
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    margin: var(--s3) 0 0;
    color: var(--ink-3);
  }

  /* 启动是英雄交互，进度就长在按钮上，不另起一个进度条区域。 */
  .go-row {
    display: flex;
    align-items: center;
    gap: var(--s4);
    margin-top: var(--s5);
  }

  /* 进度长在按钮内部那套画法已经收进 ui/Button.svelte，这里只管它占多大。 */
  .go-row :global(.go) {
    min-width: 190px;
    min-height: var(--control-lg);
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
