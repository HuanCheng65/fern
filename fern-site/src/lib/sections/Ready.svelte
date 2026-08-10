<script>
  import { onMount } from 'svelte';
  import Cover from 'fern-kit/ui/Cover.svelte';
  import PalettePanel from 'fern-kit/parts/PalettePanel.svelte';
  import SnapshotList from 'fern-kit/parts/SnapshotList.svelte';
  import PeerCard from 'fern-kit/parts/PeerCard.svelte';
  import { reveal } from '$lib/scroll.js';

  // 图示的排布是手摆的：四张卡片沿一道弧往下铺，越靠前压得越低。
  const INSTANCES = [
    { name: '主世界', meta: '1.20.1 · Fabric', hours: 320, hour: 9, x: -58, r: -5 },
    { name: '建筑档', meta: '1.21.4 · NeoForge', hours: 610, hour: 17, x: -20, r: -1.8 },
    { name: '极限生存', meta: '1.16.5 · Forge', hours: 40, hour: 21, x: 22, r: 1.8 },
    { name: '光影档', meta: '1.21.1 · Fabric', hours: 150, hour: 5, x: 60, r: 5 }
  ];

  // 图标是各项目自己的图标，本地留一份，不从 Modrinth 的 CDN 拉。
  const CONTENT = [
    { name: 'Sodium', kind: '模组', icon: 'sodium.webp', y: 12, r: -3 },
    { name: 'Fabulously Optimized', kind: '整合包', icon: 'fabulously-optimized.webp', y: 0, r: -1 },
    { name: 'Faithful 32×', kind: '资源包', icon: 'faithful-32x.png', y: 0, r: 1 },
    { name: 'BSL Shaders', kind: '光影', icon: 'bsl-shaders.webp', y: 12, r: 3 }
  ];

  /*
   * 输入 1165 找到 1.16.5——文案里那个例子，直接画出来。
   *
   * 这块板子不是仿的：`PalettePanel` 就是产品里的那一块，从 fern-kit 引进来，
   * 吃的也是它自己的 Row 结构。只有数据是站上编的——`at` 是标题上的命中位置，
   * 子序列 1·1 6 5 落在 1.16.5 的第 0、2、3、5 个字符上。
   */
  const HITS = [
    {
      key: 'v',
      kind: 'subject',
      subject: { type: 'place', id: 'v', title: '1.16.5', hint: '版本' },
      at: [0, 2, 3, 5]
    },
    {
      key: 'i',
      kind: 'subject',
      subject: { type: 'instance', id: 'i', title: '极限生存', hint: '1.16.5 · Forge' },
      at: []
    }
  ];

  /* 快照的时刻是相对「今天」算的，只能在浏览器里算——预渲染时的今天不是访客的
     今天。同 Backup 那一节。 */
  let snaps = $state([]);
  onMount(() => {
    const now = Math.floor(Date.now() / 1000);
    const hoursAgo = (h) => now - Math.round(h * 3600);
    snaps = [
      { id: '1', takenAt: hoursAgo(1.5), title: '改动模组之前', meta: '412 MB' },
      { id: '2', takenAt: hoursAgo(4), title: '游戏结束之后', meta: '408 MB' },
      { id: '3', takenAt: hoursAgo(26), title: '打红石之前', pinned: true, meta: '396 MB' },
      { id: '4', takenAt: hoursAgo(29), title: '游戏结束之后', meta: '394 MB' }
    ];
  });

  /* 联机那一格也是真卡片。这里不演连接过程——那是 Pearl 那一节的事，这一格只说
     「朋友在你的房间里」。 */
  const PEERS = [
    { id: 'f7a31c9e42', name: '小满', state: 'lan', rttMs: 3 },
    { id: 'b28d1f4a6c', name: '阿哲', state: 'punched', rttMs: 24 },
    { id: '5e9c07b3da', name: 'Nyx', state: 'via', rttMs: 61, via: 'f7a31c9e42' }
  ];
</script>

