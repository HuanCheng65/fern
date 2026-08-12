<script lang="ts">
  /**
   * 联机场景。
   *
   * 这一屏此前是从一个竖直紧凑面板搬过来的：一根 560px 的居中列，外加一整套
   * 自己的按钮样式，不属于五个布局预设中的任何一个——布局宪法（见
   * docs/frond-design-system.md）说的是「只许从预设里选」。
   *
   * 而它的前提也已经变了：房间状态现在常驻在岛上（见 lib/pearl-session
   * 的 contributes），所以这一屏不必再充当监视器，它可以变回「去那里做事的
   * 地方」。于是落在两个预设上：
   *
   *   空态     Stage——全出血，零页面导航，一个大决定（创建还是加入）
   *   会话中   Detail——横幅放此刻的事实，tabs 分成员与诊断
   *
   * 诊断那一栏是净收益。此前的注释写着「网络细节一律不上主屏，该待在日志
   * 里」，但日志里也没有。给它一个正当的位置之后，主屏才真的干净得起来。
   */
  import { Check, Copy, LogOut } from 'lucide-svelte'
  import Stage from '../layouts/Stage.svelte'
  import Detail from '../layouts/Detail.svelte'
  import PeerCard from 'fern-kit/parts/PeerCard.svelte'
  import { DEFAULT_NAME, isConnected, session } from '../lib/pearl-session.svelte'
  import { PATH_LABEL, PUNCH_STAGE_LABEL, type PathState } from '../lib/pearl-types'
  import Button from 'fern-kit/ui/Button.svelte'
  import Input from 'fern-kit/ui/Input.svelte'

  let inviteInput = $state('')
  let copied = $state<string | null>(null)
  let tab = $state('members')

  /** 指定端口：一个小表单，收在诊断那一栏里的「手动共享」下面。 */
  let portInput = $state('')
  const portValue = $derived(Number(portInput.trim()))
  const portValid = $derived(Number.isInteger(portValue) && portValue >= 1 && portValue <= 65535)

  function submitShare(event: SubmitEvent) {
    event.preventDefault()
    if (!portValid) return
    session.sharePort(portValue)
    portInput = ''
  }

  /**
   * 问候跟着钟点走。只取一次：这一屏的停留时间是秒级的，不值得一个定时器。
   * 名字是自己起的才念出来——对着默认的「玩家」寒暄会显得像机器。
   */
  const hour = new Date().getHours()
  const greeting =
    hour < 5
      ? '夜深了'
      : hour < 11
        ? '早上好'
        : hour < 14
          ? '中午好'
          : hour < 18
            ? '下午好'
            : hour < 23
              ? '晚上好'
              : '夜深了'

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text)
      copied = what
      setTimeout(() => (copied = null), 1600)
    } catch {
      // 剪贴板被拒是常事（没有安全上下文、被策略挡掉）。地址本来就摆在屏幕上
      // 可以手抄，所以这里不该弹错误。
      copied = null
    }
  }

  const nameOf = (id: string) => session.peers.find((peer) => peer.id === id)?.name

  /**
   * 要念给朋友的是整串十二位，不是前六位。前六位是房间码，后六位是口令，
   * 缺一半进不来。分成两段是为了念得准，不是因为它们性质不同。
   */
  const groups = $derived(session.spoken?.split(/\s+/).filter(Boolean) ?? null)

  const hosting = $derived(session.mode === 'hosting')
  const online = $derived(session.connected.length)

  /** 横幅上那一行事实：此刻这个房间处在什么状态。 */
  const status = $derived.by(() => {
    if (hosting) {
      if (session.sharedPort) return `正在共享端口 ${session.sharedPort}`
      if (session.world) return session.world.motd
      if (session.watchingLan) return '等待世界开放到局域网'
      return '正在准备'
    }
    return session.localPort ? '准备就绪' : '正在连接'
  })

  const detail = $derived.by(() => {
    if (hosting) {
      if (session.sharedPort) return `连接将转发到本机 127.0.0.1:${session.sharedPort}`
      if (session.world) return '朋友现在即可加入'
      if (session.watchingLan) return '朋友可以先行加入，世界开放后会自动接入'
      return ''
    }
    if (!session.localPort) return '正在与房主建立直接连接'
    return session.lanName
      ? `进入多人游戏，在列表中找到「${session.lanName}」即可开始`
      : '进入多人游戏，房间会自动出现在列表中'
  })
