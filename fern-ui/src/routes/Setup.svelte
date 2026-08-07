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
  import { ArrowLeft, ArrowRight, Check, Plus } from 'lucide-svelte'
  import { theme } from '../lib/theme.svelte'
  import { prefs, suggestedSource, type AccountKind, type DownloadSource } from '../lib/prefs.svelte'
  import { inTauri } from '../lib/instances.svelte'

  interface Props {
    /** 走完向导。create 为真时直接进到新建实例。 */
    ondone: (create: boolean) => void
  }

  let { ondone }: Props = $props()

  type StepId = 'welcome' | 'account' | 'source' | 'java' | 'done'

  let index = $state(0)
  let direction = $state(1)

  let accountKind = $state<AccountKind>(prefs.accountKind)
  let playerName = $state(prefs.playerName)
  let nameError = $state('')

  let source = $state<DownloadSource>(suggestedSource())
  const recommended = suggestedSource()
  const sourceName = { official: '官方源', bmclapi: 'BMCLAPI' } as const

  /** unknown：还没查过。missing：查过了，系统里没有可用的 Java。 */
  let java = $state<'unknown' | 'ok' | 'missing'>('unknown')
  let javaChecking = $state(false)
  let javaDetail = $state('')

  const steps = $derived<StepId[]>([
    'welcome',
    'account',
    'source',
    ...(java === 'missing' ? (['java'] as StepId[]) : []),
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

  function submitAccount() {
    if (accountKind !== 'offline') return
    const value = playerName.trim()
    if (!/^[A-Za-z0-9_]{3,16}$/.test(value)) {
      nameError = '3–16 位字母、数字或下划线'
      return
    }
    prefs.setAccount('offline', value)
    go(1)
  }

  function submitSource() {
    prefs.setDownloadSource(source)
    void checkJava().then(() => go(1))
  }

  function finish(create: boolean) {
    prefs.finishSetup()
    ondone(create)
  }

  const ACCOUNTS: { kind: AccountKind; title: string; note: string; ready: boolean }[] = [
    { kind: 'microsoft', title: '微软账户', note: '正版登录，联机、皮肤与成就', ready: false },
    { kind: 'authlib', title: '外置登录', note: 'LittleSkin 等 Yggdrasil 皮肤站', ready: true },
    { kind: 'offline', title: '离线模式', note: '只在本地世界和离线服务器游玩', ready: true },
  ]
</script>

<section class="setup" data-tauri-drag-region="deep">
  {#key step}
    <div class="screen" in:fly={enter}>
      {#if step === 'welcome'}
        <div class="mark" aria-hidden="true"><span></span></div>
        <h1 class="t-display hero">万千世界，<br /><em>一个入口。</em></h1>
        <p class="lede">欢迎使用 Fern。几步设置之后，就可以出发了。</p>
        <div class="actions">
          <button class="btn btn--primary" onclick={() => go(1)}>开始<ArrowRight size={15} /></button>
        </div>
      {:else if step === 'account'}
        <h1 class="title">你是谁？</h1>
        <p class="lede">选择一种登录方式。之后随时可以更换或添加。</p>

        <div class="options">
          {#each ACCOUNTS as item (item.kind)}
            <button
              class="option"
              class:on={accountKind === item.kind}
              disabled={!item.ready}
              onclick={() => {
                accountKind = item.kind
                nameError = ''
              }}
            >
              <span class="option-text">
                <strong>{item.title}</strong>
                <small>{item.note}</small>
              </span>
              {#if !item.ready}
                <span class="tag">尚未接入</span>
              {:else if accountKind === item.kind}
                <Check size={16} strokeWidth={2.4} />
              {/if}
            </button>
          {/each}
        </div>

        <!-- 向导只问「用哪种」，登录本身留到设置页：一屏一件事，而登录要填
             三个框、要联网、还可能失败，那不该是第一印象的一部分。 -->
        {#if accountKind === 'authlib'}
          <p class="note">选好了。继续走完，之后在设置里登录皮肤站账号。</p>
        {/if}

        {#if accountKind === 'offline'}
          <div class="field inline">
            <label for="setup-name">玩家名称</label>
            <input
              id="setup-name"
              class="input"
              bind:value={playerName}
              maxlength="16"
              spellcheck="false"
              autocomplete="nickname"
              placeholder="Steve"
              oninput={() => (nameError = '')}
              onkeydown={(event) => event.key === 'Enter' && submitAccount()}
            />
            {#if nameError}<p class="err">{nameError}</p>{/if}
          </div>
        {/if}

        <div class="actions">
          <button class="btn btn--link back" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</button>
          <button class="btn btn--primary" onclick={submitAccount}>继续<ArrowRight size={15} /></button>
        </div>
      {:else if step === 'source'}
        <h1 class="title">让下载快一点。</h1>
        <p class="lede">
          选择文件下载源。根据系统区域，Fern 建议使用 <strong>{sourceName[recommended]}</strong>。
        </p>

        <div class="options">
          <button class="option" class:on={source === 'bmclapi'} onclick={() => (source = 'bmclapi')}>
            <span class="option-text">
              <strong>BMCLAPI<span class="hint-tag" class:show={recommended === 'bmclapi'}>推荐</span></strong>
              <small>国内镜像，中国大陆网络下明显更快</small>
            </span>
            {#if source === 'bmclapi'}<Check size={16} strokeWidth={2.4} />{/if}
          </button>
          <button class="option" class:on={source === 'official'} onclick={() => (source = 'official')}>
            <span class="option-text">
              <strong>官方源<span class="hint-tag" class:show={recommended === 'official'}>推荐</span></strong>
              <small>Mojang 官方服务器，海外网络下更快</small>
            </span>
            {#if source === 'official'}<Check size={16} strokeWidth={2.4} />{/if}
          </button>
        </div>

        <p class="foot-note">另一个源会在这个源失败时自动接手，不会因为选错而下不动。</p>

        <div class="actions">
          <button class="btn btn--link back" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</button>
          <button class="btn btn--primary" disabled={javaChecking} onclick={submitSource}>
            {javaChecking ? '检查环境' : '好'}<ArrowRight size={15} />
          </button>
        </div>
      {:else if step === 'java'}
        <h1 class="title">还差一样东西。</h1>
        <p class="lede">没有找到可用的 Java。Minecraft 1.17 之前需要 Java 8，之后需要 Java 17 或 21。</p>
        <div class="actions">
          <button class="btn btn--link back" onclick={() => go(-1)}><ArrowLeft size={14} />上一步</button>
          <button class="btn btn--ghost" disabled={javaChecking} onclick={() => void checkJava()}>
            {javaChecking ? '检测中' : '我已安装，重新检测'}
          </button>
          <button class="btn btn--primary" onclick={() => go(1)}>稍后处理<ArrowRight size={15} /></button>
        </div>
      {:else}
        <h1 class="title">准备好了。</h1>
        <p class="lede">去创建你的第一个实例吧。</p>
        {#if javaDetail}<p class="detected t-mono">{javaDetail}</p>{/if}
        <div class="actions">
          <button class="btn btn--primary" onclick={() => finish(true)}>
            <Plus size={15} />创建实例
          </button>
          <button class="btn btn--link" onclick={() => finish(false)}>先看看</button>
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

  .mark {
    position: relative;
    width: 46px;
    height: 46px;
    margin-bottom: var(--s5);
    border-radius: var(--r2);
    background: var(--accent);
    box-shadow: 0 14px 44px -10px var(--accent-soft);
    transform: rotate(-8deg);
  }

  .mark span {
    position: absolute;
    inset: 13px;
    border-radius: 4px;
    background: var(--c0);
    opacity: 0.55;
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

  .options {
    display: grid;
    gap: var(--s2);
    width: 100%;
    margin-top: var(--s6);
  }

  /* 三种登录方式平等地放着：离线模式不藏、不加警告色，只是其中一个选项。 */
  .option {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3) var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px transparent;
    text-align: left;
    transition:
      background var(--t-fast) var(--ease),
      box-shadow var(--t-fast) var(--ease);
  }

  .option:hover:not(:disabled) {
    background: var(--tint-2);
  }

  .option.on {
    background: var(--tint-2);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  .option:disabled {
    opacity: 0.42;
    cursor: default;
  }

  .option :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .option-text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .option-text strong {
    display: flex;
    align-items: center;
    gap: var(--s2);
    font-size: var(--t-body);
    font-weight: 550;
  }

  .option-text small {
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  .hint-tag {
    display: none;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 10px;
    font-weight: 600;
  }

  .hint-tag.show {
    display: inline-block;
  }

  .tag {
    flex: none;
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .field.inline {
    display: grid;
    gap: var(--s2);
    width: min(320px, 100%);
    margin-top: var(--s4);
  }

  .field label {
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  /* 选了外置登录之后的一句交代，和 .lede 同级但更轻。 */
  .note {
    margin: var(--s4) 0 0;
    color: var(--ink-3);
    font-size: var(--t-body);
  }

  .err {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
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
    gap: 6px;
    color: var(--ink-3);
    order: -1;
  }

  .back:hover {
    color: var(--ink);
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
