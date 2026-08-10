<script>
  import { onMount } from 'svelte';
  import InstanceCard from 'fern-kit/parts/InstanceCard.svelte';
  import SupplyCard from 'fern-kit/parts/SupplyCard.svelte';
  import SnapshotList from 'fern-kit/parts/SnapshotList.svelte';
  import PeerCard from 'fern-kit/parts/PeerCard.svelte';
  import MemoryMeter from 'fern-kit/ui/MemoryMeter.svelte';
  import { paletteVars } from '$lib/palette.js';
  import { reveal } from '$lib/scroll.js';

  /*
   * 第四幕：东西很多，秩序只有一套。
   *
   * 接着第三幕的语气往下说。上一幕是一件真东西离开窗口落到纸上；这一幕是**一整批**
   * ——实例、模组、快照、联机、内存，五种毫不相干的活儿，全部从窗口里拆出来摊在同一
   * 张纸上。不画辅助线，不写标注：能不能看出它们是同一个产品，本来就该由它们自己
   * 证明，加一层说明反而是心虚。
   *
   * 这一幕不再钉住，也不再连贯——正常滚动，每一件进视口时自己起身。前三幕是一条不能
   * 断的线，这里是一页可以随便看的东西，节奏本来就该松下来。
   *
   * 五件都是 kit 里的真组件，一个都没有重画。**都不给回调**——kit 自己就认这条：
   * 「不给就是不能点：官网上这几张只是给人看的」（见 SupplyCard）。整块再加 inert，
   * 免得留下一排按得下去却什么都不会发生的按钮。
   *
   * 每一件带着一个实例的色板，还是那四个世界——颜色是从内容推出来的，这一章从头到尾
   * 都在说这句话。
   */
  const WORLDS = {
    home: { name: '主世界', hours: 320, hour: 9 },
    create: { name: '机械动力', hours: 260, hour: 18 },
    vanilla: { name: '原版生存', hours: 420, hour: 13 },
    poke: { name: '宝可梦', hours: 88, hour: 22 }
  };

  const SODIUM = {
    projectId: 'AANobbMI',
    slug: 'sodium',
    title: 'Sodium',
    description: '现代化的渲染引擎，大幅提升帧率，同时修掉不少原版的画面瑕疵。',
    author: 'jellysquid3',
    downloads: 52_000_000,
    iconUrl: '/content/sodium.webp',
    categories: ['optimization']
  };

  const PEER = { id: 'f7a31c9e42', name: '小满', state: 'lan', rttMs: 3 };

  /*
   * 快照的时间是相对现在算的，所以只能在浏览器里定。写在模块顶上的话，静态构建会把
   * 打包那一刻的钟点烤进 HTML，页面越放越久就越不对。
   */
  let snaps = $state([]);
  onMount(() => {
    const now = Math.floor(Date.now() / 1000);
    const hoursAgo = (h) => now - Math.round(h * 3600);
    snaps = [
      { id: '1', takenAt: hoursAgo(1.5), title: '改动模组之前', meta: '2 个世界 · 412 MB' },
      { id: '2', takenAt: hoursAgo(4), title: '游戏结束之后', meta: '2 个世界 · 408 MB' },
      { id: '3', takenAt: hoursAgo(26), title: '打红石之前', pinned: true, meta: '1 个世界 · 396 MB' }
    ];
  });
</script>

<section id="order" class="order">
  <div class="wrap">
    <h2 use:reveal>东西很多，秩序只有一套。</h2>
    <p class="lede" use:reveal={{ delay: 60 }}>
      实例、模组、快照、联机、内存——每一样活儿都不同，读法完全一样。
    </p>

    <!-- 一排只给人看的标本：不接指针，不进 tab，不进无障碍树。 -->
    <div class="spread" inert>
      <div class="piece one fern fern-dark" style={paletteVars(WORLDS.home)} use:reveal>
        <InstanceCard name="主世界" cover="主世界" detail="1.20.1 · Fabric" hour={9} current />
      </div>

      <div
        class="piece two fern fern-dark"
        style={paletteVars(WORLDS.poke)}
        use:reveal={{ delay: 90 }}
      >
        <SupplyCard hit={SODIUM} />
      </div>

      <div
        class="piece three fern fern-dark"
        style={paletteVars(WORLDS.vanilla)}
        use:reveal={{ delay: 60 }}
      >
        <SnapshotList rows={snaps} />
      </div>

      <div
        class="piece four fern fern-dark"
        style={paletteVars(WORLDS.create)}
        use:reveal={{ delay: 120 }}
      >
        <MemoryMeter
          label="内存"
          physicalMb={19456}
          ceilingMb={12288}
          valueMb={6144}
          marks={[{ at: 4300, label: '上次峰值' }]}
        />
      </div>

      <div
        class="piece five fern fern-dark"
        style={paletteVars(WORLDS.home)}
        use:reveal={{ delay: 80 }}
      >
        <PeerCard peer={PEER} hour={13} />
      </div>
    </div>
  </div>
</section>

<style>
  .order {
    padding: clamp(100px, 13vw, 180px) 0 clamp(90px, 11vw, 150px);
  }
  h2 {
    max-width: 16ch;
  }
  .lede {
    margin-top: 24px;
    max-width: 36ch;
  }

  /*
   * 十二列，每件自己占几列、自己错开多少。
   *
   * 不排成整齐的两栏或三栏：一排等宽等高的卡片会立刻变成一张组件清单，而这一幕要
   * 说的恰恰不是「我们有这些组件」，是「这些东西摆在一起仍然像一个东西」。
   */
  .spread {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: clamp(20px, 3vw, 44px);
    margin-top: clamp(56px, 8vw, 110px);
  }

  /* 每件都自带一块深色的地。kit 的这些组件是照深底画的，落到纸上得先有地面。 */
  .piece {
    align-self: start;
    padding: clamp(14px, 1.6vw, 20px);
    border-radius: 18px;
    background: var(--c0);
    box-shadow:
      inset 0 0 0 1px rgba(246, 244, 236, 0.07),
      0 14px 44px rgba(20, 32, 26, 0.16);
  }

  /* 错落靠上边距，不靠位移：位移不占位，行高就不跟着变，底下会空出莫名的缝。 */
  .one {
    grid-column: 1 / 5;
    grid-row: 1;
  }
  .two {
    grid-column: 7 / 13;
    grid-row: 1;
    margin-top: clamp(24px, 5vw, 76px);
  }
  .three {
    grid-column: 2 / 8;
    grid-row: 2;
  }
  .four {
    grid-column: 9 / 13;
    grid-row: 2;
    margin-top: clamp(20px, 4vw, 58px);
  }
  .five {
    grid-column: 4 / 9;
    grid-row: 3;
  }

  @media (max-width: 900px) {
    .spread {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .one,
    .two,
    .three,
    .four,
    .five {
      grid-column: auto;
      grid-row: auto;
      margin-top: 0;
    }
    .two,
    .three {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 560px) {
    .spread {
      grid-template-columns: minmax(0, 1fr);
    }
    .one,
    .four,
    .five {
      grid-column: 1 / -1;
    }
  }
</style>
