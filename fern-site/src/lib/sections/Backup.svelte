<script>
  import { onMount } from 'svelte';
  import SnapshotList from 'fern-kit/parts/SnapshotList.svelte';
  import { reveal, track } from '$lib/scroll.js';

  /*
   * 真的名单。
   *
   * `SnapshotList` 就是实例里那一份，从 fern-kit 引进来——按天分组、今天/昨天的
   * 说法、等宽的时间列、图钉与警示标，全是组件自己的行为。
   *
   * 上一版这里是一条轨道加几个刻度。那种图说的是「时间会过去」，任何产品都能
   * 用；而这一节要说的是「你回得去」——**能回到哪一刻，是一份名单，不是一条线**。
   * 「改动模组之前」这五个字比任何隐喻都有说服力：它说明这些点不是定时拍的，
   * 是踩着你的动作拍的。
   */
  let p = $state(0);

  /* 时刻是相对「今天」算的，所以只能在浏览器里算——预渲染时的今天不是访客的
     今天，那正是这份名单最该说对的地方。 */
  let rows = $state([]);

  onMount(() => {
    /* 一律从此刻往回数，不写死钟点——写死的话下午三点打开这一页，「今天 20:12」
       是一张还没发生的快照。往回数还顺带保证了每一行的时刻都不一样。 */
    const now = Math.floor(Date.now() / 1000);
    const hoursAgo = (h) => now - Math.round(h * 3600);

    rows = [
      { id: '1', takenAt: hoursAgo(1.5), title: '改动模组之前', meta: '3 个世界 · 412 MB' },
      { id: '2', takenAt: hoursAgo(4), title: '游戏结束之后', meta: '3 个世界 · 408 MB' },
      { id: '3', takenAt: hoursAgo(26), title: '打红石之前', pinned: true, meta: '3 个世界 · 396 MB' },
      { id: '4', takenAt: hoursAgo(29), title: '游戏结束之后', meta: '3 个世界 · 394 MB' },
      {
        id: '5',
        takenAt: hoursAgo(31.5),
        title: '启动之前',
        inconsistent: true,
        meta: '3 个世界 · 391 MB'
      },
      { id: '6', takenAt: hoursAgo(50), title: '改动模组之前', meta: '2 个世界 · 355 MB' },
      { id: '7', takenAt: hoursAgo(53), title: '游戏结束之后', meta: '2 个世界 · 351 MB' }
    ];
  });
</script>

<section id="backup" use:track={{ onprogress: (v) => (p = v) }}>
  <div class="wrap grid">
    <div class="text">
      <div class="eyebrow" use:reveal>Fern 备份</div>
      <h2 use:reveal={{ delay: 40 }}>放心改。</h2>
      <p class="lede" use:reveal={{ delay: 80 }}>
        Fern 可以为实例与世界保留备份和快照，并在需要时恢复到之前的状态。
      </p>
      <p use:reveal={{ delay: 120 }}>
        改动模组前和游戏结束后会自动拍下。多张快照之间相同的文件只存一份，所以留得住很多张。
      </p>
      <p class="quote" use:reveal={{ delay: 160 }}>回得去，才敢往前。</p>
    </div>

    <!-- 从实例页里端出来的一截名单，最后一行压着下沿出画。 -->
    <div class="art" use:reveal={{ delay: 100 }}>
      <div class="sheet fern fern-dark" style="transform:translateY({(0.5 - p) * 28}px)">
        <div class="sheet-head">
          <span class="cap mono">快照</span>
          <span class="cap mono">共 24 张 · 1.2 GB</span>
        </div>
        <div class="list">
          <SnapshotList {rows} />
        </div>
      </div>
    </div>
  </div>
</section>

<style>
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 0.85fr) minmax(0, 1.15fr);
    gap: clamp(36px, 6vw, 80px);
    align-items: center;
  }

  .text h2 {
    margin-top: 14px;
  }
  .text .lede {
    margin-top: 26px;
    max-width: 34ch;
  }
  .text p:not(.lede):not(.quote) {
    margin-top: 18px;
    color: var(--mut);
    font-size: 17px;
    max-width: 38ch;
  }

  .quote {
    margin-top: 34px;
    font-size: clamp(20px, 2vw, 26px);
    font-weight: 600;
    letter-spacing: -0.015em;
    color: var(--ink);
  }

  .sheet {
    padding: 18px 20px 0;
    border-radius: 18px;
    background: var(--pine);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.1),
      0 24px 70px rgba(20, 32, 26, 0.16);
    will-change: transform;
  }

  .sheet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 16px;
  }
  .cap {
    font-size: 10px;
    letter-spacing: 0.18em;
    color: rgba(246, 244, 236, 0.42);
  }

  /* 裁在某一行上，不裁在一行字中间——后者看着像渲染坏了。 */
  .list {
    max-height: 340px;
    overflow: hidden;
    -webkit-mask-image: linear-gradient(to bottom, #000 78%, transparent 100%);
    mask-image: linear-gradient(to bottom, #000 78%, transparent 100%);
  }

  @media (max-width: 860px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .sheet {
      transform: none !important;
    }
  }
</style>
