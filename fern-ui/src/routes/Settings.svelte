<script lang="ts">
  /**
   * 设置。
   *
   * 只放真的接着东西的开关。上一版里「启动后保持在后台」「并发任务 64 个
   * 文件」这类项要么点了没反应，要么根本是写死的说明文字——设置页里的
   * 假开关比没有这一页更伤，因为它会让人以为自己已经配置过了。
   *
   * 外观这一节是文档里「个性化出口」的第一批：改动写进主题状态，立刻
   * 全局生效，序列化出来就是一份可以贴给别人的主题码。
   */
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { ArrowLeft, Check, Copy, FolderOpen } from 'lucide-svelte'
  import Choice from '../components/Choice.svelte'
  import { ACCENT_PRESETS, theme } from '../lib/theme.svelte'
  import { prefs, suggestedSource } from '../lib/prefs.svelte'
  import { inTauri } from '../lib/instances.svelte'

  interface Props {
    onback: () => void
  }

  let { onback }: Props = $props()

  const sections = [
    { id: 'appearance', label: '外观' },
    { id: 'account', label: '账户' },
    { id: 'download', label: '下载' },
    { id: 'data', label: '数据' },
  ] as const
  type SectionId = (typeof sections)[number]['id']

  let section = $state<SectionId>('appearance')
  let paths = $state({ root: '', logs: '' })
  let pathError = $state('')
  let runtimes = $state<
    { path: string; home: string; major: number; version: string; vendor: string; managed: boolean }[]
  >([])
  let runtimeError = $state('')

  interface AccountView {
    kind: string
    apiRoot: string | null
    playerName: string
    uuid: string
  }
  let session = $state<AccountView | null>(null)
  let msa = $state<AccountView | null>(null)
  /** 正版登录要用户去浏览器输的那八位码。 */
  let deviceCode = $state<{ userCode: string; verificationUri: string } | null>(null)
  let msaBusy = $state(false)
  let msaError = $state('')
  let apiRoot = $state('https://littleskin.cn/api/yggdrasil')
  let username = $state('')
  let password = $state('')
  let loggingIn = $state(false)
  let loginError = $state('')
  let themeCode = $state('')
  let copied = $state(false)
  let importError = $state('')

  const sourceName = { official: '官方源', bmclapi: 'BMCLAPI' } as const

  onMount(() => {
    themeCode = theme.export()
    if (!inTauri()) return
    void invoke<{ root: string; logs: string }>('data_paths')
      .then((value) => (paths = value))
      .catch((error) => (pathError = String(error)))
    void loadRuntimes()
    void invoke<AccountView | null>('microsoft_session')
      .then((value) => (msa = value))
      .catch(() => {})
    void invoke<AccountView | null>('yggdrasil_session')
      .then((value) => (session = value))
      .catch(() => {
        // 钥匙串用不了的时候这里会失败，但那句话该在用户真的去登录时再说，
        // 打开设置页就先弹一条报错是噪音。
      })
  })

  async function login() {
    if (!apiRoot.trim() || !username.trim() || !password) {
      loginError = '皮肤站地址、邮箱和密码都要填'
      return
    }
    loggingIn = true
    loginError = ''
    try {
      session = await invoke<AccountView>('yggdrasil_login', {
        apiRoot: apiRoot.trim(),
        username: username.trim(),
        password,
      })
      // 拿到令牌就把密码从内存里去掉，它已经没有用处了。
      password = ''
    } catch (error) {
      loginError = String(error)
    } finally {
      loggingIn = false
    }
  }

  async function microsoftLogin() {
    msaBusy = true
    msaError = ''
    deviceCode = null
    // 八位码由后端在拿到之后推过来——它要显示的那一刻，登录还在等用户。
    const stop = await listen<{ userCode: string; verificationUri: string }>(
      'microsoft-device-code',
      ({ payload }) => (deviceCode = payload),
    )
    try {
      msa = await invoke<AccountView>('microsoft_login')
      deviceCode = null
    } catch (error) {
      msaError = String(error)
    } finally {
      stop()
      msaBusy = false
    }
  }

  async function microsoftLogout() {
    try {
      await invoke('microsoft_logout')
      msa = null
    } catch (error) {
      msaError = String(error)
    }
  }

  async function logout() {
    try {
      await invoke('yggdrasil_logout')
      session = null
    } catch (error) {
      loginError = String(error)
    }
  }

  async function loadRuntimes() {
    if (!inTauri()) return
    try {
      runtimes = await invoke('list_java_runtimes')
      runtimeError = ''
    } catch (error) {
      runtimeError = String(error)
    }
  }

  async function removeRuntime(home: string) {
    try {
      await invoke('remove_java_runtime', { home })
      await loadRuntimes()
    } catch (error) {
      runtimeError = String(error)
    }
  }

  function change<T>(apply: (value: T) => void) {
    return (value: T) => {
      apply(value)
      themeCode = theme.export()
      importError = ''
    }
  }

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(themeCode)
      copied = true
      setTimeout(() => (copied = false), 1400)
    } catch {
      // 剪贴板被拒绝时字段本身是可选中的，手动复制照样能完成这件事。
    }
  }

  function applyCode() {
    importError = theme.import(themeCode) ? '' : '这段主题码读不出来'
    if (!importError) themeCode = theme.export()
  }

  async function openLogs() {
    pathError = ''
    try {
      await invoke('open_logs_directory')
    } catch (error) {
      pathError = String(error)
    }
  }
