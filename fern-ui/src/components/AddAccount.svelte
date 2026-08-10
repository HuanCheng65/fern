<script lang="ts">
  /**
   * 添加账户。设置里的二级页。
   *
   * 三种方式要问的东西差别极大：离线要一个名字，正版要你去浏览器输一个八位码，
   * 外置要三个字段。上一版把三者塞进名单里就地展开，于是这一块的高度每选一次
   * 就跳一次，而且三张表单必须同时存在于同一段标记里。
   *
   * **哪一种由上一层决定。** 名单末尾那颗「添加账户」展开的就是这三条，选完
   * 直接落在对应的表单上——分岔长在按钮上，不必再占一整屏。这里仍然留着那一屏：
   * `account/list/new` 是可寻址的（⌘K 里的「添加账户」就落在这儿），没带上
   * 种类时总得问一次。选错的代价不小，所以两处都写清楚每种是什么。
   */
  import { untrack } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { ArrowLeft, ExternalLink } from 'lucide-svelte'
  import {
    accounts,
    addableKinds,
    verificationTarget,
    type AccountKind,
  } from '../lib/accounts.svelte'
  import { notices } from '../lib/notices.svelte'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

  interface Props {
    /** 加成了就带着新账户的 id 回去，取消则不带。 */
    ondone: (id?: string) => void
    /** 上一层已经选好了哪一种。空串表示还得在这里问。 */
    initial?: AccountKind | ''
  }

  let { ondone, initial = '' }: Props = $props()

  /**
   * 空串表示还停在选择那一步。
   *
   * 只取一次上一层给的那个值：进来之后它就归这一屏管了——按「换一种方式」
   * 退回选择步骤，不该被那个还挂在地址上的种类再拽回去。地址换了种类时
   * 由调用方重建这一屏（见 routes/Settings.svelte 里的 `{#key}`）。
   */
  let kind = $state<AccountKind | ''>(untrack(() => initial))
  let offlineName = $state('')
  let apiRoot = $state('https://littleskin.cn/api/yggdrasil')
  let username = $state('')
  let password = $state('')

  const OFFLINE_NAME = /^[A-Za-z0-9_]{3,16}$/
  /** 官方商店。离线那一步要给得出一条通往正版的路。 */
  const BUY_URL = 'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc'

  /** 交给系统浏览器。后端只放行 https。 */
  const openExternal = (url: string) => void invoke('open_external', { url })

  const KINDS = addableKinds()

  /** 加完之后新出现的那一个就是它。名册按添加顺序排，最新的在最后。 */
  const newest = () => accounts.list[accounts.list.length - 1]?.id

  function pick(next: AccountKind) {
    kind = next
    accounts.error = ''
  }

  function back() {
    kind = ''
    password = ''
    accounts.error = ''
  }

  function finish(name: string) {
    notices.say({ title: `已添加 ${name}` })
    ondone(newest())
  }

  async function submitOffline() {
    const name = offlineName.trim()
    if (!OFFLINE_NAME.test(name)) return
    await accounts.addOffline(name)
    if (!accounts.error) finish(name)
  }

  async function submitYggdrasil() {
    if (!apiRoot.trim() || !username.trim() || !password) return
    await accounts.loginYggdrasil(apiRoot.trim(), username.trim(), password)
    // 拿到令牌就把密码从内存里去掉，它已经没有用处了。
    password = ''
    if (!accounts.error) finish(accounts.list[accounts.list.length - 1]?.playerName ?? '账户')
  }

  async function submitMicrosoft() {
    await accounts.loginMicrosoft()
    if (!accounts.error) finish(accounts.list[accounts.list.length - 1]?.playerName ?? '账户')
  }
</script>

