<script>
  /**
   * 标志放到海报尺度：7×9 网格上的 33 格直角螺线。
   *
   * 格子里透出的不是另一张图，是**整屏那块场的同一处**——背景压暗、螺线不压，
   * 所以它读起来是同一个世界开的一扇窗，而不是贴上去的色块。对齐靠调用方给的
   * 原点偏移（ox/oy）和整幅尺寸（bw/bh）。
   *
   * 格子的顺序来自 fern-ui 的走线（茎底 → 外圈 → 内圈 → 芽尖），所以点亮的过程
   * 就是产品加载动画的过程，只是放大了一百倍。底色沿走线从蕨绿走到嫩芽，
   * 生长的方向是画出来的，不是配的色。
   */
  import { CELLS } from 'fern-kit/ui/mark';

  let {
    /** 整幅场的图 */
    img = '',
    /** 整幅场的尺寸，以及螺线左上角在其中的位置 */
    bw = 0,
    bh = 0,
    ox = 0,
    oy = 0,
    on = false,
    duration = 1600
  } = $props();

  const xs = CELLS.map((c) => c[0]);
  const ys = CELLS.map((c) => c[1]);
  const X0 = Math.min(...xs);
  const Y0 = Math.min(...ys);
  const COLS = Math.max(...xs) - X0 + 1;
  const ROWS = Math.max(...ys) - Y0 + 1;
  const grid = CELLS.map(([x, y]) => [x - X0, y - Y0]);

  // 整条螺线的外轮廓：一格的某条边，如果邻格不在螺线上，这条边就在轮廓上。
  // 逐格描边会变成马赛克，只描外轮廓才是一条完整的线。
  const outline = (() => {
    const has = new Set(grid.map(([x, y]) => `${x},${y}`));
    const segs = [];
    for (const [x, y] of grid) {
      if (!has.has(`${x},${y - 1}`)) segs.push([x, y, x + 1, y]);
      if (!has.has(`${x},${y + 1}`)) segs.push([x, y + 1, x + 1, y + 1]);
      if (!has.has(`${x - 1},${y}`)) segs.push([x, y, x, y + 1]);
      if (!has.has(`${x + 1},${y}`)) segs.push([x + 1, y, x + 1, y + 1]);
    }
    return segs;
  })();
</script>

<div
  class="spiral"
  class:on
  style="--cols:{COLS};--rows:{ROWS};--img:url({img});--bw:{bw}px;--bh:{bh}px;--ox:{ox}px;--oy:{oy}px;--draw:{duration}ms"
  aria-hidden="true"
>
  {#each grid as [cx, cy], i}
    <div
      class="cell"
      style="--cx:{cx};--cy:{cy};--k:{(i / (grid.length - 1)).toFixed(3)};
             --t:{((i / (grid.length - 1)) * 100).toFixed(1)}%;
             --delay:{(i / grid.length) * duration}ms"
    >
      <i></i>
      <b></b>
    </div>
  {/each}

  <svg class="edge" viewBox="0 0 {COLS} {ROWS}" preserveAspectRatio="none">
    {#each outline as [x1, y1, x2, y2]}
      <line {x1} {y1} {x2} {y2} vector-effect="non-scaling-stroke" />
    {/each}
  </svg>
</div>

<style>
  .spiral {
    position: absolute;
    inset: 0;
  }

  .cell {
    position: absolute;
    left: calc(var(--cx) * var(--cell));
    top: calc(var(--cy) * var(--cell));
    width: var(--cell);
    height: var(--cell);
    /* 没亮的格子也留一层——先看见螺线，再看见它长出来 */
    background: rgba(246, 244, 236, 0.05);
  }

  /* 场：整幅图里属于这一格的那一块。背景那层压着暗，这层反过来提亮——
     同一处曝光不同，螺线才像开出来的一扇窗。 */
  .cell i {
    position: absolute;
    inset: 0;
    opacity: 0;
    filter: brightness(1.95) saturate(1.15);
    transition: opacity 520ms ease;
    transition-delay: var(--delay);
    background-image: var(--img);
    background-size: var(--bw) var(--bh);
    background-position: calc((var(--ox) + var(--cx) * var(--cell)) * -1)
      calc((var(--oy) + var(--cy) * var(--cell)) * -1);
  }
  .spiral.on .cell i {
    opacity: 1;
  }

  /* 沿走线从蕨绿走到嫩芽，压在场上定方向 */
  .cell b {
    position: absolute;
    inset: 0;
    opacity: 0;
    background: color-mix(in oklab, var(--fern), var(--sprout) var(--t));
    mix-blend-mode: overlay;
    transition: opacity 520ms ease;
    transition-delay: var(--delay);
  }
  .spiral.on .cell b {
    opacity: calc(0.5 + 0.4 * var(--k));
  }

  .edge {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    fill: none;
    stroke: var(--sprout);
    stroke-width: 1;
    opacity: 0;
    transition: opacity 900ms ease;
    transition-delay: var(--draw);
  }
  .spiral.on .edge {
    opacity: 0.22;
  }

  @media (prefers-reduced-motion: reduce) {
    .cell i,
    .cell b,
    .edge {
      transition: none;
      transition-delay: 0ms !important;
    }
  }
</style>