<section id="ready">
  <div class="wrap">
    <div class="head" use:reveal>
      <h2>一切就位，准备开玩。</h2>
      <p class="lede">
        从游戏版本、实例与账户，到模组、存档和服务器，Fern 把 Minecraft 日常需要的一切整理在一起。
      </p>
    </div>

    <div class="bento" id="bento">
      <!-- 实例：四张真封面沿弧铺开，压着下沿出画 -->
      <article class="tile big" use:reveal>
        <header>
          <span class="label mono">实例</span>
          <h3>各有天地。</h3>
          <p>
            不同版本、加载器和模组组合，都可以拥有自己的空间。<br />创建、复制、整理，也都清清楚楚。
          </p>
        </header>
        <div class="art" aria-hidden="true">
          <div class="fan">
            {#each INSTANCES as it, i}
              <div class="card" style="--i:{i}; --x:{it.x}px; --r:{it.r}deg">
                <div class="cap">
                  <span class="n">{it.name}</span>
                  <span class="m mono">{it.meta}</span>
                </div>
                <Cover seed={it.name} hours={it.hours} hour={it.hour} w={331} h={176} quality={0.6} />
              </div>
            {/each}
          </div>
        </div>
      </article>

      <!-- 内容 -->
      <article class="tile wide" use:reveal={{ delay: 60 }}>
        <header>
          <span class="label mono">内容</span>
          <h3>想加什么，随时加。</h3>
          <p>
            模组、整合包、资源包与光影，都可以直接发现、安装和管理。版本与依赖，Fern 会一起处理。
          </p>
        </header>
        <div class="art" aria-hidden="true">
          <div class="shelf">
            {#each CONTENT as c}
              <div class="item" style="--y:{c.y}px; --r:{c.r}deg">
                <div class="cap">
                  <span class="n">{c.name}</span>
                  <span class="k mono">{c.kind}</span>
                </div>
                <img src="/content/{c.icon}" alt="" width="56" height="56" loading="lazy" />
              </div>
            {/each}
          </div>
        </div>
      </article>

      <!-- 直达 -->
      <article class="tile wide" use:reveal={{ delay: 120 }}>
        <header>
          <span class="label mono">直达</span>
          <h3>所想，即达。</h3>
          <p>
            实例、存档、服务器、设置与内容，都可以从同一个入口找到。名称、拼音、首字母、版本号，都能直接搜索。
          </p>
        </header>
        <div class="art" aria-hidden="true">
          <!-- 只露左上角：一块 600px 的板子压在格子里，右边和下边都出画 -->
          <div class="peek fern fern-dark">
            <PalettePanel query="1165" rows={HITS} cursor={0} still />
          </div>
        </div>
      </article>

      <!-- 备份 -->
      <article class="tile wide" use:reveal={{ delay: 180 }}>
        <header>
          <span class="label mono">备份</span>
          <h3>放心改。</h3>
          <p>实例与世界都可以留下备份和快照。<br />更新、换模组、改配置，都更从容。</p>
        </header>
        <div class="art" aria-hidden="true">
          <!-- 一屉名单从下沿推上来。不转角度也不悬空：名单是往回走的，
               所以它贴着底边往下延伸，越往下越旧。 -->
          <div class="drawer fern fern-dark">
            <SnapshotList rows={snaps} />
          </div>
        </div>
      </article>

      <!-- 联机 -->
      <!--
        整格是深的，而且没有板子——卡片直接落在格子上。
        这一格里的东西本来就是给深色画的，与其给它铺一块地面，不如让这一格自己
        就是地面：一片白格子里有一格是深的，格阵才有轻重。
      -->
      <article class="tile wide night fern fern-dark" use:reveal={{ delay: 240 }}>
        <header>
          <span class="label mono">联机</span>
          <h3>远一点，也像在身边。</h3>
          <p>创建房间，分享邀请码。<br />朋友在远方，也能加入你的局域网世界。</p>
          <span class="code mono">481 502</span>
        </header>
        <div class="art" aria-hidden="true">
          <div class="crowd">
            {#each PEERS as peer, i (peer.id)}
              <div class="one" style="--i:{i}">
                <PeerCard
                  {peer}
                  carrierName={peer.via ? PEERS.find((x) => x.id === peer.via)?.name : undefined}
                />
              </div>
            {/each}
          </div>
        </div>
      </article>
    </div>

    <div class="tags mono" use:reveal>
      <span>Minecraft · Fabric · Forge · NeoForge · Quilt · Modrinth</span>
      <span>Windows · macOS · Linux</span>
    </div>
  </div>
</section>

<style>
  .head h2 {
    max-width: 15ch;
  }
  .head .lede {
    margin-top: 30px;
    max-width: 46ch;
  }

  /* ---------- Bento ----------
     四列三行：实例占左边 2×2，内容和直达叠在右边，备份与联机分掉最后一行。 */

  .bento {
    margin-top: clamp(52px, 7vw, 96px);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    /* 前两行给实例那一叠。第三行原来矮一截——那时它放的是两条画出来的示意；
       现在那两格里是真的名单和真的卡片，得给它们站得下的高度。 */
    grid-template-rows: repeat(2, 344px) 384px;
    gap: 14px;
  }
  .big {
    grid-column: span 2;
    grid-row: span 2;
  }
  .wide {
    grid-column: span 2;
  }

  .tile {
    position: relative;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    padding: 26px 26px 0;
    border-radius: 20px;
    background: #fff;
    border: 1px solid var(--line);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.05),
      0 14px 38px rgba(20, 32, 26, 0.05);
  }

  .label {
    font-size: 11px;
    letter-spacing: 0.18em;
    color: var(--mut);
  }
  .tile h3 {
    margin-top: 10px;
    font-size: 22px;
  }
  .big h3 {
    font-size: 28px;
  }
  .tile p {
    margin-top: 12px;
    font-size: 14px;
    line-height: 1.85;
    color: var(--mut);
    max-width: 42ch;
  }

  header {
    flex: none;
  }
  /*
    图示铺满整格宽度（抵掉左右内边距），越出的部分交给这里裁。
    在这里裁而不是在格子上裁：往上溢出会压到文字，往下溢出才是想要的出画。
  */
  .art {
    position: relative;
    flex: 1;
    min-height: 0;
    margin: 20px -26px 0;
    overflow: hidden;
  }

  /* 通用卡面 */
  .card,
  .item {
    border-radius: 13px;
    overflow: hidden;
    background: #fff;
    border: 1px solid var(--line);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.06),
      0 16px 34px rgba(20, 32, 26, 0.12);
  }
  .cap {
    padding: 11px 14px;
  }
  .cap .n {
    display: block;
    font-size: 13px;
    font-weight: 600;
  }
  .cap .m,
  .cap .k {
    display: block;
    margin-top: 3px;
    font-size: 10px;
    letter-spacing: 0.08em;
    color: var(--mut);
  }

  /* 实例：一叠往下铺开的封面 */
  .fan {
    position: absolute;
    left: 50%;
    bottom: -30px;
    width: 60%;
    translate: -50% 0;
  }
  .fan .card {
    position: absolute;
    inset: auto 0 0;
    /* 往下铺，所以越低的越靠前。 */
    z-index: calc(3 - var(--i));
    transform-origin: 50% 100%;
    translate: var(--x) calc(var(--i) * -82px);
    rotate: var(--r);
  }
  .fan :global(canvas) {
    width: 100% !important;
    height: 176px !important;
    border-radius: 0;
  }

  /* 内容：四张摆开的内容卡 */
  .shelf {
    position: absolute;
    /* 四张卡的底一律裁掉：卡名在上，图标压出下沿。 */
    inset: auto 0 -36px;
    display: flex;
    justify-content: center;
    gap: 14px;
    padding: 0 12px;
  }
  .shelf .item {
    flex: 1 1 0;
    min-width: 0;
    max-width: 132px;
    display: flex;
    flex-direction: column;
    padding: 14px 13px 16px;
    translate: 0 var(--y);
    rotate: var(--r);
  }
  .shelf img {
    width: 52px;
    height: 52px;
    border-radius: 11px;
    /* 别人的图标一律原样呈现，不裁切也不加滤镜。 */
    object-fit: cover;
    background: var(--paper);
  }
  .shelf .cap {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    /* 卡名可以折两行，但卡头等高——四张卡的图要在同一条线上。 */
    height: 62px;
    padding: 10px 11px;
  }
  .shelf .n {
    font-size: 11.5px;
    line-height: 1.35;
  }

  /*
   * 直达：真的那块板子，只露左上角。
   *
   * 板子按它自己的宽度（产品里就是 600px）画，右边和下边压出格子外——露一角
   * 比塞一个缩小版更像那么回事，也不必为了塞进来改它任何一个尺寸。
   * `.fern` 圈住设计系统的作用域、`.fern-dark` 指定用哪张表面：变量只在这一
   * 小块里生效，纸白的页面碰不到。
   */
  .peek {
    position: absolute;
    left: 8%;
    top: 10px;
    width: 600px;
    border-radius: var(--r3, 16px);
    background: var(--panel);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      0 30px 70px rgba(20, 32, 26, 0.28);
    rotate: -1.2deg;
    overflow: hidden;
  }

  /* 备份：一屉名单从下沿推上来。贴边、不转角度、只往下出画——和上一格那块
     悬着的斜板子是两个动作，不然三格连着看就是同一张模板换了内容。 */
  .drawer {
    position: absolute;
    left: 26px;
    right: -34px;
    top: 0;
    /* 往下出画，不往上：贴着下沿裁的话，被切掉的是最上面那个「今天」，
       而那正是这份名单最该先说的一句。 */
    bottom: -48px;
    padding: 16px 20px 0;
    border-radius: 14px 14px 0 0;
    background: var(--panel);
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      0 -18px 44px rgba(20, 32, 26, 0.18);
  }

  /* 联机：这一格自己就是深色地面。 */
  .night {
    background: var(--pine);
    border-color: transparent;
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.12),
      0 18px 44px rgba(20, 32, 26, 0.16);
  }
  .night h3 {
    color: var(--paper);
  }
  .night p {
    color: var(--on-dark-mut);
  }
  .night .label {
    color: rgba(246, 244, 236, 0.42);
  }
  .code {
    display: inline-block;
    margin-top: 14px;
    color: var(--sprout);
    font-size: 15px;
    letter-spacing: 0.14em;
  }

  /* 卡片横向错开着往右下走，右边出画。没有板子，所以错位就是构图本身。 */
  .crowd {
    position: absolute;
    left: 26px;
    right: -70px;
    top: 4px;
    display: grid;
    gap: 10px;
  }
  .one {
    margin-left: calc(var(--i) * 34px);
  }
  /* 卡片是要压出格子外的，而毛玻璃在裁切边界之外照样采样——采到的是格子外面的
     纸白，于是卡片右边浮起一条亮带。这一格的底本来就是不透明的松绿，这层模糊
     在这里没有可模糊的东西。 */
  .crowd :global(.card) {
    backdrop-filter: none;
  }

  .tags {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: clamp(48px, 6vw, 84px);
    padding-top: clamp(34px, 4vw, 54px);
    border-top: 1px solid var(--line);
    font-size: 12px;
    letter-spacing: 0.1em;
    color: var(--mut);
  }

  @media (max-width: 980px) {
    .bento {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      /* 让轨道跟着长：实例格靠 min-height 撑高，固定轨道会让它压到下一行上。 */
      grid-auto-rows: minmax(330px, auto);
    }
    .big,
    .wide {
      grid-column: span 2;
    }
    .big {
      grid-row: span 1;
      min-height: 580px;
    }
    .fan {
      width: 54%;
    }
    /* 格子矮了，那叠也铺得紧一点。 */
    .fan .card {
      translate: var(--x) calc(var(--i) * -60px);
    }
    .fan :global(canvas) {
      height: 164px !important;
    }
    .shelf {
      inset: auto 0 -20px;
    }
    .shelf .item {
      padding: 12px 11px 13px;
    }
    .shelf img {
      width: 44px;
      height: 44px;
    }
  }
  @media (max-width: 640px) {
    .bento {
      grid-template-columns: 1fr;
    }
    .big,
    .wide {
      grid-column: span 1;
    }
    .fan {
      width: 70%;
    }
    /* 窄了就收敛弧度，否则最前那张会被切掉半个名字。 */
    .fan .card {
      translate: calc(var(--x) * 0.35) calc(var(--i) * -60px);
    }
    /* 一列的时候标题占三行，最上面那张放不下，就不铺它。 */
    .fan .card:last-child {
      display: none;
    }
    .shelf .item {
      max-width: none;
    }
  }
</style>
