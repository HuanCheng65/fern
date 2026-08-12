<script lang="ts">
  /**
   * 首次启动向导。
   *
   * 原则和启动器本身一致：一屏一件事，能自动的不问，能跳过的可跳过。
   * 第一次打开真正绕不开的只有两件——你是谁，文件从哪下。其余全部留给
   * 设置页：游戏目录默认值就是对的，内存该自动算，主题该先让人看到默认
   * 长什么样。向导每多一屏，「安静、清晰」的第一印象就掉一分。
   *
   * Java 那一屏是条件分支：扫得到就静默用上，什么都没有才出现。
   *
   * 空间语言和正式界面对齐——步骤之间也是镜头横向平移，不是弹窗叠弹窗。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { fly } from 'svelte/transition'
  import { ArrowLeft, ArrowRight, ExternalLink, Plus } from 'lucide-svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import Mark from 'fern-kit/ui/Mark.svelte'
  import { theme } from '../lib/theme.svelte'
  import { prefs, suggestedSource, type DownloadSource } from '../lib/prefs.svelte'
  import { accounts, verificationTarget } from '../lib/accounts.svelte'
  import { offlineLoginAllowed } from '../lib/region'
  import { inTauri, instances } from '../lib/instances.svelte'
  import Button from 'fern-kit/ui/Button.svelte'
  import RadioGroup from 'fern-kit/ui/RadioGroup.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

  interface Props {
    /** 走完向导。create 为真时直接进到新建实例。 */
    ondone: (create: boolean) => void
  }

  let { ondone }: Props = $props()

  type StepId = 'welcome' | 'account' | 'source' | 'java' | 'existing' | 'done'

  let index = $state(0)
  let direction = $state(1)

  /**
   * 登录这一步现在在哪一屏。
   *
   * 三种方式不是平级的三个选项：正版登录是绝大多数人该走的那一条，另外两条
   * 各有各的前提。摆成一组竖排单选，等于让每个人都先做一道自己没有依据的
   * 选择题；而真正的登录还被推到设置里，于是选完「微软账户」的人反而什么都
   * 没登上。所以这一步默认就是正版登录本身——能在这里登完，另外两条留成
   * 两个入口，进去是各自的一屏。
   */
  type LoginView = 'microsoft' | 'authlib' | 'offline'
  let loginView = $state<LoginView>('microsoft')
  /** 离线登录按地区提供，见 lib/region.ts。 */
  const offlineAllowed = offlineLoginAllowed()

  let playerName = $state('')
  let nameError = $state('')
  let apiRoot = $state('https://littleskin.cn/api/yggdrasil')
  let username = $state('')
  let password = $state('')

  const OFFLINE_NAME = /^[A-Za-z0-9_]{3,16}$/
  /** 官方商店。离线那一屏要给得出一条通往正版的路。 */
  const BUY_URL = 'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc'

  /** 交给系统浏览器。后端只放行 https。 */
  const openExternal = (url: string) => void invoke('open_external', { url })

  let source = $state<DownloadSource>(suggestedSource())
  const recommended = suggestedSource()
  const sourceName = { official: '官方源', bmclapi: 'BMCLAPI' } as const

  /**
   * 可执行文件旁边那个 `.minecraft`。
   *
   * 把启动器和游戏放在一起是最常见的用法，那种情况下用户期待它自己发现，
   * 而不是自己去设置里找一个路径。找不到就没有这一屏——和 Java 那一屏一样，
   * 是条件分支，不是固定步骤。
   */
  let nearby = $state('')

  /** 那一屏里的导入组件，这一步的主按钮按下时由它提交。 */
  let adopt = $state<{ commit: () => Promise<void> } | null>(null)
  let adoptStatus = $state({ chosen: 0, busy: false })

  /** unknown：还没查过。missing：查过了，系统里没有可用的 Java。 */
  let java = $state<'unknown' | 'ok' | 'missing'>('unknown')
  let javaChecking = $state(false)
  let javaDetail = $state('')

  const steps = $derived<StepId[]>([
    'welcome',
    'account',
    'source',
    ...(java === 'missing' ? (['java'] as StepId[]) : []),
    ...(nearby ? (['existing'] as StepId[]) : []),
    'done',
  ])
  const step = $derived(steps[Math.min(index, steps.length - 1)]!)

  const enter = $derived({
    x: direction * 34,
    duration: Math.round(220 * theme.motionScale),
    opacity: 0,
  })

  function go(delta: number) {
    direction = delta
    index = Math.max(0, Math.min(steps.length - 1, index + delta))
  }

  /**
   * 有没有可用的 Java。命令还没上线时（invoke 抛错）当作有——绝大多数系统
   * 里是有的，为一个查不到的结论多弹一屏，比不问更糟。
   */
  async function checkJava() {
    if (!inTauri()) return (java = 'ok')
    javaChecking = true
    try {
      const found = await invoke<{ path: string; major: number } | null>('detect_java')
      java = found ? 'ok' : 'missing'
      javaDetail = found ? `${found.path} · Java ${found.major}` : ''
    } catch {
      java = 'ok'
    } finally {
      javaChecking = false
    }
  }

  function pickLogin(next: LoginView) {
    loginView = next
    accounts.error = ''
    nameError = ''
  }

  /**
   * 登录成功之后往下走。
   *
   * 要先确认人还站在这一步：正版登录要等用户去浏览器里输码，这中间他完全可能
   * 按了「稍后登录」自己往前走了，那时候再推一步就是把他从下一屏挤走。
   */
  function advanceAfterLogin() {
    if (step === 'account') go(1)
  }

  async function loginMicrosoft() {
    await accounts.loginMicrosoft()
    if (!accounts.error) advanceAfterLogin()
  }

  async function loginYggdrasil() {
    if (!apiRoot.trim() || !username.trim() || !password) return
    await accounts.loginYggdrasil(apiRoot.trim(), username.trim(), password)
    // 拿到令牌就把密码从内存里去掉，它已经没有用处了。
    password = ''
    if (!accounts.error) advanceAfterLogin()
  }

  async function addOffline() {
    const value = playerName.trim()
    if (!OFFLINE_NAME.test(value)) {
      nameError = '3–16 位字母、数字或下划线'
      return
    }
    await accounts.addOffline(value)
    if (accounts.error) {
      nameError = accounts.error
      return
    }
    advanceAfterLogin()
  }

  function submitSource() {
    prefs.setDownloadSource(source)
    void Promise.all([checkJava(), findNearby()]).then(() => go(1))
  }

  /** 旁边有没有一个现成的游戏目录。查不到就当没有，不多一屏。 */
  async function findNearby() {
    if (!inTauri()) return
    try {
      nearby = (await invoke<string | null>('nearby_game_directory')) ?? ''
    } catch {
      nearby = ''
    }
  }

  /**
   * 把选中的版本真的添加进来，然后往下走。
   *
   * 这一步的主按钮就是「添加」本身。之前它只是「继续」，而添加按钮长在下面那个
   * 组件里——勾好了复选框按下最显眼的那颗，得到的是一个什么都没导入的启动器。
   */
  async function adoptAndContinue() {
    await adopt?.commit()
    go(1)
  }

  function finish(create: boolean) {
    prefs.finishSetup()
    ondone(create)
  }
