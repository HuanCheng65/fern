<script lang="ts">
  /**
   * 添加账户。设置里的二级页。
   *
   * 三种方式要问的东西差别极大：离线要一个名字，正版要你去浏览器输一个八位码，
   * 外置要三个字段。上一版把三者塞进名单里就地展开，于是这一块的高度每选一次
   * 就跳一次，而且三张表单必须同时存在于同一段标记里。
   *
   * 第一步只做一件事：说清楚三种方式各自是什么。这一步选错的代价不小——离线
   * 账户进不了正版服务器，而这句话要在选之前说，不是在失败之后说。
   */
  import { invoke } from '@tauri-apps/api/core'
  import { ArrowLeft, ExternalLink } from 'lucide-svelte'
  import { accounts, verificationTarget, type AccountKind } from '../lib/accounts.svelte'
  import { notices } from '../lib/notices.svelte'
  import { offlineLoginAllowed } from '../lib/region'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

  interface Props {
    /** 加成了就带着新账户的 id 回去，取消则不带。 */
    ondone: (id?: string) => void
  }

  let { ondone }: Props = $props()

  /** 空串表示还停在第一步。 */
  let kind = $state<AccountKind | ''>('')
  let offlineName = $state('')
  let apiRoot = $state('https://littleskin.cn/api/yggdrasil')
  let username = $state('')
  let password = $state('')

  const OFFLINE_NAME = /^[A-Za-z0-9_]{3,16}$/
  /** 官方商店。离线那一步要给得出一条通往正版的路。 */
  const BUY_URL = 'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc'

  /** 交给系统浏览器。后端只放行 https。 */
  const openExternal = (url: string) => void invoke('open_external', { url })

  // 离线登录按地区提供，和首次启动向导用的是同一条判断（见 lib/region.ts）。
  // 关掉的只是这个入口，名册里已有的离线账户照常能用。
  const KINDS: { kind: AccountKind; title: string; note: string }[] = [
    { kind: 'microsoft', title: '微软账户', note: '正版登录，支持联机、皮肤与成就' },
    { kind: 'authlib', title: '外置登录', note: 'LittleSkin 等 Yggdrasil 兼容皮肤站' },
    ...(offlineLoginAllowed()
      ? [{ kind: 'offline' as const, title: '离线模式', note: '仅可游玩本地世界与离线服务器' }]
      : []),
  ]

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
            <Button variant="ghost" onclick={() => openExternal(verificationTarget(code))}>
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
