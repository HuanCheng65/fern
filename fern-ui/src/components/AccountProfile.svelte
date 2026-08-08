<script lang="ts">
  /**
   * 一个账户的档案。设置里的二级页。
   *
   * 存在的理由：账户不是一个值，是一个**有身份、有状态、有几个动作的东西**。
   * 挤在名单的一行里，就只能靠就地展开来说完这些话——而每一次展开都会把下面
   * 所有的行往下顶一截。
   *
   * 这一页回答三个问题，顺序就是它们的重要程度：这是谁（脸和名字）、它是什么
   * （类型、UUID、皮肤站）、有谁在用它（绑定的实例）。动作压在最下面，危险的
   * 那一个要再点一次才成立。
   *
   * **UUID 摆出来而不是藏起来。** 离线账户的 UUID 由名字推导，改名就等于换了
   * 一个人——存档里的物品、领地插件的记录都认这个号。这句话只有在能看见那串
   * 数字的时候才说得清楚。
   */
  import { Check, Trash2 } from 'lucide-svelte'
  import Cover from 'fern-kit/Cover.svelte'
  import { accounts, KIND_LABEL, siteName } from '../lib/accounts.svelte'
  import { instances } from '../lib/instances.svelte'
  import { notices } from '../lib/notices.svelte'

  interface Props {
    accountId: string
    /** 账户没了，这一页也就没有内容了。 */
    ongone: () => void
  }

  let { accountId, ongone }: Props = $props()

  const account = $derived(accounts.list.find((item) => item.id === accountId))
  const isActive = $derived(accounts.active?.id === accountId)
  /** 记着用这个账户的实例。它们是「移除会影响到什么」的答案。 */
  const bound = $derived(instances.list.filter((item) => item.accountId === accountId))

  const OFFLINE_NAME = /^[A-Za-z0-9_]{3,16}$/

  let renaming = $state(false)
  let renameTo = $state('')
  let confirmingRemove = $state(false)

  function startRename() {
    renaming = true
    renameTo = account?.playerName ?? ''
  }

  async function submitRename() {
    if (!account || !OFFLINE_NAME.test(renameTo.trim())) return
    await accounts.renameOffline(account.id, renameTo.trim())
    renaming = false
    if (!accounts.error) notices.say({ title: `已改名为 ${renameTo.trim()}` })
  }

  async function remove() {
    if (!account) return
    const name = account.playerName
    await accounts.remove(account.id)
    if (accounts.error) return
    notices.say({ title: `已移除 ${name}` })
    ongone()
  }

  const added = $derived(
    account ? new Date(account.addedAt * 1000).toISOString().slice(0, 10) : '',
  )
</script>

{#if !account}
  <p class="t-quiet">这个账户已经不在名单里了。</p>
{:else}
  <div class="profile">
    <header class="who">
      <span class="face"><Cover seed={account.uuid} quality={0.6} /></span>
      <div class="names">
        {#if renaming}
          <form
            class="rename"
            onsubmit={(event) => {
              event.preventDefault()
              void submitRename()
            }}
          >
            <input class="input" bind:value={renameTo} maxlength="16" spellcheck="false" />
            <button class="btn btn--primary" type="submit" disabled={!OFFLINE_NAME.test(renameTo.trim())}>
              保存
            </button>
            <button class="btn btn--link" type="button" onclick={() => (renaming = false)}>取消</button>
          </form>
        {:else}
          <h2>{account.playerName}</h2>
        {/if}
        <p class="kind t-quiet">
          {KIND_LABEL[account.kind]}{account.apiRoot ? ` · ${siteName(account.apiRoot)}` : ''}
        </p>
      </div>

      {#if isActive}
        <span class="badge"><Check size={12} strokeWidth={2.6} />当前使用</span>
      {:else}
        <button class="btn btn--ghost" onclick={() => void accounts.use(account.id)}>设为当前</button>
      {/if}
    </header>

    <dl class="facts">
      <div>
        <dt>UUID</dt>
        <dd class="t-mono selectable">{account.uuid}</dd>
      </div>
      {#if account.apiRoot}
        <div>
          <dt>皮肤站</dt>
          <dd class="t-mono selectable">{account.apiRoot}</dd>
        </div>
      {/if}
      <div>
        <dt>添加于</dt>
        <dd class="t-mono">{added}</dd>
      </div>
    </dl>

    {#if account.kind === 'offline'}
      <p class="t-quiet note">
        离线账户的 UUID 由名字推导，改名等同于换一个人：存档里属于旧名字的东西不会跟过来。
      </p>
    {/if}

    <!--
      「谁在用它」必须说。移除一个被三个实例记着的账户，那三个实例下次启动
      会退回当前账户——这件事该在按下移除之前就看得见。
    -->
    <section>
      <h3>使用这个账户的实例</h3>
      {#if bound.length === 0}
        <p class="t-quiet">没有实例专门记着它。实例第一次成功启动时会记下当时用的账户。</p>
      {:else}
        <ul class="bound">
          {#each bound as item (item.id)}
            <li>
              <span>{item.name}</span>
              <button class="btn btn--link" onclick={() => void instances.setAccount(item.id, null)}>
                解除
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <div class="acts">
      {#if account.kind === 'offline' && !renaming}
        <button class="btn btn--ghost" onclick={startRename}>改名</button>
      {/if}
      {#if confirmingRemove}
        <span class="confirm">
          <span class="t-quiet">
            移除后需要重新登录{bound.length > 0 ? `，${bound.length} 个实例会退回当前账户` : ''}。
          </span>
          <button class="btn btn--ghost danger" onclick={() => void remove()}>确认移除</button>
          <button class="btn btn--link" onclick={() => (confirmingRemove = false)}>取消</button>
        </span>
      {:else}
        <button class="btn btn--link danger" onclick={() => (confirmingRemove = true)}>
          <Trash2 size={13} strokeWidth={1.9} />移除账户
        </button>
      {/if}
    </div>

    {#if accounts.error}<div class="alert">{accounts.error}</div>{/if}
  </div>
{/if}

<style>
  .profile {
    display: grid;
    gap: var(--s6);
  }

  .who {
    display: flex;
    align-items: center;
    gap: var(--s4);
  }

  .face {
    display: block;
    flex: none;
    width: 64px;
    height: 64px;
    overflow: hidden;
    border-radius: var(--r2);
  }

  .names {
    flex: 1;
    min-width: 0;
  }

  .names h2 {
    margin: 0;
    color: var(--ink);
    font-size: var(--t-h2);
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .kind {
    margin: 2px 0 0;
    font-size: var(--t-small);
  }

  .rename {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    color: var(--accent);
    font-size: var(--t-small);
  }

  .facts {
    display: grid;
    gap: var(--s4);
    margin: 0;
  }

  .facts dt {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .facts dd {
    margin: 3px 0 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    overflow-wrap: anywhere;
  }

  .note {
    margin: 0;
    max-width: 56ch;
    font-size: var(--t-small);
    line-height: 1.6;
  }

  h3 {
    margin: 0 0 var(--s3);
    color: var(--ink-2);
    font-size: var(--t-small);
    font-weight: 500;
  }

  .bound {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .bound li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding: var(--s2) 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .bound li:last-child {
    box-shadow: none;
  }

  .acts {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }

  .confirm {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--s3);
    font-size: var(--t-small);
  }

  .danger:hover {
    color: var(--danger);
  }
</style>
