<script lang="ts">
  /**
   * 联机场景。
   *
   * 这是一个会话工具，不是落地页。真实的使用时间线：开房三十秒，然后几个
   * 小时挂在后台，偶尔被瞥一眼「朋友还连着吗」。所以布局是一根竖列——
   * 头部是当下的状态（码、世界名、连接进度），下面是成员，一眼从上扫到下。
   * 同类软件（语音面板、组网工具）全是这个形状，因为内容的形状就是这样。
   *
   * 一次只回答一个问题（见 docs/UI_DESIGN.md 一）：没开房时问「创建还是加入」，
   * 开了房问「码在这里，人到了没有」，加入后问「怎么进游戏」。三种状态各自
   * 只有一屏，不做标签页，不做折叠区。
   *
   * 屏幕上最大的元素永远是状态和数据，不是宣传句。介绍产品是官网的事，
   * 已经打开软件的人需要的是控件和事实。
   *
   * 网络细节一律不上主屏。NAT 类型、候选地址、STUN 结果这些东西对排查有用、
   * 对玩家没用，它们该待在日志里，等真的连不上时再拿出来。
   */
  import { DEFAULT_NAME, session } from '../lib/pearl-session.svelte'
  import PeerCard from '../components/PeerCard.svelte'

  let inviteInput = $state('')
  let copied = $state<string | null>(null)

  /**
   * 「指定端口」收在世界状态那一行的右侧,点开是一个小弹层。它不占自己的
   * 一行——MC 玩家永远不用看见它展开,而它就站在它要改写的那句话旁边。
   */
  let sharing = $state(false)
  let portInput = $state('')
  let shareTool = $state<HTMLElement>()
  const portValue = $derived(Number(portInput.trim()))
  const portValid = $derived(Number.isInteger(portValue) && portValue >= 1 && portValue <= 65535)

  function submitShare(e: SubmitEvent) {
    e.preventDefault()
    if (!portValid) return
    session.sharePort(portValue)
    sharing = false
    portInput = ''
  }

  function closeShare() {
    sharing = false
    portInput = ''
  }

  /** 弹层的退路:点到外面或按 Esc 都算「算了」。 */
  function onWindowPointerDown(e: PointerEvent) {
    if (sharing && shareTool && !shareTool.contains(e.target as Node)) closeShare()
  }

  function onWindowKeyDown(e: KeyboardEvent) {
    if (sharing && e.key === 'Escape') closeShare()
  }

  /** 弹层是为了输入才开的,开了就把光标放进去。 */
  function autofocus(node: HTMLInputElement) {
    node.focus()
  }

  /**
   * 问候跟着钟点走，像游戏主菜单对着「现在」说话。只取一次：这一屏的停留
   * 时间是秒级的，不值得一个定时器。名字是自己起的才念出来——对着默认的
   * 「玩家」寒暄会显得像机器。
   */
  const hour = new Date().getHours()
  const greeting =
    hour < 5 ? '夜深了' : hour < 11 ? '早上好' : hour < 14 ? '中午好' : hour < 18 ? '下午好' : hour < 23 ? '晚上好' : '夜深了'

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text)
      copied = what
      setTimeout(() => (copied = null), 1600)
    } catch {
      // 剪贴板被拒是常事（没有安全上下文、被策略挡掉）。地址本来就摆在
      // 屏幕上可以手抄，所以这里不该弹错误吓人。
      copied = null
    }
  }

  const nameOf = (id: string) => session.peers.find((p) => p.id === id)?.name

  /**
   * 要念给朋友的是整串十二位，不是前六位。
   *
   * 前六位是房间码，后六位是口令，缺一半进不来。之前这里只显示前六位，等于
   * 请玩家把一个不完整的东西念出去——朋友照着输，进不来，而屏幕上看不出为什么。
   *
   * 分成两段是为了念得准，不是为了它们性质不同。
   */
  const groups = $derived(session.spoken?.split(/\s+/).filter(Boolean) ?? null)
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onWindowKeyDown} />

