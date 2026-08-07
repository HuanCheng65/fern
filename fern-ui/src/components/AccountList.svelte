<script lang="ts">
  /**
   * 设置里的账户那一节。
   *
   * 一份名单加一个「添加」，不是三选一的单选框。之前那一版把「登录方式」做成
   * 一个全局开关，于是同时有一个正版号和一个测试用的离线号是不可能的——想换
   * 一个就得把另一个挤掉。名单让身份变回一件可以有很多个的东西。
   *
   * 添加是一条就地展开的支线，不是弹窗：三种方式各自要问的东西差别很大
   * （离线一个名字、正版一个八位码、外置三个字段），弹窗要么撑成最大那个的
   * 尺寸，要么每次开都跳一下。
   */
  import { Check, Plus, X } from 'lucide-svelte'
  import Cover from './Cover.svelte'
  import { accounts, KIND_LABEL, siteName, type AccountKind } from '../lib/accounts.svelte'

  /** 正在添加哪一种。空串表示没在添加。 */
  let adding = $state<AccountKind | ''>('')
  let offlineName = $state('')
  let apiRoot = $state('https://littleskin.cn/api/yggdrasil')
  let username = $state('')
  let password = $state('')
  /** 正在改名的那一条离线账户。 */
  let renaming = $state('')
  let renameTo = $state('')

  const OFFLINE_NAME = /^[A-Za-z0-9_]{3,16}$/

  function close() {
    adding = ''
    offlineName = ''
    username = ''
    password = ''
  }

  async function submitOffline() {
    if (!OFFLINE_NAME.test(offlineName.trim())) return
    await accounts.addOffline(offlineName.trim())
    close()
  }

  async function submitYggdrasil() {
    if (!apiRoot.trim() || !username.trim() || !password) return
    await accounts.loginYggdrasil(apiRoot.trim(), username.trim(), password)
    // 拿到令牌就把密码从内存里去掉，它已经没有用处了。
    password = ''
    if (!accounts.error) close()
  }

  async function submitMicrosoft() {
    await accounts.loginMicrosoft()
    if (!accounts.error) close()
  }

  function startRename(id: string, current: string) {
    renaming = id
    renameTo = current
  }

  async function submitRename() {
    if (!OFFLINE_NAME.test(renameTo.trim())) return
    await accounts.renameOffline(renaming, renameTo.trim())
    renaming = ''
  }

  const KINDS: { kind: AccountKind; title: string; note: string }[] = [
    { kind: 'microsoft', title: '微软账户', note: '正版登录，支持联机、皮肤与成就' },
    { kind: 'authlib', title: '外置登录', note: 'LittleSkin 等 Yggdrasil 兼容皮肤站' },
    { kind: 'offline', title: '离线模式', note: '仅可游玩本地世界与离线服务器' },
  ]
</script>

