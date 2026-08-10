<script>
  import { untrack } from 'svelte';
  import Cover from 'fern-kit/ui/Cover.svelte';
  import Island from 'fern-kit/parts/Island.svelte';
  import TopBar from 'fern-kit/parts/TopBar.svelte';
  import LaunchHero from 'fern-kit/parts/LaunchHero.svelte';
  import { PRIORITY } from 'fern-kit/parts/island';
  import { biomeName, mixAccent, mixVars, paletteVars, salutationAt } from '$lib/palette.js';
  import { track } from '$lib/scroll.js';

  /*
   * 一块钉住的屏幕，整章的舞台。三幕连着演，中间一次都不松手。
   *
   * 第一幕：屏幕滚上来、钉住、亮起——亮起来的只有背景，那是启动那一屏真正的底：实例
   * 的群系封面本身。再往下，界面的各个部分从页面的四面八方汇进来，落到它们在启动界
   * 面里的**原位**：顶栏在上、招呼行和实例名压在左下、启动键在它下面。
   *
   * 第二幕：屏幕不松开，里面的实例一个接一个换过去。换的不只是名字——封面、色板、
   * 连那句问候都跟着换，因为它们本来就是同一个东西推出来的。
   *
   * 第三幕：界面一件件退场，只留下顶栏右边那座岛。它在原地展开成面板——那是产品里
   * 真会发生的事——然后被从窗口里**拎出来**，放大着走到正中；窗口本身淡掉。接着别的
   * 岛从四面八方汇进来，错落地落在纸上。这就是「解构 UI」：真的界面元素离开了窗口，
   * 在网页的空间里重新构图。
   *
   * 屏幕里装的是 `TopBar`、`LaunchHero`、`Island`——产品里那几样本身，不是仿的。
   * 汇入和拎出是网页的叙事，不是 Fern 里的转场。
   */
  let p = $state(0);

  /*
   * 时间轴用「轨道走过了多少 vh」来写，不用 0–1 的比例——比例一改轨道长度就全乱，
   * 而每一段该占多长的路是有手感的，得能直接看见。
   *
   * track 交出的是 (视口高 + 元素高) 这段路上的进度，所以分母是 RAIL + 100。
   * 屏幕在 t≈93 钉住，在 t≈RAIL 松开：所有事都得发生在这中间。
   */
  const RAIL = 1620;
  const TRAVEL = RAIL + 100;

  const clamp = (v) => Math.max(0, Math.min(1, v));
  const ease = (v) => 1 - Math.pow(1 - v, 3);
  const smooth = (v) => v * v * (3 - 2 * v);

  const seg = (from, span) => clamp((p * TRAVEL - from) / span);
  const at = (from, span) => ease(seg(from, span));

  const SCENES = [
    { id: 'launch', label: '启动' },
    { id: 'instances', label: '实例' },
    { id: 'supply', label: '补给' },
    { id: 'multiplayer', label: '联机' },
    { id: 'wardrobe', label: '衣柜' }
  ];

  /* 顶栏那座岛说的话。第三幕它会展开，所以行和动作从一开始就备齐。 */
  const PRESENCE = {
    id: 'job',
    priority: PRIORITY.work,
    tone: 'work',
    label: '安装 Sodium',
    fraction: 0.62,
    rows: [
      {
        id: 'r1',
        label: '安装 Sodium',
        detail: '正在写入模组目录',
        meta: '18.4 MB / 29.6 MB · 6.1 MB/s',
        fraction: 0.62
      }
    ],
    actions: [{ label: '查看详情', run: () => {} }]
  };

  /* 同一个人的四个实例。钟点各不相同，所以四张封面的色温、四句问候也各不相同。 */
  const INSTANCES = [
    { name: '主世界', hours: 320, hour: 9, detail: 'Minecraft 1.20.1 · Fabric' },
    { name: '机械动力', hours: 260, hour: 18, detail: 'Minecraft 1.20.1 · Fabric · 96 个模组' },
    { name: '原版生存', hours: 420, hour: 13, detail: 'Minecraft 1.21.4 · 原版' },
    { name: '宝可梦', hours: 88, hour: 22, detail: 'Minecraft 1.21.1 · Fabric · 132 个模组' }
  ];

  const ME = { name: '小满', face: { url: '/skins/steve.png', hat: false } };

  /* ---- 第一幕 ---- */

  /* 屏幕到位：钉住之前先把它送上来。 */
  const arrive = $derived(at(10, 74));
  /*
   * 亮起：只有背景。
   *
   * 在屏幕还没站稳的时候就开始亮——一块屏先自己走完，停住，再亮，是两件事排队；
   * 叠着来才是一件事：它一边到位一边醒过来。
   */
  const lit = $derived(at(42, 86));
  /*
   * 汇入。**每一块从它自己那个角落来**：顶栏在上，就从上面来；启动那一组压在
   * 左下，就从左下来。错开一点起步，四个方向同时到齐会像一次整体位移，而不是
   * 各自从各自的地方回来。
   */
  const fly = (i) => 1 - at(150 + i * 26, 88);
  /* 文字最后出现，等界面站定。 */
  const words = $derived(at(300, 62));

  /* ---- 第二幕 ---- */

  const CYCLE = 420;
  const STEP = 118;

  /*
   * 游标 0→3，一路走过四个实例。每一段前后各留三成平的：不留的话屏幕永远在变，
   * 没有一刻是「这就是这个实例的样子」，而那才是这一幕要给人看的东西。换的那一段
   * 反过来要短——中间那一下两张封面各透一半，谁都不成立，久了就是一片糊。
   */
  const cursor = $derived.by(() => {
    const raw = seg(CYCLE, STEP * 3) * 3;
    const i = Math.min(2, Math.floor(raw));
    return i + smooth(clamp((raw - i - 0.3) / 0.4));
  });
  const from = $derived(Math.min(2, Math.floor(cursor)));
  const mix = $derived(cursor - from);

  /*
   * 封面一层压一层，下面那张一直是实心的，只有上面那张在化开。两张各按自己的
   * 不透明度对半开的话，中间那一下会连黑底一起透出来，整块屏会先暗一截。
   */
  const cover = (k) => (k === 0 ? 1 : clamp(cursor - k + 1));
  /*
   * 字不能这么叠——字是透的，两个名字同时在场就是糊成一团。所以让它交叉淡出，
   * 而且比封面快得多：中间那一小段两边都近乎不在，读起来是换了一次，不是化开。
   */
  const hero = (k) => clamp(1 - Math.abs(cursor - k) * 1.9);

  /* 换实例时那一行小字直接换掉，趁着谁都看不见的那一下。 */
  const near = $derived(Math.round(cursor));
  const shown = $derived(INSTANCES[near]);
  const cycling = $derived(
    at(438, 42) * (1 - at(800, 40)) * (1 - clamp(Math.abs(cursor - near) * 2))
  );

  /* ---- 第三幕 ---- */

  /*
   * 岛到这里才出现。
   *
   * 前两幕它不该在：那两幕说的是界面和内容，顶栏右边空着才是 Fern 大多数时候的样子
   * ——「零状态零挂件」。它一直挂在 DOM 里（不然设置键会横跳，而且量不到它的位置），
   * 只是没有形。
   */
  const emerge = $derived(at(880, 70));
  /* 除了岛，界面上的东西一件件退场；窗口自己也淡掉。 */
  const shed = $derived(at(955, 85));
  /*
   * 岛在原地展开成面板。这是产品里真会发生的事，所以放在被拎走**之前**。
   *
   * 展到几成由滚到哪儿说了算，不是过了某个位置就放一段动画：滚动里进度就是位置，
   * 手停在半路，岛就停在半路；往回滚，它就收回去。所以这里给的是 0–1，不是开关。
   */
  const unfold = $derived(smooth(seg(1050, 110)));
  /*
   * 拎出来：走到正中，同时放大。
   *
   * 不等展开走完就起步——展开到一半的岛已经在往中间去了，读起来是「它被拿了出来」
   * 这一个动作，而不是「先开，然后搬」两拍。
   */
  const lift = $derived(at(1105, 150));
  /* 别的岛从四周汇进来，错开起步。 */
  const land = (i) => at(1290 + i * 32, 125);
  /* 岛都落定了字才出来。 */
  const words3 = $derived(at(1450, 70));

  /* 放大到多少。真界面里它就这么大，放大只是为了让它在整页里站得住。 */
  const BIG = 1.3;


  /*
   * 岛在屏幕里的原位，以及它展开之后会有多大。量一次就够，视口变了再量。
   *
   * 不去等「没有变换的那一刻」——**把变换除掉**。岛身上那点位移和缩放是这里自己
   * 给的，每一帧都知道是多少，量到的减掉它就是本来的样子。等窗口的话，谁直接跳到
   * 轨道中段（刷新后浏览器恢复滚动位置、点锚点、分享一个位置进来）就永远错过它，
   * 岛会留在顶栏里只是变大——而且展开和位移一叠，那个窗口本来就快没了。
   *
   * 落点用表面的**右上角**：无论收着还是展开，那个角都不动（表面钉在胶囊的右上角
   * 向左下方长，见 Island.svelte），`transform-origin` 也定在这个角，所以缩放不影响
   * 它，只需减掉位移。终态尺寸从面板量：面板一直是按终态排好的 320px，和表面此刻
   * 多宽无关——所以不必等它展开完，这正是展开和位移能叠着演的前提。
   */
  let screenEl = $state();
  let capsEl = $state();
  let sw = $state(0);
  let sh = $state(0);
  let seat = $state(null);
  /*
   * 整组要往上让多少。
   *
   * 前两幕窗口还在，那块框就是眼睛用的框，居中于它是对的。第三幕窗口淡掉了，眼睛
   * 换了一个框：**网页顶栏的下沿，到底下那行字的上沿**。而那行字还占着原来的位置，
   * 于是同一个几何居中读起来就偏下。
   *
   * 差多少不能写死。钉住那一格是「屏幕 + 空隙 + 那行字」整组一起居中的，视口越高，
   * 组的上下各留的余量越多——而新的框只含上面那一截，不含下面那一截，所以偏差随
   * 视口高度一路长大。量出来才准。
   */
  let rise = $state(0);
  let lastSize = '';

  $effect(() => {
    void p;
    void sw;
    void sh;
    untrack(() => {
      const key = `${sw}x${sh}`;
      if (key !== lastSize) {
        lastSize = key;
        seat = null;
      }
      /* 顶栏还在往下飞的时候位置不作数：那一截位移是 .bar 的，不是岛自己的。 */
      if (seat || !screenEl || !capsEl || fly(0) > 0) return;
      const surface = screenEl.querySelector('.bar .surface');
      const panel = screenEl.querySelector('.bar .body');
      if (!surface || !panel) return;
      const box = screenEl.getBoundingClientRect();
      const r = surface.getBoundingClientRect();
      const rp = panel.getBoundingClientRect();
      /* 此刻岛身上的缩放。原点在右上角，所以它只放大尺寸，不挪那个角。 */
      const s = (0.86 + 0.14 * emerge) * grew;
      seat = {
        right: r.right - box.left - tx,
        top: r.top - box.top - ty,
        w: rp.width / s,
        /* 表头没有单独的盒子，用「表面的上沿到面板的下沿」把它一起量进来。 */
        h: (rp.bottom - r.top) / s
      };
      /* 这时钉住早已生效，量到的就是它整幕待着的位置。 */
      const nav = parseFloat(getComputedStyle(screenEl).getPropertyValue('--nav')) || 0;
      rise = Math.max(0, box.top + sh / 2 - (nav + capsEl.getBoundingClientRect().top) / 2);
    });
  });

  const berth = $derived(
    seat && sw && sh
      ? {
          right: sw / 2 + (seat.w * BIG) / 2,
          top: sh / 2 - (seat.h * BIG) / 2 - rise
        }
      : null
  );
  const tx = $derived(berth ? (berth.right - seat.right) * lift : 0);
  const ty = $derived(berth ? (berth.top - seat.top) * lift : 0);
  const grew = $derived(1 + (BIG - 1) * lift);

  /*
   * 落在纸上的那些岛。
   *
   * 状态各不相同：装东西的、跑着的、出事的、有人在一起的；有的收着，有的展开。
   * 色板也各不相同——每座岛带着它那个实例的颜色，因为颜色本来就是从实例推出来的。
   * 位置按屏幕的百分比给，稍微越出边界，不整齐排开：这是构图，不是一张对照表。
   */
  const FIELD = [
    {
      key: 'alert',
      scene: INSTANCES[1],
      x: 16,
      y: 11,
      s: 1,
      fx: -36,
      fy: -24,
      open: false,
      presences: [
        {
          id: 'a',
          priority: PRIORITY.alert,
          tone: 'alert',
          label: 'Sodium 与当前版本不兼容',
          rows: [],
          actions: []
        }
      ]
    },
    {
      key: 'many',
      scene: INSTANCES[3],
      x: 82,
      y: 14,
      s: 0.96,
      fx: 34,
      fy: -26,
      open: false,
      presences: [
        {
          id: 'm1',
          priority: PRIORITY.work,
          tone: 'work',
          label: '下载 Create',
          fraction: 0.34,
          rows: [],
          actions: []
        },
        {
          id: 'm2',
          priority: PRIORITY.work,
          tone: 'work',
          label: '校验资源',
          fraction: 0.71,
          rows: [],
          actions: []
        },
        {
          id: 'm3',
          priority: PRIORITY.alert,
          tone: 'alert',
          label: '一个模组装失败了',
          rows: [],
          actions: []
        }
      ]
    },
    {
      key: 'live',
      scene: INSTANCES[0],
      x: 17,
      y: 73,
      s: 1.05,
      fx: -38,
      fy: 26,
      open: false,
      presences: [
        {
          id: 'l',
          priority: PRIORITY.live,
          tone: 'live',
          label: '主世界 运行中',
          fill: 0.46,
          rows: [],
          actions: []
        }
      ]
    },
    {
      key: 'room',
      scene: INSTANCES[2],
      x: 90,
      y: 68,
      s: 0.94,
      fx: 36,
      fy: 24,
      open: true,
      presences: [
        {
          id: 'r',
          priority: PRIORITY.room,
          tone: 'live',
          label: '和 3 人在一起',
          rows: [
            { id: 'r1', label: '朋友的服务器', detail: '延迟 34 ms', meta: '4 / 8 人' }
          ],
          actions: [{ label: '复制邀请', run: () => {} }]
        }
      ]
    },
    {
      key: 'idle',
      scene: INSTANCES[1],
      x: 33,
      y: 89,
      s: 0.9,
      fx: 0,
      fy: 32,
      open: false,
      presences: [
        { id: 'i', priority: PRIORITY.work, tone: 'work', label: '读取版本信息', rows: [], actions: [] }
      ]
    }
  ];

  /* 到第三幕才把它们挂上：挂着不画也要画五座岛，而且会挡住前两幕的鼠标。 */
  const staged = $derived(p * TRAVEL > 1220);

  /*
   * 底下这一格一次只站一段字：三段的窗口互不重叠——重叠一点，那一格就是两行字
   * 压在一起。
   */
  const caption1 = $derived(words * (1 - at(386, 46)));
  const caption2 = $derived(at(840, 66) * (1 - at(940, 50)));
