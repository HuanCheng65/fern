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
   *
   * **题头那句问候不是装饰。** 封面的环境种子本来就在按真实时间调色温（文档
   * 二），问候语是同一个信号的文字面——一屏之内画面和语言说同一件事。而它招呼
   * 的是**你即将扮演的那个身份**，不是键盘前的人：所以固定用小号的实例上写着
   * 「晚上好，小号」并不别扭，反而正好省掉一行解释。
   *
   * 身份原来是启动键旁边一枚和它等高的玻璃胶囊。那个形状有两处错：它把「确认
   * 我是谁」这件每次都发生的**被动**事情，做成了一个和启动并列的**控件**；而且
   * 它只写名字——而一个人名下同时挂着正版的 Steve、离线的 Steve 和某个皮肤站上
   * 的 Steve 是完全合法的状态（`roster.rs` 的去重键是 kind + uuid + 皮肤站），
   * 光有名字根本分不开。
   */
  import { ChevronDown, Play, Plus, X } from 'lucide-svelte'
  import { palette } from 'fern-kit/parts/palette'
  import AccountFace from '../components/AccountFace.svelte'
  import Stage from '../layouts/Stage.svelte'
  import { accounts, launchIdentity, originOf, switchAction } from '../lib/accounts.svelte'
  import { instances } from '../lib/instances.svelte'
  import { fraction, jobs, measure } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { preflight } from '../lib/preflight.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

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

  /** 按下启动会用谁的身份。和后端的 `roster::for_instance` 是同一条规则。 */
  const identity = $derived(current ? launchIdentity(current.id) : undefined)
  /**
   * 出处只在这个名字确实有第二个人在用时才写出来。
   *
   * 例外才发声：只有一个 Steve 的时候补一句「正版」，说的是一件没人问的事。
   */
  const ambiguous = $derived(identity ? accounts.duplicated.has(identity.playerName) : false)

  /**
   * 问候语跟着真实时间走，和封面的环境种子同一个信号。
   *
   * 每分钟对一次表：启动器可以整夜开着，跨过午夜还说「下午好」比不打招呼更糟。
   * 不写「夜深了」那种——那是在劝人做什么，文案规范里明确不做这件事。
   */
  let hour = $state(new Date().getHours())
  $effect(() => {
    const timer = setInterval(() => (hour = new Date().getHours()), 60_000)
    return () => clearInterval(timer)
  })
  const salutation = $derived(hour >= 18 || hour < 5 ? '晚上好' : hour >= 12 ? '下午好' : '早上好')

  /**
   * 换一个身份走命令面板，和上面点实例名呼出切换器是同一个东西。
   *
   * 于是这一屏只有一条规则：点一个名词，面板预过滤到那个名词的类型。两个 `⌄`
   * 说的是同一句话，学一次通两处。「添加账户」由面板自己接住——那个动作声明了
   * `creates: 'account'`，所以它永远是这份名单的最后一行。
   */
  function switchIdentity() {
    palette.open({ kind: 'subjects', type: 'account', label: '身份', action: switchAction() })
    nav.show('palette')
  }
</script>

<Stage>
  {#if current}
      <!--
        题头。它在实例名之上，读下来是「以这个身份 · 玩这个世界 · 启动」。
      -->
      {#if identity}
        <button class="hail" onclick={switchIdentity} title="切换账户">
          <span class="salute">{salutation}，</span>
          <AccountFace account={identity} size={26} round />
          <span class="who">{identity.playerName}</span>
          {#if ambiguous}<span class="origin">· {originOf(identity)}</span>{/if}
          <ChevronDown size={15} strokeWidth={1.8} />
        </button>
      {:else if !accounts.loading}
        <!-- 一个账户都没有时，这一行就是这一屏此刻唯一该做的事。 -->
        <button class="hail none" onclick={() => nav.show('settings', 'account/list/new')}>
          <Plus size={15} strokeWidth={2} />尚未添加账户
        </button>
      {/if}

      <button class="name" onclick={onswitch} title="切换实例">
        <span>{current.name}</span>
        <ChevronDown size={26} strokeWidth={1.6} />
      </button>

      <p class="meta t-mono">
        Minecraft {current.gameVersion} · {current.loader}
        <!-- 这一屏把管理欲望引去实例详情，却一直没给出那扇门。就在这里。 -->
        <Button variant="link" onclick={() => nav.enter('instances', current.id)}>
          管理
        </Button>
      </p>

      <div class="go-row">
        <!-- 游戏已经开着的时候不再提供「启动」：再点一下会起第二个进程，
             两份游戏抢同一个存档目录。 -->
        <Button
          variant="primary"
          class="go {working ? 'busy' : ''}"
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
        </Button>

        <!--
          结束只在游戏真的起来之后出现，而且说的是「强制」：这是 kill，没存
          的进度会丢。它存在的理由是游戏已经不响应了。
        -->
        {#if phase === 'running' || phase === 'starting'}
          <Button variant="ghost" onclick={() => void launch.stop(current.id)}>
            强制结束
          </Button>
        {/if}

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
          <Button variant="icon" aria-label="关闭" onclick={() => launch.dismissError()}>
            <X size={14} />
          </Button>
        </div>
      {/if}
  {:else}
      <h1 class="t-display">创建第一个实例</h1>
      <div class="go-row">
        <Button variant="primary" onclick={oncreate} disabled={instances.loading}>
          选择版本
        </Button>
      </div>
      {#if instances.error}
        <div class="alert error"><span>{instances.error}</span></div>
      {/if}
  {/if}
</Stage>

<style>
  /*
   * 题头。19px 而不是 12px——弄错身份的代价是进错服、白名单不认、存档里那不是
   * 你的背包，重量该跟代价走，不跟操作频次走。实例名仍然是它的两三倍，主角没变。
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
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    margin: var(--s3) 0 0;
    color: var(--ink-3);
  }

  /* 等宽只留给机器数据，「管理」两个字不是。 */
  .go-row {
    display: flex;
    align-items: center;
    gap: var(--s4);
    margin-top: var(--s5);
  }

  /* 启动是英雄交互，进度就长在按钮上，不另起一个进度条区域。 */
  /* 布局归调用方，但 Svelte 的作用域样式进不了组件，所以罩一层自己的祖先。 */
  .go-row :global(.go) {
    position: relative;
    isolation: isolate;
    min-width: 190px;
    min-height: var(--control-lg);
    overflow: hidden;
  }

  .go-row :global(.go.busy) {
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