<div class="accounts">
  {#if accounts.list.length === 0 && !accounts.loading}
    <p class="t-quiet empty">尚未添加账户。添加后方可启动游戏。</p>
  {/if}

  <ul class="roster">
    {#each accounts.list as account (account.id)}
      <li class="row" class:on={accounts.active?.id === account.id}>
        <!-- 封面那套生成式图形当头像用：每个身份一张恒定的脸，不必去拉皮肤。 -->
        <span class="face"><Cover seed={account.uuid} quality={0.4} /></span>

        {#if renaming === account.id}
          <form class="rename" onsubmit={(event) => { event.preventDefault(); void submitRename() }}>
            <input class="input" bind:value={renameTo} maxlength="16" spellcheck="false" />
            <button class="btn btn--ghost" type="submit">保存</button>
            <button class="btn btn--link" type="button" onclick={() => (renaming = '')}>取消</button>
          </form>
        {:else}
          <button
            class="who"
            aria-pressed={accounts.active?.id === account.id}
            onclick={() => void accounts.use(account.id)}
          >
            <strong>{account.playerName}</strong>
            <small>
              {KIND_LABEL[account.kind]}{account.apiRoot ? ` · ${siteName(account.apiRoot)}` : ''}
            </small>
          </button>

          {#if accounts.active?.id === account.id}
            <span class="badge"><Check size={12} strokeWidth={2.6} />当前</span>
          {/if}
          {#if account.kind === 'offline'}
            <button class="btn btn--link" onclick={() => startRename(account.id, account.playerName)}>
              改名
            </button>
          {/if}
          <button class="btn btn--link danger" onclick={() => void accounts.remove(account.id)}>
            移除
          </button>
        {/if}
      </li>
    {/each}
  </ul>

  {#if adding === ''}
    <button class="btn btn--ghost add" onclick={() => (adding = 'microsoft')}>
      <Plus size={14} strokeWidth={2} />添加账户
    </button>
  {:else}
    <div class="adder">
      <div class="kinds">
        {#each KINDS as option (option.kind)}
          <button
            class="kind"
            class:on={adding === option.kind}
            onclick={() => { adding = option.kind; accounts.error = '' }}
          >
            <strong>{option.title}</strong>
            <small>{option.note}</small>
          </button>
        {/each}
        <button class="btn btn--icon close" aria-label="取消添加" onclick={close}>
          <X size={14} />
        </button>
      </div>

      {#if adding === 'offline'}
        <form class="fields" onsubmit={(event) => { event.preventDefault(); void submitOffline() }}>
          <label class="field">
            <span>玩家名称<small>3–16 位字母、数字或下划线。UUID 由名称推导，修改名称即更换身份。</small></span>
            <input class="input" bind:value={offlineName} maxlength="16" spellcheck="false" placeholder="Steve" />
          </label>
          <button class="btn btn--primary" type="submit" disabled={!OFFLINE_NAME.test(offlineName.trim())}>
            添加
          </button>
        </form>
      {:else if adding === 'microsoft'}
        {#if accounts.deviceCode}
          <!-- 登录码是这一屏此刻唯一要做的事，所以给它整行和最大的字号。 -->
          <div class="fields">
            <span class="field-label">
              在浏览器中输入以下代码<small>密码仅在微软页面输入，不经过 Fern。</small>
            </span>
            <p class="code t-mono selectable">{accounts.deviceCode.userCode}</p>
            <p class="t-mono site selectable">{accounts.deviceCode.verificationUri}</p>
          </div>
        {:else}
          <div class="fields">
            <span class="field-label">
              微软账户<small>获取登录码后在浏览器中完成验证，无需在此输入密码。</small>
            </span>
            <button class="btn btn--primary" disabled={accounts.busy} onclick={() => void submitMicrosoft()}>
              {accounts.busy ? '等待中' : '获取登录码'}
            </button>
          </div>
        {/if}
      {:else}
        <form class="fields" onsubmit={(event) => { event.preventDefault(); void submitYggdrasil() }}>
          <label class="field">
            <span>皮肤站地址<small>Yggdrasil API 根地址，可在皮肤站的「在启动器中使用」页面获取。</small></span>
            <input class="input" bind:value={apiRoot} spellcheck="false" />
          </label>
          <label class="field">
            <span>邮箱</span>
            <input class="input" bind:value={username} spellcheck="false" autocomplete="username" />
          </label>
          <label class="field">
            <span>密码<small>仅用于换取令牌，不会保存。令牌存入系统钥匙串。</small></span>
            <input class="input" type="password" bind:value={password} autocomplete="current-password" />
          </label>
          <button class="btn btn--primary" type="submit" disabled={accounts.busy}>
            {accounts.busy ? '登录中' : '登录'}
          </button>
        </form>
      {/if}
    </div>
  {/if}

  {#if accounts.error}<div class="alert">{accounts.error}</div>{/if}
</div>

<style>
  .accounts {
    display: grid;
    gap: var(--s4);
  }

  .empty {
    margin: 0;
  }

  .roster {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) 0;
  }

  .face {
    display: block;
    flex: none;
    width: 30px;
    height: 30px;
    overflow: hidden;
    border-radius: calc(var(--r1) * 0.8);
  }

  /* 整块名字都是切换按钮：一行里最大的那块该是最常用的动作。 */
  .who {
    display: grid;
    gap: 1px;
    flex: 1;
    min-width: 0;
    text-align: left;
    color: var(--ink-2);
    transition: color var(--t-fast) var(--ease);
  }

  .who:hover {
    color: var(--ink);
  }

  .row.on .who strong {
    color: var(--ink);
  }

  .who strong {
    overflow: hidden;
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .who small {
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    color: var(--accent);
    font-size: var(--t-micro);
  }

  .danger:hover {
    color: var(--danger);
  }

  .rename {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: 1;
  }

  .add {
    justify-self: start;
  }

  .adder {
    display: grid;
    gap: var(--s4);
    padding: var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
  }

  .kinds {
    display: flex;
    align-items: flex-start;
    gap: var(--s2);
  }

  .kind {
    display: grid;
    gap: 2px;
    flex: 1;
    padding: var(--s2) var(--s3);
    border-radius: var(--r1);
    color: var(--ink-3);
    text-align: left;
    transition:
      color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .kind:hover {
    color: var(--ink-2);
  }

  .kind.on {
    background: var(--tint-2);
    color: var(--ink);
  }

  .kind strong {
    font-size: var(--t-small);
    font-weight: 500;
  }

  .kind small {
    font-size: var(--t-micro);
    opacity: 0.7;
  }

  .close {
    flex: none;
  }

  .fields {
    display: grid;
    gap: var(--s3);
    justify-items: stretch;
  }

  .field {
    display: grid;
    gap: var(--s2);
  }

  .field > span,
  .field-label {
    display: grid;
    gap: 3px;
    color: var(--ink);
    font-size: var(--t-body);
  }

  .field small,
  .field-label small {
    max-width: 46ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.55;
  }

  .fields .btn--primary {
    justify-self: start;
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
</style>