<div class="adder">
  {#if kind === ''}
    <p class="lead t-quiet">可以同时保存多个身份，之后随时切换。</p>
    <div class="kinds">
      {#each KINDS as option (option.kind)}
        <button class="kind" onclick={() => pick(option.kind)}>
          <strong>{option.title}</strong>
          <small>{option.note}</small>
        </button>
      {/each}
    </div>
  {:else}
    <div class="step-back">
      <Button variant="link" tone="quiet" onclick={back}>
        <ArrowLeft size={14} strokeWidth={2} />换一种方式
      </Button>
    </div>

    {#if kind === 'offline'}
      <form
        class="fields"
        onsubmit={(event) => {
          event.preventDefault()
          void submitOffline()
        }}
      >
        <Input
          label="玩家名称"
          hint="3–16 位字母、数字或下划线。UUID 由名称推导，修改名称即更换身份。"
          bind:value={offlineName}
          maxlength={16}
          spellcheck="false"
          placeholder="Steve"
        />
        <div class="submit">
          <Button variant="primary" type="submit" disabled={!OFFLINE_NAME.test(offlineName.trim())}>
            添加
          </Button>
          <Button variant="link" onclick={() => openExternal(BUY_URL)}>
            购买正版 Minecraft<ExternalLink size={13} strokeWidth={1.8} />
          </Button>
        </div>
      </form>
    {:else if kind === 'microsoft'}
      {#if accounts.deviceCode}
        {@const code = accounts.deviceCode}
        <!-- 登录码是这一屏此刻唯一要做的事，所以给它整行和最大的字号。 -->
        <div class="fields">
          <span class="field-label">
            浏览器已经打开，在其中输入以下代码
            <small>密码仅在微软页面输入，不经过 Fern。</small>
          </span>
          <p class="code t-mono selectable">{code.userCode}</p>
          <p class="t-mono site selectable">{code.verificationUri}</p>
          <div class="submit">
            <!-- 轮询自己也会发现，这一颗省的是那几秒的干等。 -->
            <Button variant="primary" onclick={() => void accounts.checkMicrosoft()}>
              我已完成登录
            </Button>
            <Button variant="link" onclick={() => openExternal(verificationTarget(code))}>
              重新打开页面<ExternalLink size={13} strokeWidth={1.8} />
            </Button>
          </div>
        </div>
      {:else}
        <div class="fields">
          <span class="field-label">
            微软账户
            <small>将打开浏览器完成验证，无需在此输入密码。</small>
          </span>
          <Button variant="primary" disabled={accounts.busy} onclick={() => void submitMicrosoft()}>
            {accounts.busy ? '等待中' : '打开浏览器登录'}
          </Button>
        </div>
      {/if}
    {:else}
      <form
        class="fields"
        onsubmit={(event) => {
          event.preventDefault()
          void submitYggdrasil()
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
          hint="仅用于换取令牌，不会保存。令牌存入系统钥匙串。"
          type="password"
          bind:value={password}
          autocomplete="current-password"
        />
        <div class="submit">
          <Button variant="primary" type="submit" disabled={accounts.busy}>
            {accounts.busy ? '登录中' : '登录'}
          </Button>
        </div>
      </form>
    {/if}
  {/if}

  {#if accounts.error}<div class="alert">{accounts.error}</div>{/if}
</div>

<style>
  .adder {
    display: grid;
    gap: var(--s5);
  }

  .lead {
    margin: 0;
    font-size: var(--t-small);
  }

  /* 三种方式各占一整行：它们要说的话不一样长，挤成三列会把最长的那句截断。 */
  .kinds {
    display: grid;
    gap: var(--s2);
  }

  .kind {
    display: grid;
    gap: 3px;
    padding: var(--s3) var(--s4);
    border-radius: var(--r2);
    background: var(--tint-1);
    color: var(--ink-3);
    text-align: left;
    transition:
      color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .kind:hover {
    background: var(--tint-2);
    color: var(--ink-2);
  }

  .kind strong {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .kind small {
    font-size: var(--t-small);
  }

  .step-back {
    justify-self: start;
  }

  .fields {
    display: grid;
    gap: var(--s4);
    justify-items: stretch;
  }

  .field-label {
    display: grid;
    gap: 3px;
    color: var(--ink);
    font-size: var(--t-body);
  }

  .field-label small {
    max-width: 46ch;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.55;
  }

  .fields .submit {
    display: flex;
    align-items: center;
    gap: var(--s4);
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