</script>

<section class="stage-rail" style="height:{RAIL}vh" use:track={{ onprogress: (v) => (p = v) }}>
  <div
    class="pin"
    style="--glow:{mixAccent(INSTANCES[from], INSTANCES[from + 1], mix)};--lit:{lit};--shed:{shed}"
  >
    <div
      bind:this={screenEl}
      bind:clientWidth={sw}
      bind:clientHeight={sh}
      inert
      class="screen fern fern-dark"
      style="{mixVars(INSTANCES[from], INSTANCES[from + 1], mix)};--arrive:{arrive};--lit:{lit};--shed:{shed};--emerge:{emerge};--tx:{tx}px;--ty:{ty}px;--grew:{grew};--rise:{rise}px;--f0:{fly(
        0
      )};--f1:{fly(1)};--f2:{fly(2)};--f3:{fly(3)};--f4:{fly(4)}"
    >
      <!-- 窗口本身：一块深色的板和它的影子。第三幕它先淡掉，岛才好被拎出来。 -->
      <div class="frame"></div>

      <!-- 亮起来的是背景，不是整块屏。 -->
      <div class="light">
        {#each INSTANCES as it, k (it.name)}
          <div class="sky" style="opacity:{cover(k)}">
            <Cover seed={it.name} hours={it.hours} hour={it.hour} quality={0.7} />
          </div>
        {/each}
      </div>

      <!--
        窗口按钮。**这三颗是系统画的，不是 Fern 的组件**——Fern 是无边框窗口，
        macOS 上交给系统那一套，只有 Windows 和 Linux 才自己画（见应用里的
        WindowFrame）。所以这里画的是「Fern 在 macOS 上的样子」，属于窗口，
        不属于界面：它跟着屏幕一起在，不参与后面那场汇入。
      -->
      <div class="traffic" aria-hidden="true">
        <span class="close"></span>
        <span class="min"></span>
        <span class="zoom"></span>
      </div>

      <!-- 真顶栏：标志、五个场景词、岛、设置键。产品里那一条本身。 -->
      <div class="bar">
        <TopBar
          scenes={SCENES}
          scene="launch"
          mac
          presences={[PRESENCE]}
          islandUnfold={unfold}
          updateAvailable
        />
      </div>

      <!--
        四份启动屏叠在原位，同一时刻只有一份看得见。整份换掉、而不是把字改掉，
        是因为换的时候两份要同时在场——一份走一份来，中间那一下谁都不该抢。
      -->
      {#each INSTANCES as it, k (it.name)}
        <div class="hero" style="--o:{hero(k)};--dy:{(cursor - k) * -16}px">
          <LaunchHero
            name={it.name}
            detail={it.detail}
            identity={ME}
            salutation={salutationAt(it.hour)}
          />
        </div>
      {/each}

      <!-- 从四周汇进来的那些岛。落在窗口原来的那片地方，稍微越出去一点。 -->
      {#if staged}
        <div class="field">
          {#each FIELD as it, i (it.key)}
            <div
              class="drift fern fern-dark"
              style="{paletteVars(it.scene)};left:{it.x}%;top:{it.y}%;--s:{it.s};--f:{1 -
                land(i)};--fx:{it.fx}vw;--fy:{it.fy}vh;opacity:{land(i)}"
            >
              <Island presences={it.presences} unfold={it.open ? 1 : 0} />
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="captions" bind:this={capsEl}>
      <p style="opacity:{caption1}">
        Fern 把界面、内容与状态放在一起设计。信息保持清楚，重要的内容始终留在该在的位置。
      </p>

      <!-- 换的到底是哪两个东西，写在这儿：一个世界，一个钟点。颜色是这两样推出来的。 -->
      <p class="biome mono" style="opacity:{cycling}">
        {shown.name} · {biomeName(shown)} · {shown.hour} 时
      </p>

      <p class="headline" style="opacity:{caption2}">不同的世界，自然有不同的样子。</p>

      <p class="headline" style="opacity:{words3}">正在发生的一切，都在同一个地方。</p>
    </div>
  </div>
</section>

<style>
  /* 轨道很高：钉住的那一屏要在这段距离里把三幕演完。高度由脚本给，两边同一个数。 */
  .stage-rail {
    position: relative;
    padding: 0;
  }

  /* 让开固定顶栏：钉在 0 的话，屏幕的上沿会一直压在导航条下面。 */
  .pin {
    /* 屏幕到文字、文字到底边，用同一个数——两处空隙一样宽，这一屏才站得稳。 */
    --gap: clamp(20px, 2.8vh, 32px);

    position: sticky;
    top: var(--nav);
    display: grid;
    /* 两行都按内容高，整组一起居中——屏幕单独占一个 1fr 的话它会在自己那行里
       居中，下面凭空多出一截，屏到字就永远比字到底宽。 */
    grid-template-rows: auto auto;
    align-content: center;
    justify-items: center;
    height: calc(100vh - var(--nav));
    /* 上下不对称：顶上不需要留那么多，而下面要留出文字加一段和它相等的余地。 */
    padding: clamp(12px, 1.8vh, 24px) clamp(20px, 4vw, 48px) var(--gap);
  }

  /*
   * 页面身后的空气也跟着换色。界面向内容学色彩，那么这一页也该学——不然屏幕里换
   * 了一个世界，屏幕外还是刚才那张纸，两边像是没在讲同一件事。
   * 压得很淡：底子是纸白，色一重就脏。
   *
   * 左右**不能**出边：`.pin` 本来就是整幅宽，往外撑一截，整个文档就宽出那一截，
   * 页面右边空出一条。body 上的 overflow-x 拦不住它——滚动的是 html。范围靠渐变
   * 自己的半径给，不靠盒子撑。
   */
  .pin::before {
    content: '';
    position: absolute;
    inset: -10% 0;
    z-index: -1;
    background: radial-gradient(
      72% 46% at 50% 46%,
      rgba(var(--glow), 0.16),
      rgba(var(--glow), 0) 70%
    );
    /* 窗口淡掉之后这层光也该退：纸上剩几座岛，身后不该还留着一团绿。 */
    opacity: calc(var(--lit) * (1 - var(--shed)));
    pointer-events: none;
  }

  /*
   * 屏幕本身只是一个定位框，没有自己的样子——样子在 .frame 上。
   *
   * 分开是为了第三幕：窗口要淡掉，而岛还长在这个框里，得留在场上。两样并在一个
   * 元素上的话，透明度是连坐的，淡掉窗口就等于把岛一起淡掉。
   */
  .screen {
    /* 窗口的度量由宿主给：产品那边是 app.css，这里是这块屏。不给的话顶栏用的是
       kit 里的回落值，设置键会贴到圆角上被切掉。 */
    --top: 48px;
    --pad-x: 22px;
    --frame-controls: 0px;

    position: relative;
    /*
     * 宽度是在和竖向余量做交换：窗口比例定死 16/10，屏幕越窄就越矮，空出来的
     * 竖向余量总得落在某处。1020 是让「上空 ≈ 下空」而屏到字仍然更紧的那个点。
     */
    width: min(100%, 1020px);
    aspect-ratio: 16 / 10;
    /* 下面那段文字是两行，加上它自己的上边距，得先把地方留出来，屏幕再按剩下的
       高度收——否则字要么被挤没，要么顶到视口外面。 */
    max-height: calc(100vh - var(--nav) - 13rem);
    border-radius: clamp(14px, 1.6vw, 22px);
    transform: translateY(calc((1 - var(--arrive)) * 8vh)) scale(calc(0.88 + 0.12 * var(--arrive)));
  }

  .frame {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #0a0d0c;
    box-shadow:
      inset 0 0 0 1px rgba(246, 244, 236, 0.08),
      0 40px 110px rgba(20, 32, 26, calc(0.1 + 0.16 * var(--arrive)));
    opacity: calc((0.25 + 0.75 * var(--arrive)) * (1 - var(--shed)));
  }

  /*
   * 背景自己亮起来。屏是先在的，亮的是里面。
   *
   * 裁切只给它，不给整块屏：汇进来的那些部分要从页面的四面八方飞过来，屏幕要是
   * 裁着，它们就只有进了屏才看得见——那成了「屏里的动画」，不是「从整页汇入」。
   */
  .light {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    overflow: hidden;
    opacity: calc(var(--lit) * (1 - var(--shed)));
  }
  .sky {
    position: absolute;
    inset: 0;
  }
  .sky :global(canvas) {
    width: 100%;
    height: 100%;
    display: block;
  }

  .traffic {
    position: absolute;
    top: clamp(12px, 1.5vw, 18px);
    left: clamp(14px, 1.6vw, 20px);
    z-index: 4;
    display: flex;
    gap: 8px;
    opacity: calc(var(--arrive) * (1 - var(--shed)));
  }
  .traffic span {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.16);
  }
  .close {
    background: #ff5f57;
  }
  .min {
    background: #febc2e;
  }
  .zoom {
    background: #28c840;
  }

  /* 顶栏在窗口顶上，所以它从上面来。 */
  .bar {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 3;
    height: var(--top, 48px);
    opacity: calc(1 - var(--f0));
    transform: translateY(calc(var(--f0) * -26vh));
  }

  /* 第三幕：顶栏上除了岛，其余的先退场。类名是 TopBar 自己的。 */
  .bar :global(.brand),
  .bar :global(nav),
  .bar :global(.right > button) {
    opacity: calc(1 - var(--shed));
  }

  /*
   * 岛被拎出窗口。原点定在右上角——它的表面本来就钉在那个角上向左下方长，原点
   * 定在同一个角，缩放才不会把这个角挪走，位移算出来的落点也才是准的。
   */
  .bar :global(.island) {
    transform-origin: 100% 0;
    /* 浮现的那点放大和后面拎出来的放大乘在一起：同一个元素只能有一条 transform，
       分两处写后写的那条会把前一条整个盖掉。 */
    transform: translate(var(--tx, 0px), var(--ty, 0px))
      scale(calc((0.86 + 0.14 * var(--emerge, 1)) * var(--grew, 1)));
    opacity: var(--emerge, 1);
  }

  /* 内容压在左下角，右边和上边整片留给背景。这不是没排满，是画框的意思。 */
  .hero {
    position: absolute;
    left: clamp(20px, 3.4vw, 52px);
    right: clamp(20px, 3.4vw, 52px);
    bottom: clamp(20px, 3.4vw, 46px);
    z-index: 2;
    opacity: calc(var(--o) * (1 - var(--shed)));
    /* 换实例时整份轻轻往上走一截：来的从下面来，走的从上面走。 */
    transform: translateY(var(--dy, 0px));
  }

  /*
   * 各部分从画外回到原位。类名是 LaunchHero 自己的，Svelte 的作用域样式进不去，
   * 所以挂在自己拥有的 .hero 下面用 :global。
   */
  /* 这一组压在左下，所以四行都从左下方来，只是深浅不同——同一个方向，不同的
     距离，读起来才是「一叠东西归位」而不是四条各飞各的。 */
  .hero :global(.hail) {
    opacity: calc(1 - var(--f1));
    transform: translate(calc(var(--f1) * -34vw), calc(var(--f1) * 12vh));
  }
  .hero :global(.name) {
    opacity: calc(1 - var(--f2));
    transform: translate(calc(var(--f2) * -46vw), calc(var(--f2) * 20vh));
  }
  .hero :global(.meta) {
    opacity: calc(1 - var(--f3));
    transform: translate(calc(var(--f3) * -30vw), calc(var(--f3) * 26vh));
  }
  .hero :global(.go-row) {
    opacity: calc(1 - var(--f4));
    transform: translate(calc(var(--f4) * -22vw), calc(var(--f4) * 34vh));
  }

  /*
   * 落在纸上的那些岛。不裁切：它们本来就该稍微越出窗口原来的边界。
   *
   * 压在被拎出来那座（顶栏那条是 z-index 3）**下面**：重叠是要的，但主角只有一个，
   * 谁压谁不能随 DOM 顺序碰运气。
   */
  .field {
    position: absolute;
    inset: 0;
    z-index: 2;
    /* 和被拎出来那座让开同样多，整组才是一起挪的。 */
    transform: translateY(calc(var(--rise, 0px) * -1));
  }

  /*
   * 纸上要有影子。岛这套表面是照深色背景调的，只有一圈内描边把自己和底分开——落在
   * 纸白上那圈描边什么也分不开，看着像印上去的。展开的那座自带 --shadow-lg，作用域
   * 更深，不受这条影响。
   */
  .drift :global(.surface) {
    box-shadow:
      inset 0 0 0 1px var(--panel-line),
      0 10px 30px rgba(20, 32, 26, 0.16);
  }

  /*
   * 卫星点同理，而且更彻底：它在应用里躺在深色背景上，靠一层极淡的白色叠加
   * （--tint-1）就能被看见。落到纸上，那层叠加下面什么都没有，点就整个消失了。
   * 给它一块和岛同族的实底。只补底，不碰字色——那颗表示「其中一件失败了」的点
   * 是危险色，那是它要说的话。
   */
  .drift :global(.sat) {
    background: var(--panel);
  }

  /* left/top 给的是落点，-50% 把它变成「以这个点为心」，剩下两段是飞进来的路。 */
  .drift {
    position: absolute;
    transform: translate(-50%, -50%)
      translate(calc(var(--f) * var(--fx)), calc(var(--f) * var(--fy))) scale(var(--s));
  }

  /* 三段字共用同一格：一段接一段地换，底下的版面不能跟着抖。 */
  .captions {
    display: grid;
    min-height: 2lh;
    margin-top: var(--gap);
    text-align: center;
    font-size: 17px;
    line-height: 1.8;
    color: var(--mut);
  }
  .captions > * {
    grid-area: 1 / 1;
    place-self: center;
    max-width: 46ch;
  }

  .biome {
    font-size: 12px;
    letter-spacing: 0.16em;
    color: var(--mut);
  }

  .headline {
    font-size: clamp(21px, 2.3vw, 30px);
    font-weight: 620;
    letter-spacing: -0.02em;
    line-height: 1.3;
    color: var(--ink);
  }

  @media (max-width: 760px) {
    .screen {
      aspect-ratio: 3 / 4;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .screen,
    .bar,
    .hero,
    .drift,
    .bar :global(.island),
    .hero :global(.hail),
    .hero :global(.name),
    .hero :global(.meta),
    .hero :global(.go-row) {
      transform: none !important;
    }
    .screen,
    .bar,
    .hero :global(.hail),
    .hero :global(.name),
    .hero :global(.meta),
    .hero :global(.go-row) {
      opacity: 1 !important;
    }
  }
</style>
