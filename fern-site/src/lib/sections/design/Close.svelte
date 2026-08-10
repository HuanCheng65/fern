<script>
  import TopBar from 'fern-kit/parts/TopBar.svelte';
  import InstanceCard from 'fern-kit/parts/InstanceCard.svelte';
  import { PRIORITY } from 'fern-kit/parts/island';
  import { paletteVars } from '$lib/palette.js';
  import { track } from '$lib/scroll.js';

  /*
   * 第五幕：收回来。
   *
   * 这一章一路在拆：界面的各部分汇进一块屏、屏里的实例一个个换过去、岛被从窗口里
   * 拎出来落到纸上、五件真东西摊了一整页。拆到这里够了——最后一幕把它们收回一个
   * 窗口，页面安静下来，只剩一句话。
   *
   * 收回去的是**另一个场景**：不是前三幕那一屏启动，是实例那一屏。同一条顶栏、同一
   * 个窗口、同一座岛，换个场景仍然成立——这比再演一遍启动更能把这一章说完。
   *
   * 窗口和顶栏是真的（`TopBar`、`InstanceCard`），和第一幕那块屏共用同一套度量，
   * 所以两头看着是同一台机器。这里不再讲任何新的设计概念，也不留下一步提示：合完
   * 就进下一章。
   */
  let p = $state(0);

  const RAIL = 420;
  const TRAVEL = RAIL + 100;

  const clamp = (v) => Math.max(0, Math.min(1, v));
  const ease = (v) => 1 - Math.pow(1 - v, 3);
  /* 和整章同一种写法：时间轴按「轨道走过了多少 vh」记。 */
  const at = (from, span) => ease(clamp((p * TRAVEL - from) / span));

  /*
   * 窗口先在，东西才回来——「收回来」本来就得先有个可收的地方。
   *
   * 反过来（先摆卡片再围边框）试过：卡片一路是浅色的字压在纸上，名字整段路都看不
   * 见；而且中间那一大段窗口半透明，深色压在纸白上就是一片灰。
   */
  const frame = $derived(at(60, 100));
  /* 卡片从四面收回各自的格子。 */
  const gather = $derived(at(120, 140));
  /* 全都安静下来之后，才轮到那句话。 */
  const last = $derived(at(300, 70));

  const SCENES = [
    { id: 'launch', label: '启动' },
    { id: 'instances', label: '实例' },
    { id: 'supply', label: '补给' },
    { id: 'multiplayer', label: '联机' },
    { id: 'wardrobe', label: '衣柜' }
  ];

  /* 游戏正开着。岛回到顶栏右边——第三幕拎出去的那座，落回它自己的位置。 */
  const PRESENCE = {
    id: 'run',
    priority: PRIORITY.live,
    tone: 'live',
    label: '主世界 运行中',
    fill: 0.48,
    rows: [],
    actions: []
  };

  /*
   * 前四个是整章一路跟下来的那批世界，同一批钟点；后四个是这个人库里别的实例。
   *
   * 只放四个的话，一扇 16:10 的窗口有三分之二是空的——那读起来是「这个启动器里
   * 没什么东西」，不是「安静」。真实的库本来就不止四个。
   */
  const INSTANCES = [
    { name: '主世界', detail: '1.20.1 · Fabric', hour: 9, current: true },
    { name: '机械动力', detail: '1.20.1 · Fabric', hour: 18 },
    { name: '原版生存', detail: '1.21.4 · 原版', hour: 13 },
    { name: '宝可梦', detail: '1.21.1 · Fabric', hour: 22 },
    { name: '建筑档', detail: '1.21.4 · NeoForge', hour: 16 },
    { name: '光影档', detail: '1.21.1 · Fabric', hour: 6 },
    { name: '极限生存', detail: '1.16.5 · Forge', hour: 21 },
    { name: '红石实验', detail: '1.19.2 · Fabric', hour: 11 }
  ];

  /*
   * 每张从自己那边回来，深浅不一。和第一幕的汇入是同一条规矩，只是这次终点在窗口
   * **里面**：那一幕是各部分回到界面上的原位，这一幕是各实例回到名单里的位置。
   */
  const DRIFT = [
    { x: -30, y: 14, s: 0.16 },
    { x: -11, y: -26, s: -0.1 },
    { x: 13, y: 25, s: 0.09 },
    { x: 32, y: -16, s: -0.13 },
    { x: -26, y: -20, s: -0.12 },
    { x: -8, y: 24, s: 0.11 },
    { x: 16, y: -22, s: 0.14 },
    { x: 29, y: 18, s: -0.09 }
  ];
</script>

<section
  id="design-close"
  class="close-rail"
  style="height:{RAIL}vh"
  use:track={{ onprogress: (v) => (p = v) }}
