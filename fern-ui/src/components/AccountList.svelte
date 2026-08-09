<script lang="ts">
  /**
   * 设置里的账户那一节：一份名单，没有别的。
   *
   * 一份名单加一个「添加」，不是三选一的单选框。之前那一版把「登录方式」做成
   * 一个全局开关，于是同时有一个正版号和一个测试用的离线号是不可能的——想换
   * 一个就得把另一个挤掉。名单让身份变回一件可以有很多个的东西。
   *
   * **这一节只负责「有哪些」。** 添加、改名、移除、看 UUID 全部走二级页
   * （见 routes/Settings.svelte）。上一版把它们都做成就地展开：添加时在名单
   * 中间撑开一整张表单，改名时把那一行换成一个输入框——每一次展开都把下面所有
   * 的行往下顶一截，而三种登录方式要问的东西差别极大（离线一个名字、正版一个
   * 八位码、外置三个字段），撑开的高度每次都不一样。
   *
   * 一行只做一件最常用的事：**点名字切换到它**。其余的进档案。
   */
  import { Check, ChevronRight, Plus } from 'lucide-svelte'
  import AccountFace from './AccountFace.svelte'
  import { accounts, originOf } from '../lib/accounts.svelte'
  import { nav } from '../lib/nav.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  /** 二级页的地址。这一行本身的 id 是 `account/list`。 */
  const open = (target: string) => nav.show('settings', `account/list/${target}`)
</script>

<div class="accounts">
  {#if accounts.list.length === 0 && !accounts.loading}
    <p class="t-quiet empty">尚未添加账户。添加后方可启动游戏。</p>
  {/if}

  <ul class="roster">
    {#each accounts.list as account (account.id)}
      <li class="row" class:on={accounts.active?.id === account.id}>
        <AccountFace {account} size={30} />

        <button
          class="who"
          aria-pressed={accounts.active?.id === account.id}
          onclick={() => void accounts.use(account.id)}
        >
          <strong>{account.playerName}</strong>
          <!-- 这一处永远写全出处：这份名单的职责就是把同名的人分开。 -->
          <small>{originOf(account)}</small>
        </button>

        {#if accounts.active?.id === account.id}
          <span class="badge"><Check size={12} strokeWidth={2.6} />当前</span>
        {/if}

        <button class="more" aria-label="{account.playerName} 的档案" onclick={() => open(account.id)}>
          <ChevronRight size={15} strokeWidth={2} />
        </button>
      </li>
    {/each}
  </ul>

  <Button variant="ghost" class="add" onclick={() => open('new')}>
    <Plus size={14} strokeWidth={2} />添加账户
  </Button>

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

  /* 进档案的入口。箭头是「这里还有一层」的标准说法，不用再写一遍字。 */
  .more {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--r1);
    color: var(--ink-4);
    transition:
      color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .more:hover {
    background: var(--tint-1);
    color: var(--ink);
  }

  /* 布局归调用方，但 Svelte 的作用域样式进不了组件，所以罩一层自己的祖先。 */
  .accounts :global(.add) {
    justify-self: start;
  }
</style>