</script>

<section class="settings scroll">
  <div class="inner">
    <header data-tauri-drag-region="deep">
      <button class="btn btn--link back" onclick={onback}><ArrowLeft size={14} />返回</button>
      <h1 class="t-h1">设置</h1>
    </header>

    <div class="layout">
      <nav aria-label="设置分类">
        {#each sections as item (item.id)}
          <button class:on={section === item.id} onclick={() => (section = item.id)}>
            {item.label}
          </button>
        {/each}
      </nav>

      <div class="content">
        {#if section === 'appearance'}
          <div class="row">
            <span class="label">强调色</span>
            <Choice
              label="强调色来源"
              value={theme.accentMode}
              onchange={change((value) => theme.set('accentMode', value))}
              options={[
                { value: 'biome', label: '跟随背景' },
                { value: 'locked', label: '锁定' },
              ]}
            />
          </div>

          {#if theme.accentMode === 'locked'}
            <div class="row swatch-row">
              <span class="label">颜色</span>
              <div class="swatches">
                {#each ACCENT_PRESETS as preset (preset.key)}
                  <button
                    class="swatch"
                    class:on={theme.accent.toLowerCase() === preset.value}
                    style:background={preset.value}
                    title={preset.name}
                    aria-label={preset.name}
                    onclick={() => change((v: string) => theme.set('accent', v))(preset.value)}
                  >
                    {#if theme.accent.toLowerCase() === preset.value}
                      <Check size={13} strokeWidth={3} />
                    {/if}
                  </button>
                {/each}
                <label class="swatch custom" title="自定义颜色" style:background={theme.accent}>
                  <input
                    type="color"
                    value={theme.accent}
                    oninput={(event) =>
                      change((v: string) => theme.set('accent', v))(event.currentTarget.value)}
                  />
                </label>
              </div>
            </div>
          {/if}

          <div class="row">
            <span class="label">界面密度</span>
            <Choice
              label="界面密度"
              value={theme.density}
              onchange={change((value) => theme.set('density', value))}
              options={[
                { value: 'compact', label: '紧凑' },
                { value: 'default', label: '标准' },
                { value: 'roomy', label: '宽松' },
              ]}
            />
          </div>

          <div class="row">
            <span class="label">圆角</span>
            <Choice
              label="圆角"
              value={theme.radius}
              onchange={change((value) => theme.set('radius', value))}
              options={[
                { value: 'sharp', label: '直角' },
                { value: 'default', label: '标准' },
                { value: 'round', label: '圆润' },
              ]}
            />
          </div>

          <div class="row">
            <span class="label">
              动效
              <small>关闭会同时停掉背景粒子和指针视差。窗口失焦时始终暂停。</small>
            </span>
            <Choice
              label="动效"
              value={theme.motion}
              onchange={change((value) => theme.set('motion', value))}
              options={[
                { value: 'full', label: '完整' },
                { value: 'reduced', label: '减弱' },
                { value: 'off', label: '关闭' },
              ]}
            />
          </div>

          <div class="row stack">
            <span class="label">
              主题码
              <small>上面这些选择的全部内容。贴给别人，对方粘贴后按应用即可复现。</small>
            </span>
            <div class="code-row">
              <input class="input selectable t-mono" bind:value={themeCode} spellcheck="false" />
              <button class="btn btn--icon" aria-label="复制" title="复制" onclick={() => void copyCode()}>
                {#if copied}<Check size={15} />{:else}<Copy size={14} />{/if}
              </button>
              <button class="btn btn--ghost" onclick={applyCode}>应用</button>
            </div>
            {#if importError}<p class="err">{importError}</p>{/if}
          </div>

          <div class="row">
            <span class="label">恢复默认外观</span>
            <button
              class="btn btn--ghost"
              onclick={() => {
                theme.reset()
                themeCode = theme.export()
              }}
            >
              恢复
            </button>
          </div>
        {:else if section === 'account'}
          <div class="row stack">
            <span class="label">
              登录方式
            </span>
            <Choice
              label="登录方式"
              value={prefs.accountKind}
              onchange={(next) => prefs.setAccount(next, prefs.playerName)}
              options={[
                { value: 'offline', label: '离线模式' },
                { value: 'microsoft', label: '微软账户' },
                { value: 'authlib', label: '外置登录' },
              ]}
            />
          </div>

          {#if prefs.accountKind === 'microsoft'}
            {#if msa}
              <div class="row">
                <span class="label">已登录<small class="t-mono">{msa.uuid}</small></span>
                <span class="value">{msa.playerName}</span>
              </div>
              <div class="row">
                <span class="label">退出登录<small>令牌会从系统钥匙串里删掉。</small></span>
                <button class="btn btn--ghost" onclick={() => void microsoftLogout()}>退出</button>
              </div>
            {:else if deviceCode}
              <!-- 登录码是这一屏此刻唯一要做的事，所以给它整行和最大的字号。 -->
              <div class="row stack">
                <span class="label">
                  在浏览器里输入这个码
                  <small>密码只在微软的页面上输入，不经过 Fern。</small>
                </span>
                <p class="code t-mono selectable">{deviceCode.userCode}</p>
                <p class="t-mono path selectable">{deviceCode.verificationUri}</p>
              </div>
            {:else}
              <div class="row">
                <span class="label">
                  微软账户
                  <small>会打开一个八位码，去浏览器里输入即可，不用在这里填密码。</small>
                </span>
                <button
                  class="btn btn--primary"
                  disabled={msaBusy}
                  onclick={() => void microsoftLogin()}
                >
                  {msaBusy ? '等待中' : '登录'}
                </button>
              </div>
            {/if}
            {#if msaError}<div class="alert">{msaError}</div>{/if}
          {/if}

          {#if prefs.accountKind === 'authlib'}
            {#if session}
              <div class="row">
                <span class="label">
                  已登录
                  <small class="t-mono">{session.apiRoot}</small>
                </span>
                <span class="value">{session.playerName}</span>
              </div>
              <div class="row">
                <span class="label">退出登录<small>令牌会从系统钥匙串里删掉。</small></span>
                <button class="btn btn--ghost" onclick={() => void logout()}>退出</button>
              </div>
            {:else}
              <div class="row stack">
                <span class="label">
                  皮肤站地址
                  <small>Yggdrasil API 根地址，皮肤站的「在启动器中使用」页面会给出。</small>
                </span>
                <input
                  class="input"
                  bind:value={apiRoot}
                  spellcheck="false"
                  placeholder="https://littleskin.cn/api/yggdrasil"
                />
              </div>
              <div class="row stack">
                <span class="label">邮箱</span>
                <input class="input" bind:value={username} spellcheck="false" />
              </div>
              <div class="row stack">
                <span class="label">
                  密码
                  <small>只用来换取令牌，不会保存。令牌存进系统钥匙串。</small>
                </span>
                <input
                  class="input"
                  type="password"
                  bind:value={password}
                  onkeydown={(event) => event.key === 'Enter' && void login()}
                />
              </div>
              <div class="row">
                <span class="label"></span>
                <button
                  class="btn btn--primary"
                  disabled={loggingIn}
                  onclick={() => void login()}
                >
                  {loggingIn ? '登录中' : '登录'}
                </button>
              </div>
            {/if}
            {#if loginError}<div class="alert">{loginError}</div>{/if}
          {:else if prefs.accountKind === 'offline'}
            <div class="row stack">
              <span class="label">
                玩家名称
                <small>用于离线启动。这个名字会生成稳定的离线 UUID。</small>
              </span>
              <input
                class="input name"
                value={prefs.playerName}
                maxlength="16"
                spellcheck="false"
                placeholder="Steve"
                oninput={(event) => prefs.setPlayerName(event.currentTarget.value)}
              />
            </div>
          {/if}
        {:else if section === 'download'}
          <div class="row">
            <span class="label">
              下载源
              <small
                >按你的系统区域，建议用 {sourceName[suggestedSource()]}。任一个源失败时另一个自动接手。</small
              >
            </span>
            <Choice
              label="下载源"
              value={prefs.downloadSource}
              onchange={(value) => prefs.setDownloadSource(value)}
              options={[
                { value: 'official', label: '官方源' },
                { value: 'bmclapi', label: 'BMCLAPI' },
              ]}
            />
          </div>
        {:else}
          <div class="row stack">
            <span class="label">数据目录</span>
            <p class="path t-mono selectable">{paths.root || '—'}</p>
          </div>
          <div class="row stack">
            <span class="label">日志目录</span>
            <p class="path t-mono selectable">{paths.logs || '—'}</p>
            <button class="btn btn--ghost open" onclick={() => void openLogs()}>
              <FolderOpen size={14} strokeWidth={1.8} />打开日志目录
            </button>
          </div>
          {#if pathError}<div class="alert">{pathError}</div>{/if}
          <!-- Java 平时是隐形的；能看见的唯一理由是它占了地方，要能删。 -->
          <div class="row stack">
            <span class="label">Java 运行时</span>
            {#if runtimes.length === 0}
              <p class="t-quiet">还没有找到任何 Java。第一次启动游戏时会自动下载。</p>
            {:else}
              <ul class="runtimes">
                {#each runtimes as item (item.path)}
                  <li>
                    <span class="rt-name">
                      Java {item.major}
                      <small class="t-quiet">
                        {item.vendor || '未知发行版'} · {item.managed ? 'Fern 下载' : '系统自带'}
                      </small>
                    </span>
                    {#if item.managed}
                      <button class="btn btn--link" onclick={() => void removeRuntime(item.home)}>
                        删除
                      </button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if runtimeError}<div class="alert">{runtimeError}</div>{/if}
          </div>
          <div class="row">
            <span class="label">版本</span>
            <span class="t-mono value">Fern 0.1.0</span>
          </div>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  /* 八位码是这一刻唯一要读的东西，字号给到位。 */
  .code {
    margin: var(--s2) 0 0;
    color: var(--ink);
    font-size: var(--t-h1);
    font-weight: 600;
    letter-spacing: 0.14em;
  }

  .runtimes {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .runtimes li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
  }

  .rt-name {
    display: grid;
    gap: 1px;
    color: var(--ink-2);
    font-size: var(--t-body);
  }

  .settings {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    padding: 0 var(--pad-x) var(--s8);
  }

  .inner {
    max-width: 860px;
    margin: 0 auto;
  }

  header {
    display: grid;
    gap: var(--s3);
    /* 顶栏那一条留给窗口按钮和拖拽，返回键不要挤进去。 */
    padding: calc(var(--top) + var(--s4)) calc(var(--frame-controls)) var(--s6) 0;
  }

  .back {
    justify-self: start;
    gap: 6px;
    color: var(--ink-3);
  }

  .back:hover {
    color: var(--ink);
  }

  .layout {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: clamp(var(--s5), 5vw, var(--s8));
  }

  nav {
    display: flex;
    flex-direction: column;
    align-items: start;
    gap: var(--s1);
    position: sticky;
    top: 0;
    align-self: start;
  }

  nav button {
    padding: var(--s1) 0;
    color: var(--ink-4);
    font-size: var(--t-body);
    transition: color var(--t-fast) var(--ease);
  }

  nav button:hover {
    color: var(--ink-2);
  }

  nav button.on {
    color: var(--ink);
    font-weight: 550;
  }

  .content {
    min-width: 0;
  }

  /* 每一行是「一个名字，一个控件」。说明文字只在没有它就会用错的地方出现。 */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s5);
    padding: var(--s4) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .row.stack {
    display: grid;
    justify-items: stretch;
    gap: var(--s3);
  }

  .row:last-child {
    box-shadow: none;
  }

  .label {
    display: grid;
    gap: 4px;
    font-size: var(--t-body);
    color: var(--ink);
  }

  .label small {
    max-width: 46ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.55;
  }

  .row :global(.choice) {
    flex: none;
    width: 210px;
  }

  .swatch-row {
    align-items: center;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    justify-content: flex-end;
  }

  .swatch {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: #10171b;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.2);
    transition: transform var(--t-fast) var(--spring);
  }

  .swatch:hover {
    transform: scale(1.12);
  }

  .swatch.on {
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.2),
      0 0 0 2px var(--panel),
      0 0 0 3.5px var(--ink);
  }

  .swatch.custom {
    position: relative;
    overflow: hidden;
    cursor: pointer;
    background-image: conic-gradient(#e88, #ee8, #8e8, #8ee, #88e, #e8e, #e88);
  }

  .swatch.custom input {
    position: absolute;
    inset: -6px;
    opacity: 0;
    cursor: pointer;
  }

  .code-row {
    display: flex;
    gap: var(--s2);
  }

  .code-row .input {
    font-size: var(--t-small);
  }

  .name {
    max-width: 260px;
  }

  .path {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .open {
    justify-self: start;
  }

  .value {
    color: var(--ink-3);
  }

  .err {
    margin: 0;
    color: var(--danger);
    font-size: var(--t-small);
  }

  @media (max-width: 720px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--s5);
    }

    nav {
      position: static;
      flex-direction: row;
      gap: var(--s4);
    }

    .row {
      flex-direction: column;
      align-items: stretch;
      gap: var(--s3);
    }

    .row :global(.choice) {
      width: 100%;
    }

    .swatches {
      justify-content: flex-start;
    }
  }
</style>