>
  <div class="pin">
    <!-- 和前三幕那块屏一样，是一张产品的画，不是产品。 -->
    <div
      class="screen fern fern-dark"
      style="{paletteVars({ name: '主世界', hours: 320, hour: 9 })};--frame:{frame}"
      inert
    >
      <div class="plate"></div>

      <!-- 系统画的那三颗，不是 Fern 的组件（见第一幕的注释）。 -->
      <div class="traffic" aria-hidden="true">
        <span class="close"></span>
        <span class="min"></span>
        <span class="zoom"></span>
      </div>

      <div class="bar">
        <TopBar scenes={SCENES} scene="instances" mac presences={[PRESENCE]} />
      </div>

      <div class="grid">
        {#each INSTANCES as item, i (item.name)}
          <div
            class="slot"
            style="--g:{1 - gather};--gx:{DRIFT[i].x}vw;--gy:{DRIFT[i].y}vh;--gs:{DRIFT[i].s}"
          >
            <InstanceCard
              name={item.name}
              cover={item.name}
              detail={item.detail}
              hour={item.hour}
              current={item.current}
            />
          </div>
        {/each}
      </div>
    </div>

    <p class="last" style="opacity:{last}">好看，自有道理。</p>
  </div>
</section>

<style>
  /*
   * 卡片从画外飞回来，横向会甩出视口一大截——不夹住的话整个文档就宽出那一截，页面
   * 右边空出一条（第二幕那层环境光已经栽过一次）。用 clip 不用 hidden：hidden 会
   * 让这里变成一个滚动容器，里面那个 sticky 就不钉了。
   */
  .close-rail {
    position: relative;
    padding: 0;
    overflow-x: clip;
  }

  /* 和第一幕那块屏同一套：让开固定顶栏，两行都按内容高，整组一起居中。 */
  .pin {
    --gap: clamp(20px, 2.8vh, 32px);

    position: sticky;
    top: var(--nav);
    display: grid;
    grid-template-rows: auto auto;
    align-content: center;
    justify-items: center;
    height: calc(100vh - var(--nav));
    padding: clamp(12px, 1.8vh, 24px) clamp(20px, 4vw, 48px) var(--gap);
  }

  .screen {
    /* 窗口的度量，和第一幕那块屏一个字不差。 */
    --top: 48px;
    --pad-x: 22px;
    --frame-controls: 0px;

    position: relative;
    width: min(100%, 1020px);
    aspect-ratio: 16 / 10;
    max-height: calc(100vh - var(--nav) - 13rem);
    border-radius: clamp(14px, 1.6vw, 22px);
  }

  /* 窗口自己。它比里面的东西晚一步。 */
  .plate {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--c0);
    box-shadow:
      inset 0 0 0 1px rgba(246, 244, 236, 0.08),
      0 40px 110px rgba(20, 32, 26, calc(0.06 + 0.2 * var(--frame)));
    opacity: var(--frame);
  }

  .traffic {
    position: absolute;
    top: clamp(12px, 1.5vw, 18px);
    left: clamp(14px, 1.6vw, 20px);
    z-index: 4;
    display: flex;
    gap: 8px;
    opacity: var(--frame);
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

  .bar {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 3;
    height: var(--top);
    opacity: var(--frame);
  }

  /* 实例那一屏：顶栏底下一排卡片。 */
  .grid {
    position: absolute;
    inset: calc(var(--top) + clamp(14px, 2.4vw, 30px)) clamp(20px, 3.4vw, 46px) auto;
    z-index: 2;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: clamp(12px, 1.8vw, 22px);
  }

  /*
   * 越靠近格子越实。远端那一截路在窗口外面，卡片的字是给深底调的浅色，压在纸上
   * 什么也读不出来——那一段干脆别看见。
   */
  .slot {
    opacity: calc(1 - var(--g) * 1.15);
    transform: translate(calc(var(--g) * var(--gx)), calc(var(--g) * var(--gy)))
      scale(calc(1 + var(--g) * var(--gs)));
  }

  /* 这一章的最后一句。它是回声，不是新的一句话。 */
  .last {
    margin-top: var(--gap);
    min-height: 2lh;
    text-align: center;
    font-size: clamp(28px, 3.6vw, 46px);
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--ink);
  }

  @media (max-width: 760px) {
    .screen {
      aspect-ratio: 3 / 4;
    }
    .grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  /* 关掉动效时不留半路的状态：窗口就在那儿，八张卡就在格子里。 */
  @media (prefers-reduced-motion: reduce) {
    .slot {
      transform: none !important;
    }
    .slot,
    .plate,
    .traffic,
    .bar,
    .last {
      opacity: 1 !important;
    }
  }
</style>
