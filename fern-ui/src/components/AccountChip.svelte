<script lang="ts">
  /**
   * 「以谁的身份启动」，站在启动键旁边。
   *
   * 身份原来在顶栏。挪下来是因为顶栏是全应用唯一常驻的东西，而身份并不是随时
   * 都相关——它只在**要按启动**的那一刻最重要，而那一刻它非常重要：用错账户
   * 意味着进错服、白名单不认、存档里是另一个人。
   *
   * 换一个不是「切换全局账户」，是「这个实例用这个」。所以点开的清单落在实例
   * 上，选完立刻记住；下周再打开这个整合包，它还是用小号，哪怕这期间你用大号
   * 玩过别的。绑定不需要一个绑定界面，它是「记住上次」的副产品。
   */
  import { Check } from 'lucide-svelte'
  import Cover from 'fern-kit/Cover.svelte'
  import { accounts, KIND_LABEL, siteName } from '../lib/accounts.svelte'
  import { instances } from '../lib/instances.svelte'
  import { nav } from '../lib/nav.svelte'

  interface Props {
    instanceId: string
  }

  let { instanceId }: Props = $props()

  let open = $state(false)
  let root = $state<HTMLElement>()

  const instance = $derived(instances.list.find((item) => item.id === instanceId))
  /** 实例记着的那一个优先，没记过就跟当前的走——和后端同一条规则。 */
  const account = $derived(
    accounts.list.find((item) => item.id === instance?.accountId) ?? accounts.active,
  )

  async function pick(id: string | null) {
    open = false
    await instances.setAccount(instanceId, id)
  }

  function onWindowPointerDown(event: PointerEvent) {
    if (open && root && !root.contains(event.target as Node)) open = false
  }
</script>

<svelte:window
  onpointerdown={onWindowPointerDown}
  onkeydown={(event) => event.key === 'Escape' && (open = false)}
/>

<div class="chip-root" bind:this={root}>
  <button class="chip" onclick={() => (open = !open)} title="启动身份">
    {#if account}
      <span class="face"><Cover seed={account.uuid} quality={0.4} /></span>
      <span class="who">{account.playerName}</span>
    {:else}
      <span class="face empty"></span>
      <span class="who none">添加账户</span>
    {/if}
  </button>

  {#if open}
    <div class="menu">
      {#if accounts.list.length === 0}
        <p class="none-row">尚未添加账户</p>
      {/if}
      {#each accounts.list as item (item.id)}
        <button class="row" onclick={() => void pick(item.id)}>
          <span class="face"><Cover seed={item.uuid} quality={0.4} /></span>
          <span class="text">
            <strong>{item.playerName}</strong>
            <small>
              {KIND_LABEL[item.kind]}{item.apiRoot ? ` · ${siteName(item.apiRoot)}` : ''}
            </small>
          </span>
          {#if account?.id === item.id}<Check size={14} strokeWidth={2.4} />{/if}
        </button>
      {/each}
      <button class="row manage" onclick={() => { open = false; nav.show('settings', 'account') }}>
        管理账户…
      </button>
    </div>
  {/if}
</div>

<style>
  .chip-root {
    position: relative;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: var(--s2);
    /* 左右不对称：左边贴着头像，右边是文字，两侧留一样多会显得头像掉在外面。 */
    padding: var(--s1) var(--s4) var(--s1) var(--s1);
    border-radius: 999px;
    background: var(--glass);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
    color: var(--ink-2);
    font-size: var(--t-small);
    transition: color var(--t-fast) var(--ease);
  }

  .chip:hover {
    color: var(--ink);
  }

  .face {
    display: block;
    flex: none;
    width: 20px;
    height: 20px;
    overflow: hidden;
    border-radius: 999px;
  }

  /* 芯片和启动键并排，一样高；头像跟着长满，否则它像掉在一个空盒子里。 */
  .chip .face {
    width: 32px;
    height: 32px;
  }

  .face.empty {
    background: var(--tint-2);
  }

  .who {
    max-width: 14ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .who.none {
    color: var(--ink-3);
  }

  /* 往上开：这颗芯片贴着屏幕底部那一行，往下没有地方。 */
  .menu {
    position: absolute;
    bottom: calc(100% + var(--s2));
    left: 0;
    z-index: 20;
    display: grid;
    gap: 1px;
    min-width: 220px;
    padding: var(--s2);
    border-radius: var(--r2);
    background: var(--glass-2);
    -webkit-backdrop-filter: blur(24px);
    backdrop-filter: blur(24px);
    box-shadow:
      inset 0 0 0 1px var(--hairline-2),
      var(--shadow-2);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2);
    border-radius: var(--r1);
    color: var(--ink-2);
    text-align: left;
    transition: background var(--t-fast) var(--ease);
  }

  .row:hover {
    background: var(--tint-2);
    color: var(--ink);
  }

  .text {
    display: grid;
    gap: 1px;
    flex: 1;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    font-size: var(--t-small);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-3);
    font-size: var(--t-micro);
  }

  .manage {
    color: var(--ink-3);
    font-size: var(--t-small);
    box-shadow: inset 0 1px 0 var(--hairline-2);
    border-radius: 0 0 var(--r1) var(--r1);
  }

  .none-row {
    margin: 0;
    padding: var(--s2);
    color: var(--ink-3);
    font-size: var(--t-small);
  }
</style>