</script>

{#if session.mode === 'idle'}
  <Stage>
    <p class="greet">{greeting}{session.name !== DEFAULT_NAME ? `，${session.name}` : ''}</p>
    <p class="lede">和朋友进入同一个世界。</p>

    <!-- 两扇门竖着排：创建在上、加入在下。不写口号，问候之外这一屏只有控件。 -->
    <div class="doors">
      <Button variant="primary" class="door" onclick={() => session.host(session.name)}>
        创建房间
      </Button>

      <form
        class="joiner"
        onsubmit={(event) => {
          event.preventDefault()
          if (inviteInput.trim()) session.join(inviteInput.trim(), session.name)
        }}
      >
        <input
          class="selectable"
          bind:value={inviteInput}
          placeholder="粘贴邀请码或邀请链接"
          spellcheck="false"
          autocomplete="off"
        />
        <button type="submit" disabled={!inviteInput.trim()}>加入</button>
      </form>
    </div>

    <!-- 唯一保留的一句说明，写成规格而不是广告：陈述事实，没有形容词。 -->
    <p class="fact">点对点直连 · 数据不经过服务器</p>
  </Stage>
{:else if session.ended}
  <!--
    会话整个结束了——隧道超时、房主关房、后台出错都走到这里。此后不会再有任何
    事件，房间码和成员列表描述的都是不存在的东西，所以整屏换成一句事实加一条
    出路，而不是在一份过期的名单上面挂一条提示。
  -->
  <Stage>
    <p class="eyebrow">房间{session.code ? ` · ${session.code}` : ''}</p>
    <h1 class="muted">{hosting ? '房间已关闭' : '连接已断开'}</h1>
    <p class="lede">
      {session.error ?? (hosting ? '会话已经结束。' : '与房主的连接已经关闭。')}
    </p>
    <div class="actions">
      <Button variant="ghost" onclick={() => session.leave()}>返回</Button>
    </div>
  </Stage>
{:else}
  <Detail
    tabs={[
      { id: 'members', label: hosting ? '成员' : '连接' },
      { id: 'diagnostics', label: '诊断', reading: true },
    ]}
    {tab}
    ontab={(id) => (tab = id)}
    showBanner={false}
  >
    {#snippet head()}
      <p class="eyebrow">
        {hosting ? '房间' : '已加入房间'}{session.code ? ` · ${session.code}` : ''}
      </p>

      {#if hosting && groups}
        <!-- 数字直接当视觉元素用：这一屏最大的东西就是要念出去的那串码。 -->
        <div class="code selectable">
          {#each groups as group, index (index)}<span>{group}</span>{/each}
        </div>
        <p class="hint">请将两段数字完整告知朋友，缺一不可。</p>
      {:else}
        <h1 class:muted={!hosting && !session.localPort}>{status}</h1>
      {/if}

      {#if detail}<p class="lede">{detail}</p>{/if}

      <div class="actions">
        {#if hosting}
          <Button variant="ghost" onclick={() => copy(session.invite ?? '', 'invite')}>
            {#if copied === 'invite'}<Check size={14} />{:else}<Copy size={14} />{/if}
            {copied === 'invite' ? '已复制' : '复制邀请链接'}
          </Button>
          <Button variant="ghost" onclick={() => copy(session.spoken ?? '', 'code')}>
            {copied === 'code' ? '已复制' : '复制邀请码'}
          </Button>
          <Button variant="link" onclick={() => session.leave()}>
            <LogOut size={13} strokeWidth={1.8} />关闭房间
          </Button>
        {:else}
          {#if session.localPort}
            <Button variant="ghost" onclick={() => copy(`127.0.0.1:${session.localPort}`, 'addr')}>
              {copied === 'addr' ? '已复制' : `复制地址 127.0.0.1:${session.localPort}`}
            </Button>
          {/if}
          <Button variant="link" onclick={() => session.leave()}>
            <LogOut size={13} strokeWidth={1.8} />离开房间
          </Button>
        {/if}
      </div>

      {#if !session.signalOnline}
        <!-- 信令断了不等于连接断了：已经连上的人还在，只是没人能再加入。 -->
        <p class="notice">房间服务暂时不可用 · 已建立的连接不受影响</p>
      {/if}
      {#if session.error}
        <p class="notice">{session.error}</p>
      {/if}
    {/snippet}

    {#snippet compactHead()}
      <span>{session.code ? `房间 ${session.code}` : '联机'} · {online + 1} 人</span>
    {/snippet}

    {#if tab === 'members'}
      {#if session.peers.length === 0}
        <p class="empty">尚无成员加入。将邀请码分享给朋友后，他们会出现在这里。</p>
      {:else}
        <div class="peers">
          {#each session.peers as peer (peer.id)}
            <PeerCard {peer} carrierName={peer.via ? nameOf(peer.via) : undefined} />
          {/each}
        </div>
      {/if}
    {:else}
      <!--
        网络细节的正当归宿。它们对排查有用、对玩家没用，所以既不该上主屏，
        也不该只留在日志里让人翻——连不上的时候，这一栏就是要看的地方。
      -->
      <dl class="facts">
        <div><dt>房间服务</dt><dd>{session.signalOnline ? '已连接' : session.signalDetail || '不可用'}</dd></div>
        {#if session.nodeId}
          <div><dt>本机节点</dt><dd class="t-mono selectable">{session.nodeId}</dd></div>
        {/if}
        {#if hosting}
          <div>
            <dt>共享来源</dt>
            <dd>
              {#if session.sharedPort}
                手动指定端口 {session.sharedPort}
              {:else if session.world}
                {session.world.motd} · {session.world.address}
              {:else}
                自动发现，尚未找到
              {/if}
            </dd>
          </div>
        {:else if session.localPort}
          <div><dt>本地代理</dt><dd class="t-mono">127.0.0.1:{session.localPort}</dd></div>
        {/if}
        {#each session.peers as peer (peer.id)}
          <div>
            <dt>{peer.name}</dt>
            <dd>
              {isConnected(peer) && peer.state !== 'connected'
                ? PATH_LABEL[peer.state as PathState]
                : peer.state === 'connected'
                  ? '已连接'
                  : peer.stage
                    ? PUNCH_STAGE_LABEL[peer.stage]
                    : peer.detail || '连接中'}
              {#if peer.rttMs !== undefined}<span class="t-mono"> · {peer.rttMs} ms</span>{/if}
              {#if peer.via}<span> · 经由 {nameOf(peer.via) ?? peer.via} 中转</span>{/if}
            </dd>
          </div>
        {/each}
      </dl>

      {#if hosting}
        <section class="share">
          <h2>手动共享</h2>
          <p class="t-quiet">
            共享 Minecraft 之外的游戏或应用。设定之后，朋友的连接会转发到这个本机端口，
            不再跟随游戏的局域网宣告。
          </p>
          {#if session.sharedPort}
            <Button variant="ghost" onclick={() => session.sharePort(null)}>
              改回跟随游戏
            </Button>
          {:else}
            <form class="sharer" onsubmit={submitShare}>
              <Input
                aria-label="共享端口"
                bind:value={portInput}
                placeholder="端口，如 25565"
                inputmode="numeric"
                spellcheck="false"
                autocomplete="off"
              />
              <Button variant="ghost" type="submit" disabled={!portValid}>共享</Button>
            </form>
          {/if}
        </section>
      {/if}
    {/if}
  </Detail>
{/if}

<style>
  .greet {
    margin: 0;
    font-size: clamp(28px, 8vw, 34px);
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
  }

  .eyebrow {
    margin: 0 0 var(--s3);
    font-size: var(--t-cap);
    letter-spacing: 0.3em;
    text-transform: uppercase;
    color: var(--ink-3);
  }

  h1 {
    margin: 0;
    font-size: clamp(26px, 7.5vw, 36px);
    font-weight: 500;
    line-height: 1.15;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  h1.muted {
    color: var(--ink-3);
  }

  .lede {
    margin: var(--s4) 0 0;
    color: var(--ink-2);
    font-size: var(--t-lead);
  }

  /*
   * 十二位要在一行里念完，换行会让人以为那是两个东西。窗口窄，所以字号
   * 跟着窗口走，到上限为止。
   */
  .code {
    display: flex;
    gap: var(--s4);
    font-family: var(--mono);
    font-size: clamp(30px, 10vw, 40px);
    line-height: 1;
    letter-spacing: 0.04em;
    font-variant-numeric: tabular-nums;
  }

  .hint {
    margin: var(--s3) 0 0;
    font-size: var(--t-cap);
    letter-spacing: 0.08em;
    color: var(--ink-3);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
    margin-top: var(--s5);
  }

  .notice {
    margin: var(--s4) 0 0;
    color: var(--ink-3);
    font-size: var(--t-cap);
    letter-spacing: 0.06em;
  }

  .doors {
    margin-top: var(--s7);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
    max-width: 460px;
  }

  /* 布局归调用方，但 Svelte 的作用域样式进不了组件，所以罩一层自己的祖先。 */
  .doors :global(.door) {
    justify-content: center;
    padding: var(--s4) var(--s6);
    font-size: var(--t-h3);
    letter-spacing: 0.04em;
  }

  /* 输入框和按钮共用一只胶囊：加入是一件事，不是一个字段加一个按钮。 */
  .joiner {
    display: flex;
    align-items: stretch;
    border-radius: var(--r2);
    background: var(--glass);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.06),
      var(--shadow-1);
    overflow: hidden;
    transition: box-shadow var(--t-base) var(--ease);
  }

  .joiner:focus-within {
    box-shadow:
      inset 0 0 0 1.5px var(--c3),
      var(--shadow-1);
  }

  .joiner input {
    flex: 1;
    min-width: 0;
    padding: var(--s3) var(--s4);
    color: var(--ink);
  }

  .joiner input::placeholder {
    color: var(--ink-3);
  }

  .joiner button {
    padding: var(--s3) var(--s5);
    color: var(--ink-2);
    border-left: 1px solid var(--line-2);
    letter-spacing: 0.08em;
    transition:
      color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .joiner button:not(:disabled):hover {
    color: var(--ink);
    background: rgba(255, 255, 255, 0.06);
  }

  .joiner button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .fact {
    margin: var(--s7) 0 0;
    font-size: var(--t-cap);
    letter-spacing: 0.14em;
    color: var(--ink-3);
  }

  .peers {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }

  .empty {
    margin: 0;
    padding: var(--s5);
    border-radius: var(--r2);
    background: var(--glass);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.05),
      var(--shadow-1);
    color: var(--ink-3);
    font-size: var(--t-body);
  }

  /* 事实一行一条，名字在左、值在右，扫下来就是一份可以贴出去的报告。 */
  .facts {
    display: grid;
    gap: 1px;
    margin: 0;
  }

  .facts div {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s4);
    padding: var(--s3) 0;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
  }

  .facts dt {
    flex: none;
    color: var(--ink-2);
    font-size: var(--t-body);
  }

  .facts dd {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-small);
    text-align: right;
    overflow-wrap: anywhere;
  }

  .share {
    margin-top: var(--s6);
  }

  .share h2 {
    margin: 0 0 var(--s2);
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  .share .t-quiet {
    margin: 0 0 var(--s3);
  }

  .sharer {
    display: flex;
    gap: var(--s2);
    max-width: 320px;
  }
</style>
