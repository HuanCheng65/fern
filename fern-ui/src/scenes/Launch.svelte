<script lang="ts">
  /**
   * 启动场景——正在播放（见 docs/frond-design-system.md）。
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
  import { palette } from 'fern-kit/parts/palette'
  import LaunchHero from 'fern-kit/parts/LaunchHero.svelte'
  import Stage from '../layouts/Stage.svelte'
  import { accounts, launchIdentity, originOf, switchAction } from '../lib/accounts.svelte'
  import { instances } from '../lib/instances.svelte'
  import { aside, fraction, jobs } from '../lib/jobs.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { preflight } from '../lib/preflight.svelte'
  import { skins } from '../lib/skins.svelte'
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
   * 这一屏的脸直接交给 `LaunchHero`，没经过 `components/AccountFace.svelte`，
   * 所以取皮肤的那一次请求得在这里发——否则头一次打开永远是默认的 Steve/Alex，
   * 要等别的屏（账户名单、实例设置）替它问过一遍才会换成真的。
   */
  $effect(() => {
    if (identity) void skins.request(identity)
  })
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
      这一屏的样子在 `fern-kit/parts/LaunchHero.svelte`。留在这里的是产品才知道的
      事：谁是当前实例、按下启动用谁的身份、作业跑到哪、预检查拦不拦。
    -->
    <LaunchHero
      name={current.name}
      detail={`Minecraft ${current.gameVersion} · ${current.loader}`}
      identity={identity
        ? {
            name: identity.playerName,
            face: skins.face(identity),
            origin: ambiguous ? originOf(identity) : undefined,
          }
        : undefined}
      {salutation}
      noAccount={!identity && !accounts.loading}
      {phase}
      jobLabel={job ? job.stage || job.title : undefined}
      {done}
      {working}
      measure={job ? aside(job) : undefined}
      warn={blocking.length === 1
        ? blocking[0].title
        : blocking.length > 1
          ? `启动前有 ${blocking.length} 个问题`
          : undefined}
      error={launch.error}
      onidentity={switchIdentity}
      onaddaccount={() => nav.settings('account/list/new')}
      {onswitch}
      onmanage={() => nav.enter('instances', current.id)}
      onlaunch={() => void launch.launch(current.id)}
      onstop={() => void launch.stop(current.id)}
      onwarn={() => nav.enter('instances', current.id)}
      ondismiss={() => launch.dismissError()}
    />
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
  /* 空态：一个实例都没有的时候，这一屏只做一件事。启动那一屏的样子在
     fern-kit/parts/LaunchHero.svelte。 */
  .go-row {
    display: flex;
    align-items: center;
    gap: var(--s4);
    margin-top: var(--s5);
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