</script>

<section class="setup" data-tauri-drag-region="deep">
  {#key step}
    <div class="screen" in:fly={enter}>
      {#if step === 'welcome'}
        <!--
          第一次打开还没有实例，也就没有背景可学色彩——所以这一屏用品牌自己
          的两个值：墨松底上的嫩芽（见 docs/fern-brand-system.html 03）。
        -->
        <div class="lockup">
          <Mark size={54} />
          <span class="word">fern</span>
        </div>
        <h1 class="t-display hero">万千世界，<br /><em>一个入口。</em></h1>
        <p class="lede">欢迎使用 Fern。几步设置之后，就可以出发了。</p>
        <div class="actions">
          <Button variant="primary" onclick={() => go(1)}>开始<ArrowRight size={15} /></Button>
        </div>
      {:else if step === 'account'}
        {#if loginView === 'microsoft'}
          <h1 class="title">你是谁？</h1>
          <p class="lede">
            使用微软账户登录，即可联机、使用正版皮肤与成就。密码只在微软的页面中输入，不经过 Fern。
          </p>

          {#if accounts.deviceCode}
            {@const code = accounts.deviceCode}
            <!-- 登录码是这一刻唯一要做的事，所以给它整行和最大的字号。 -->
            <div class="device">
              <span class="device-label">浏览器已经打开，在其中输入这串代码</span>
              <p class="code t-mono selectable">{code.userCode}</p>
              <p class="site t-mono selectable">{code.verificationUri}</p>
              <div class="device-action">
                <Button variant="ghost" onclick={() => openExternal(verificationTarget(code))}>
                  <ExternalLink size={14} strokeWidth={1.8} />重新打开页面
                </Button>
                <!-- 轮询自己也会发现，这一颗省的是那几秒的干等。 -->
                <Button variant="primary" onclick={() => void accounts.checkMicrosoft()}>
                  我已完成登录
                </Button>
              </div>
            </div>
          {/if}

          {#if accounts.error}<p class="alert">{accounts.error}</p>{/if}

          <div class="actions">
            <div class="back">
              <Button variant="link" tone="quiet" onclick={() => go(-1)}>
                <ArrowLeft size={14} />上一步
              </Button>
            </div>
            <Button
              variant={accounts.deviceCode ? 'ghost' : 'primary'}
              disabled={accounts.busy}
              onclick={() => void loginMicrosoft()}>
              {accounts.busy ? '等待验证' : '登录微软账户'}<ArrowRight size={15} />
            </Button>
            <Button variant="link" tone="quiet" onclick={() => go(1)}>稍后登录</Button>
          </div>

          <!-- 另外两条路各自是一屏，不是这一屏里的两个单选项。 -->
          <div class="alternates">
            <Button variant="link" onclick={() => pickLogin('authlib')}>使用外置登录</Button>
            {#if offlineAllowed}
              <Button variant="link" onclick={() => pickLogin('offline')}>使用离线模式</Button>
            {/if}
          </div>
        {:else if loginView === 'authlib'}
          <div class="step-back">
            <Button variant="link" tone="quiet" onclick={() => pickLogin('microsoft')}>
              <ArrowLeft size={14} />返回正版登录
            </Button>
          </div>
          <h1 class="title">外置登录。</h1>
          <p class="lede">
            在 LittleSkin 等 Yggdrasil 兼容皮肤站上登录。密码仅用于换取令牌，不会保存。
          </p>

          <form
            class="fields"
            onsubmit={(event) => {
              event.preventDefault()
              void loginYggdrasil()
            }}
          >
            <Input
              label="皮肤站地址"
              hint="Yggdrasil API 根地址，可在皮肤站的「在启动器中使用」页面获取。"
              bind:value={apiRoot}
              spellcheck="false"
            />
            <Input label="邮箱" bind:value={username} spellcheck="false" autocomplete="username" />
            <Input
              label="密码"
              type="password"
              bind:value={password}
              autocomplete="current-password"
            />

            {#if accounts.error}<p class="alert">{accounts.error}</p>{/if}

            <div class="actions">
              <div class="back">
                <Button variant="link" tone="quiet" onclick={() => go(-1)}>
                  <ArrowLeft size={14} />上一步
                </Button>
              </div>
              <Button variant="primary" type="submit" disabled={accounts.busy}>
                {accounts.busy ? '登录中' : '登录'}<ArrowRight size={15} />
              </Button>
            </div>
          </form>
        {:else}
          <div class="step-back">
            <Button variant="link" tone="quiet" onclick={() => pickLogin('microsoft')}>
              <ArrowLeft size={14} />返回正版登录
            </Button>
          </div>
          <h1 class="title">离线模式。</h1>
          <p class="lede">
            离线账户只能游玩本地世界与离线服务器，无法进入正版服务器，也无法使用正版皮肤。
          </p>

          <div class="inline">
            <Input
              label="玩家名称"
              hint="3–16 位字母、数字或下划线。UUID 由名称推导，修改名称即更换身份。"
              error={nameError}
              bind:value={playerName}
              maxlength={16}
              spellcheck="false"
              autocomplete="nickname"
              placeholder="Steve"
              oninput={() => (nameError = '')}
              onkeydown={(event) => event.key === 'Enter' && void addOffline()}
            />
          </div>

          <div class="alternates">
            <Button variant="link" onclick={() => openExternal(BUY_URL)}>
              购买正版 Minecraft<ExternalLink size={13} strokeWidth={1.8} />
            </Button>
          </div>

          <div class="actions">
            <div class="back">
              <Button variant="link" tone="quiet" onclick={() => go(-1)}>
                <ArrowLeft size={14} />上一步
              </Button>
            </div>
            <Button variant="primary" onclick={() => void addOffline()}>
              继续<ArrowRight size={15} />
            </Button>
          </div>
        {/if}
      {:else if step === 'source'}
        <h1 class="title">让下载快一点。</h1>
        <p class="lede">
          选择文件下载源。根据系统区域，建议使用 <strong>{sourceName[recommended]}</strong>。
        </p>

        <div class="options">
        <RadioGroup
          aria-label="下载源"
          value={source}
          onchange={(next) => (source = next)}
          options={[
            {
              value: 'bmclapi' as const,
              label: 'BMCLAPI',
              note: '国内镜像，中国大陆网络下更快',
              badge: recommended === 'bmclapi' ? '推荐' : undefined,
              badgeTone: 'accent' as const,
            },
            {
              value: 'official' as const,
              label: '官方源',
              note: 'Mojang 官方服务器，海外网络下更快',
              badge: recommended === 'official' ? '推荐' : undefined,
              badgeTone: 'accent' as const,
            },
          ]}
        />
        </div>

        <p class="foot-note">当前源失败时将自动切换到另一个源，选择错误不会导致无法下载。</p>

        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button variant="primary" loading={javaChecking} onclick={submitSource}>
            继续<ArrowRight size={15} />
          </Button>
        </div>
      {:else if step === 'java'}
        <h1 class="title">还差一样东西。</h1>
        <p class="lede">未找到可用的 Java。Minecraft 1.17 之前需要 Java 8，之后需要 Java 17 或 21。</p>
        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button variant="ghost" loading={javaChecking} onclick={() => void checkJava()}>
            重新检测
          </Button>
          <Button variant="primary" onclick={() => go(1)}>稍后处理<ArrowRight size={15} /></Button>
        </div>
      {:else if step === 'existing'}
        <h1 class="title">发现了一个游戏目录。</h1>
        <p class="lede">
          Fern 旁边有一个 .minecraft 目录。可以直接把其中的版本添加为实例，游戏文件保留在原位置，不会移动或复制。
        </p>
        <div class="found">
          <AdoptDirectory
            bind:this={adopt}
            initial={nearby}
            standalone={false}
            onstatus={(status) => (adoptStatus = status)}
          />
        </div>
        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button
            variant="primary"
            disabled={adoptStatus.busy}
            onclick={() => void adoptAndContinue()}>
            {adoptStatus.chosen > 0 ? `添加 ${adoptStatus.chosen} 个版本` : '继续'}
            <ArrowRight size={15} />
          </Button>
          {#if adoptStatus.chosen > 0}
            <Button variant="link" tone="quiet" disabled={adoptStatus.busy} onclick={() => go(1)}>
              暂不添加
            </Button>
          {/if}
        </div>
      {:else}
        <!--
          最后一屏是道别，不是第七个任务。这里已经没有必须做的事了——尤其是
          刚从上一屏把一整个目录接进来的人，他的实例已经在那儿了，再被推去
          「创建你的第一个实例」只会以为刚才那一步没生效。
        -->
        <h1 class="title">欢迎使用 Fern。</h1>
        <p class="lede">
          {instances.list.length > 0
            ? `设置已完成，${instances.list.length} 个实例已经就绪。`
            : '设置已完成。创建一个实例，就可以开始游戏。'}
        </p>
        {#if javaDetail}<p class="detected t-mono">{javaDetail}</p>{/if}
        <div class="actions">
          <Button variant="primary" onclick={() => finish(false)}>
            进入 Fern<ArrowRight size={15} />
          </Button>
          <Button variant="link" onclick={() => finish(true)}>
            <Plus size={14} />创建实例
          </Button>
        </div>
      {/if}
    </div>
  {/key}

  <div class="progress" aria-hidden="true">
    {#each steps as id, position (id)}
      <span class:on={position <= index}></span>
    {/each}
  </div>
</section>

<style>
  .setup {
    position: relative;
    z-index: 1;
    display: grid;
    align-content: center;
    flex: 1;
    min-height: 0;
    padding: 0 var(--pad-x);
  }

  .screen {
    grid-area: 1 / 1;
    display: grid;
    justify-items: start;
    width: min(560px, 100%);
  }

  /* 纵排字标：标志与字之间一格半（见 docs/fern-brand-system.html 04）。 */
  .lockup {
    display: grid;
    justify-items: start;
    gap: var(--s3);
    margin-bottom: var(--s5);
    color: var(--sprout);
  }

  .word {
    color: var(--paper);
    font-size: var(--t-h2);
    font-weight: 650;
    letter-spacing: -0.015em;
  }

  /* 这一屏比别的屏内容多，给它一个自己的滚动区，不撑破居中的版心。 */
  .found {
    width: 100%;
    max-height: 46vh;
    margin: var(--s4) 0;
    overflow-y: auto;
  }

  .hero {
    font-size: clamp(46px, 7vw, 92px);
    line-height: 0.96;
    letter-spacing: -0.055em;
  }

  .hero em {
    color: var(--accent);
    font-style: normal;
  }

  /* 内页标题比欢迎屏小一档：欢迎屏是招牌，其余是提问。 */
  .title {
    margin: 0;
    font-size: clamp(30px, 3.6vw, 44px);
    font-weight: 620;
    line-height: 1.05;
    letter-spacing: -0.04em;
  }

  .lede {
    max-width: 44ch;
    margin: var(--s4) 0 0;
    color: var(--ink-2);
    font-size: var(--t-lead);
    line-height: 1.7;
  }

  .lede strong {
    color: var(--ink);
    font-weight: 600;
  }

  /* 布局归调用方：这一组在向导里离上面那段话多远，是这一屏的事。 */
  .options {
    width: 100%;
    margin-top: var(--s6);
  }

  /* 布局归调用方：这个字段在向导里多宽、离上面多远，是这一屏的事。 */
  .inline {
    width: min(320px, 100%);
    margin-top: var(--s4);
  }

  /* 外置登录那一屏的三个字段。 */
  .fields {
    display: grid;
    gap: var(--s4);
    width: min(400px, 100%);
    margin-top: var(--s5);
  }

  /* 从正版登录进来的两屏，返回的出口在标题上方。 */
  .step-back {
    justify-self: start;
    margin-bottom: var(--s3);
  }

  /* 正版登录之外的两个入口。它们是入口，不是这一屏的动作。 */
  .alternates {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s4);
    margin-top: var(--s5);
  }

  /* 等待用户去浏览器输码的那一刻，这块就是这一屏的全部内容。 */
  .device {
    display: grid;
    justify-items: start;
    gap: var(--s2);
    margin-top: var(--s5);
  }

  .device-label {
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  /* 八位码是这一刻唯一要读的东西，字号给到位。 */
  .code {
    margin: 0;
    color: var(--ink);
    font-size: var(--t-h1);
    font-weight: 600;
    letter-spacing: 0.14em;
  }

  .site {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .device-action {
    display: flex;
    align-items: center;
    gap: var(--s3);
    margin-top: var(--s2);
  }

  .alert {
    margin-top: var(--s4);
  }

  .foot-note {
    margin: var(--s4) 0 0;
    color: var(--ink-4);
    font-size: var(--t-small);
  }

  .detected {
    margin: var(--s3) 0 0;
    color: var(--ink-4);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--s4);
    margin-top: var(--s6);
  }

  .back {
    order: -1;
  }

  /* 进度：四五道短线，不是百分比。 */
  .progress {
    position: absolute;
    left: var(--pad-x);
    bottom: var(--pad-b);
    display: flex;
    gap: 6px;
  }

  .progress span {
    width: 18px;
    height: 2px;
    border-radius: 2px;
    background: var(--tint-2);
    transition: background var(--t-base) var(--ease);
  }

  .progress span.on {
    background: var(--accent);
  }
</style>
