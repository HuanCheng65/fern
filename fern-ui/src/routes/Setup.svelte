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
  import { ArrowLeft, ArrowRight, Plus } from 'lucide-svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import Mark from 'fern-kit/ui/Mark.svelte'
  import { theme } from '../lib/theme.svelte'
  import { prefs, suggestedSource, type DownloadSource } from '../lib/prefs.svelte'
  import { accounts, type AccountKind } from '../lib/accounts.svelte'
  import { inTauri } from '../lib/instances.svelte'
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

  // 只在这一屏里活着：账户类型不再是一个偏好，选它只是决定这一步问什么。
  let accountKind = $state<AccountKind>('offline')
  let playerName = $state('')
  let nameError = $state('')

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

  /**
   * 离线那一支在这里就把账户建出来；另外两支只往下走。
   *
   * 登录要填三个框、要联网、还可能失败，那不该是第一印象的一部分——所以向导
   * 只问「先用哪种」，真正的登录留给设置页那份名单。
   */
  async function submitAccount() {
    if (accountKind !== 'offline') {
      go(1)
      return
    }
    const value = playerName.trim()
    if (!/^[A-Za-z0-9_]{3,16}$/.test(value)) {
      nameError = '3–16 位字母、数字或下划线'
      return
    }
    await accounts.addOffline(value)
    if (accounts.error) {
      nameError = accounts.error
      return
    }
    go(1)
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

  function finish(create: boolean) {
    prefs.finishSetup()
    ondone(create)
  }

  const ACCOUNTS: { kind: AccountKind; title: string; note: string; ready: boolean }[] = [
    { kind: 'microsoft', title: '微软账户', note: '正版登录，支持联机、皮肤与成就', ready: true },
    { kind: 'authlib', title: '外置登录', note: 'LittleSkin 等 Yggdrasil 兼容皮肤站', ready: true },
    { kind: 'offline', title: '离线模式', note: '仅可游玩本地世界与离线服务器', ready: true },
  ]
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
        <h1 class="title">你是谁？</h1>
        <p class="lede">选择一种登录方式。之后随时可以更换或添加。</p>

        <div class="options">
        <RadioGroup
          aria-label="登录方式"
          value={accountKind}
          onchange={(next) => {
            accountKind = next
            nameError = ''
          }}
          options={ACCOUNTS.map((item) => ({
            value: item.kind,
            label: item.title,
            note: item.note,
            disabled: !item.ready,
            badge: item.ready ? undefined : '尚未接入',
          }))}
        />
        </div>

        <!-- 向导只问「用哪种」，登录本身留到设置页：一屏一件事，而登录要填
             三个框、要联网、还可能失败，那不该是第一印象的一部分。 -->
        {#if accountKind === 'authlib'}
          <p class="note">继续完成设置，稍后可在设置中登录皮肤站账号。</p>
        {:else if accountKind === 'microsoft'}
          <p class="note">继续完成设置，稍后可在设置中完成登录。</p>
        {/if}

        {#if accountKind === 'offline'}
          <div class="inline">
            <Input
              label="玩家名称"
              error={nameError}
              bind:value={playerName}
              maxlength={16}
              spellcheck="false"
              autocomplete="nickname"
              placeholder="Steve"
              oninput={() => (nameError = '')}
              onkeydown={(event) => event.key === 'Enter' && void submitAccount()}
            />
          </div>
        {/if}

        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button variant="primary" onclick={() => void submitAccount()}>继续<ArrowRight size={15} /></Button>
        </div>
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
          <Button variant="primary" disabled={javaChecking} onclick={submitSource}>
            {javaChecking ? '检查环境' : '继续'}<ArrowRight size={15} />
          </Button>
        </div>
      {:else if step === 'java'}
        <h1 class="title">还差一样东西。</h1>
        <p class="lede">未找到可用的 Java。Minecraft 1.17 之前需要 Java 8，之后需要 Java 17 或 21。</p>
        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button variant="ghost" disabled={javaChecking} onclick={() => void checkJava()}>
            {javaChecking ? '检测中' : '重新检测'}
          </Button>
          <Button variant="primary" onclick={() => go(1)}>稍后处理<ArrowRight size={15} /></Button>
        </div>
      {:else if step === 'existing'}
        <h1 class="title">发现了一个游戏目录。</h1>
        <p class="lede">
          Fern 旁边有一个 .minecraft 目录。可以直接把其中的版本添加为实例，游戏文件保留在原位置。
        </p>
        <div class="found">
          <AdoptDirectory initial={nearby} />
        </div>
        <div class="actions">
          <div class="back">
            <Button variant="link" tone="quiet" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</Button>
          </div>
          <Button variant="primary" onclick={() => go(1)}>继续<ArrowRight size={15} /></Button>
        </div>
      {:else}
        <h1 class="title">准备好了。</h1>
        <p class="lede">去创建你的第一个实例吧。</p>
        {#if javaDetail}<p class="detected t-mono">{javaDetail}</p>{/if}
        <div class="actions">
          <Button variant="primary" onclick={() => finish(true)}>
            <Plus size={15} />创建实例
          </Button>
          <Button variant="link" onclick={() => finish(false)}>稍后创建</Button>
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

  /* 选了外置登录之后的一句交代，和 .lede 同级但更轻。 */
  .note {
    margin: var(--s4) 0 0;
    color: var(--ink-3);
    font-size: var(--t-body);
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