<section class="scene" class:centered={session.mode === 'idle'}>
  {#if session.mode === 'idle'}
    <div class="hero">
      <p class="greet">{greeting}{session.name !== DEFAULT_NAME ? `，${session.name}` : ''}</p>
      <p class="greet-sub">和朋友进入同一个世界。</p>

      <!-- 两扇门竖着排：创建在上、加入在下，占满这根列。不写口号，问候之外
           这一屏就只有控件。 -->
      <div class="doors">
        <button class="primary" onclick={() => session.host(session.name)}>创建房间</button>

        <form
          class="joiner"
          onsubmit={(e) => {
            e.preventDefault()
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

      <!-- 唯一保留的一句说明，写成规格而不是广告：陈述事实，没有形容词。
           昵称不在这里：它住在右上角的面板里，和其他偏好在一起。 -->
      <p class="fact">点对点直连 · 数据不经过服务器</p>
    </div>
  {:else if session.ended}
    <!-- 会话整个结束了——隧道超时、房主关房、后台出错都走到这里。此后不会
         再有任何事件,房间码和成员列表描述的都是不存在的东西,所以整屏换成
         一句事实加一条出路,而不是在「准备就绪」下面弹一条 toast。 -->
    <header class="head">
      <p class="eyebrow">房间{session.code ? ` · ${session.code}` : ''}</p>
      <h1 class="muted">{session.mode === 'hosting' ? '房间已关闭' : '连接已断开'}</h1>
      <p class="lede">
        {session.error ?? (session.mode === 'hosting' ? '会话已经结束。' : '与房主的连接已经关闭。')}
      </p>
      <div class="beneath">
        <button class="ghost" onclick={() => session.leave()}>返回</button>
      </div>
    </header>
  {:else if session.mode === 'hosting'}
    <header class="head">
      <p class="eyebrow">房间已创建</p>
      {#if groups}
        <!-- 数字直接当视觉元素用：这一屏最大的东西就是要念出去的那串码。 -->
        <div class="code selectable">
          {#each groups as group, i (i)}<span>{group}</span>{/each}
        </div>
        <p class="hint">请将两段数字完整告知朋友，缺一不可。</p>
        <div class="beneath">
          <button class="ghost" onclick={() => copy(session.invite ?? '', 'invite')}>
            {copied === 'invite' ? '已复制' : '复制邀请链接'}
          </button>
          <button class="ghost" onclick={() => copy(session.spoken ?? '', 'code')}>
            {copied === 'code' ? '已复制' : '复制邀请码'}
          </button>
        </div>
      {:else}
        <div class="code placeholder">· · · · · · &nbsp; · · · · · ·</div>
        <p class="hint muted">正在创建房间</p>
      {/if}
    </header>

    <div class="world">
      {#if session.sharedPort}
        <!-- 手动指定优先于自动发现:设了端口,转发就指着它,游戏宣告什么都不改变这一点。 -->
        <span class="dot on"></span>
        <div class="wtext">
          <div class="wname">正在共享端口 {session.sharedPort}</div>
          <div class="muted small">朋友的连接会转发到本机 127.0.0.1:{session.sharedPort}</div>
        </div>
      {:else if session.world}
        <span class="dot on"></span>
        <div class="wtext">
          <!-- 不加书名号：全角引号的空腔在大字号下左右不对称，名字本身的
               字重和位置已经说明它是名字。 -->
          <div class="wname">{session.world.motd}</div>
          <!-- 从玩家那一侧说。转发到哪个端口是 Pearl 的内务，主屏不提。 -->
          <div class="muted small">朋友现在即可加入</div>
        </div>
      {:else if session.watchingLan}
        <span class="dot wait"></span>
        <div class="wtext">
          <div class="wname">等待世界开放到局域网</div>
          <!-- 说清楚这不是错误状态：朋友可以先进房间等着。 -->
          <div class="muted small">朋友可以先行加入，世界开放后会自动接入</div>
        </div>
      {:else}
        <span class="dot"></span>
        <div class="wtext"><div class="wname muted">正在准备</div></div>
      {/if}

      <!-- 共享 Minecraft 之外的东西的出路,就站在它要改写的那句话旁边:
           点开一个小弹层输端口;设定之后同一个位置变成撤销它的按钮。 -->
      <div class="wtool" bind:this={shareTool}>
        {#if session.sharedPort}
          <button class="ghost small" onclick={() => session.sharePort(null)}>改回自动</button>
        {:else}
          <button class="ghost small" onclick={() => (sharing ? closeShare() : (sharing = true))}>
            指定端口
          </button>
          {#if sharing}
            <form class="popover" onsubmit={submitShare}>
              <div class="sharer">
                <input
                  bind:value={portInput}
                  use:autofocus
                  placeholder="端口，如 25565"
                  inputmode="numeric"
                  spellcheck="false"
                  autocomplete="off"
                />
                <button type="submit" disabled={!portValid}>共享</button>
              </div>
              <p class="muted small">共享 Minecraft 之外的游戏或应用：朋友的连接会转发到这个本机端口。</p>
            </form>
          {/if}
        {/if}
      </div>
    </div>

    <section class="members">
      <div class="rail">
        <span class="rlabel">成员{session.peers.length ? ` · ${session.peers.length}` : ''}</span>
        <button class="ghost small" onclick={() => session.leave()}>关闭房间</button>
      </div>
      {#if session.peers.length === 0}
        <p class="empty">还没有成员加入。把邀请码分享给朋友，他们会出现在这里。</p>
      {:else}
        <div class="peers">
          {#each session.peers as peer (peer.id)}
            <PeerCard {peer} carrierName={peer.via ? nameOf(peer.via) : undefined} />
          {/each}
        </div>
      {/if}
    </section>
  {:else}
    <header class="head">
      <!-- 访客也要看得到自己在哪个房间：码在 eyebrow 里陪着状态。大元素留给
           状态词——访客此刻关心的是「我能进游戏了吗」，不是房间叫什么。 -->
      <p class="eyebrow">已加入房间{session.code ? ` · ${session.code}` : ''}</p>
      {#if session.localPort}
        <h1>准备就绪</h1>
        <p class="lede">
          进入多人游戏，{session.lanName
            ? `在列表中找到「${session.lanName}」即可开始`
            : '房间会自动出现在列表中'}。
        </p>
        <div class="beneath">
          <button class="ghost" onclick={() => copy(`127.0.0.1:${session.localPort}`, 'addr')}>
            {copied === 'addr' ? '已复制' : `复制地址 127.0.0.1:${session.localPort}`}
          </button>
        </div>
        <!-- 列表是自动的，但多播在某些机器上被挡掉，这条是那时候的出路。 -->
        <p class="muted small">列表中没有显示？使用「直接连接」输入上方地址。</p>
      {:else}
        <h1 class="muted">正在连接</h1>
        <p class="lede">正在与房主建立直接连接。</p>
      {/if}
    </header>

    <section class="members">
      <div class="rail">
        <span class="rlabel">连接</span>
        <button class="ghost small" onclick={() => session.leave()}>离开房间</button>
      </div>
      <div class="peers">
        {#each session.peers as peer (peer.id)}
          <PeerCard {peer} carrierName={peer.via ? nameOf(peer.via) : undefined} />
        {/each}
      </div>
    </section>
  {/if}

  <div class="notices">
    {#if !session.signalOnline && !session.ended}
      <!-- 信令断了不等于连接断了：已经连上的人还在，只是没人能再加入。
           这条区别值得说清楚，否则玩家会以为一切都完了。会话结束后这句就
           不成立了——那时没有什么「不受影响」。 -->
      <p class="notice">房间服务暂时不可用 · 已建立的连接不受影响</p>
    {/if}
    <!-- 结束原因已经写在整屏里,不再重复弹一条。 -->
    {#if session.error && !session.ended}
      <p class="notice">{session.error}</p>
    {/if}
  </div>
</section>

<style>
  /*
   * 一根竖列，居中、有上限。默认窗口下上限不生效，列就是窗口；窗口被拉大时
   * 列停在能读的宽度，多出来的部分留给背景——画框变宽，画不变形。
   */
  .scene {
    position: relative;
    z-index: 10;
    height: 100%;
    width: 100%;
    max-width: 560px;
    margin-inline: auto;
    padding: var(--top) var(--pad-x) var(--s7);
    display: flex;
    flex-direction: column;
  }

  /* 空态是正门，居中站稳；会话状态是工作屏，从顶上开始排。 */
  .scene.centered {
    justify-content: center;
    padding-bottom: var(--top);
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

  .lede {
    margin: var(--s4) 0 0;
    color: var(--ink-2);
    font-size: var(--t-lead);
  }

  .greet {
    margin: 0;
    font-size: clamp(28px, 8vw, 34px);
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
  }

  .greet-sub {
    margin: var(--s3) 0 0;
    color: var(--ink-2);
    font-size: var(--t-lead);
  }

  .doors {
    margin-top: var(--s7);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }

  .primary {
    padding: var(--s4) var(--s6);
    border-radius: var(--r2);
    /* 群系色可深可浅，纯色块在暗色板下会读成「禁用」。一道顶光、一线内高光
       和一圈同色的弥散光让它在任何色板下都是一个实体，而不是一块颜色。 */
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.2), rgba(255, 255, 255, 0) 58%),
      var(--c4);
    color: var(--on-accent);
    font-size: var(--t-h3);
    font-weight: 500;
    letter-spacing: 0.04em;
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.28),
      0 10px 30px -8px var(--accent-glow),
      0 3px 10px rgba(0, 0, 0, 0.18);
    transition:
      transform var(--spring),
      filter var(--soft);
  }

  .primary:hover {
    filter: brightness(1.06);
  }

  .primary:active {
    transform: scale(0.99);
  }

  .ghost {
    padding: var(--s2) var(--s4);
    border-radius: var(--r1);
    background: var(--glass);
    color: var(--ink-2);
    font-size: var(--t-body);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.06),
      var(--shadow-1);
    transition:
      color var(--soft),
      background var(--soft);
  }

  .ghost:hover:not(:disabled) {
    color: var(--ink);
    background: linear-gradient(rgba(255, 255, 255, 0.07), rgba(255, 255, 255, 0.07)), var(--glass);
  }

  .ghost.small {
    padding: var(--s1) var(--s3);
    font-size: var(--t-cap);
  }

  /* 输入框和按钮共用一只胶囊：加入是一件事，不是一个字段加一个按钮。 */
  .joiner {
    display: flex;
    align-items: stretch;
    border-radius: var(--r2);
    background: var(--glass);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.06),
      var(--shadow-1);
    overflow: hidden;
    transition: box-shadow var(--soft);
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
      color var(--soft),
      background var(--soft);
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

  .code.placeholder {
    color: var(--ink-3);
    letter-spacing: 0.02em;
  }

  .beneath {
    margin-top: var(--s5);
    display: flex;
    gap: var(--s2);
    flex-wrap: wrap;
  }

  .beneath + .small {
    margin-top: var(--s3);
  }

  /* 段落之间用发丝线分隔：状态头、世界、成员，一节一节读下来。 */
  .world {
    position: relative;
    margin-top: var(--s6);
    padding-top: var(--s5);
    border-top: 1px solid var(--line-2);
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
  }

  .wtext {
    min-width: 0;
    flex: 1;
  }

  .wname {
    font-size: var(--t-lead);
  }

  .dot {
    width: 7px;
    height: 7px;
    margin-top: 9px;
    border-radius: 999px;
    background: var(--ink-3);
    flex: none;
  }

  .dot.on {
    background: var(--c4);
  }

  .dot.wait {
    background: var(--c3);
    animation: pulse 1.8s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.3;
    }
    50% {
      opacity: 1;
    }
  }

  /* 站在世界状态那一行的右肩上,和标题的首行对齐。 */
  .wtool {
    margin-left: auto;
    flex: none;
    position: relative;
  }

  /* 弹层挂在按钮下方右对齐,压过下面的成员区。玻璃比卡片那层更实一档——
     它浮在内容上,得自己站得住。 */
  .popover {
    position: absolute;
    top: calc(100% + var(--s2));
    right: 0;
    z-index: 20;
    width: min(280px, 78vw);
    padding: var(--s3);
    border-radius: var(--r2);
    background: var(--glass-2);
    backdrop-filter: blur(24px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.07),
      var(--shadow-2);
  }

  .popover > .small {
    margin: var(--s2) var(--s1) 0;
  }

  /* 和首屏的 joiner 同族：输入和动作共用一只胶囊，只是小一号。 */
  .sharer {
    display: flex;
    align-items: stretch;
    border-radius: var(--r1);
    /* 弹层自己已经是玻璃了,胶囊在它里面用一层实的提亮就够。 */
    background: rgba(255, 255, 255, 0.05);
    box-shadow: inset 0 0 0 1px var(--line-2);
    overflow: hidden;
    transition: box-shadow var(--soft);
  }

  .sharer:focus-within {
    box-shadow: inset 0 0 0 1.5px var(--c3);
  }

  .sharer input {
    flex: 1;
    min-width: 0;
    padding: var(--s2) var(--s3);
    color: var(--ink);
    font-size: var(--t-body);
  }

  .sharer input::placeholder {
    color: var(--ink-3);
  }

  .sharer button {
    padding: var(--s2) var(--s4);
    color: var(--ink-2);
    border-left: 1px solid var(--line-2);
    font-size: var(--t-body);
    transition:
      color var(--soft),
      background var(--soft);
  }

  .sharer button:not(:disabled):hover {
    color: var(--ink);
    background: rgba(255, 255, 255, 0.06);
  }

  .sharer button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .muted {
    color: var(--ink-3);
  }

  .small {
    font-size: var(--t-cap);
  }

  .members {
    margin-top: var(--s6);
    padding-top: var(--s5);
    border-top: 1px solid var(--line-2);
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .rail {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--s4);
  }

  .rlabel {
    font-size: var(--t-cap);
    letter-spacing: 0.24em;
    text-transform: uppercase;
    color: var(--ink-3);
  }

  /* 人多时这一栏自己滚，别把整个场景撑出窗口——窗口本身不滚。 */
  .peers {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--line) transparent;
  }

  .empty {
    margin: 0;
    padding: var(--s5);
    border-radius: var(--r2);
    background: var(--glass);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.05),
      var(--shadow-1);
    color: var(--ink-3);
    font-size: var(--t-body);
  }

  .notices {
    position: absolute;
    left: var(--pad-x);
    right: var(--pad-x);
    bottom: var(--s5);
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--s2);
    pointer-events: none;
  }

  .notice {
    margin: 0;
    padding: var(--s2) var(--s4);
    border-radius: 999px;
    background: var(--glass-2);
    backdrop-filter: blur(18px);
    box-shadow: var(--shadow-1);
    color: var(--ink-2);
    font-size: var(--t-cap);
    letter-spacing: 0.06em;
  }
</style>
