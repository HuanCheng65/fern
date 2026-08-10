<script>
  import MemoryMeter from 'fern-kit/ui/MemoryMeter.svelte';
  import { reveal, track } from '$lib/scroll.js';

  let p = $state(0);
  /** 一段行程映射成 0–1。整节的编舞都由它排。 */
  const at = (from, span) => Math.min(1, Math.max(0, (p - from) / span));
  /** 第 i 行该出来了吗。前一格走完才轮到下一格。 */
  const on = (k, i, n) => k > (i + 0.15) / n;

  /*
   * 两栏都不是编的。
   *
   * 左边是 fern-core 真的会去读的信号（launch/memory/signals.rs）：版本、
   * 加载器、模组数、显卡，以及**此刻可用**的物理内存——不是总量。只看总量的
   * 方案会在用户开着浏览器和 IDE 的时候把系统压进 swap。
   *
   * 右边是它据此定下的三件事。Java 21 以上给分代 ZGC，更老的给 G1。
   */
  const READ = [
    ['版本', '1.21.4'],
    ['加载器', 'NeoForge'],
    ['模组', '148 个'],
    ['显卡', '独立'],
    ['可用内存', '19.4 GB']
  ];
  const SET = [
    ['Java', '21'],
    ['堆内存', '6.0 GB'],
    ['垃圾回收', '分代 ZGC']
  ];
  /** 跑过几次之后，实测数据把静态估算换掉。 */
  const TUNED = '5.4 GB';

  /*
   * 整段编舞压在 0.30–0.68 里。这一节不高，track 的 p 走到 0.7 时它已经在
   * 视口上沿之外了——排在那之后的节拍等于没发生过。
   */
  const read = $derived(at(0.3, 0.12));
  const spine = $derived(at(0.4, 0.06));
  const link = $derived(at(0.45, 0.05));
  const set = $derived(at(0.48, 0.12));
  const loop = $derived(at(0.58, 0.07));
  const tuned = $derived(p > 0.66);
</script>

<section id="runtime" use:track={{ onprogress: (v) => (p = v) }}>
  <div class="wrap grid">
    <div class="text" use:reveal>
      <div class="eyebrow">运行</div>
      <h2>准备妥当，随时开玩。</h2>
      <p>Fern 会根据游戏版本、实例内容与设备情况，自动准备合适的 Java、内存与运行配置。</p>
      <p><strong>Fern 自适应运行</strong>还会参考实际运行情况，持续调整资源分配。</p>
    </div>

    <!--
      示意图，不是界面：没有卡片、没有边框，直接落在纸上。走线只走直角——
      和标志是同一套几何，这一页上的转角都该是这一个转角。
    -->
    <div class="rig" use:reveal={{ delay: 60 }} aria-hidden="true">
      <div class="side">
        <p class="cap mono">读到的</p>
        {#each READ as [k, v], i}
          <div class="row" class:on={on(read, i, READ.length)}>
            <span class="k mono">{k}</span>
            <span class="v mono">{v}</span>
          </div>
        {/each}
      </div>

      <div class="side">
        <p class="cap mono">定下的</p>
        {#each SET as [k, v], i}
          <div class="row" class:on={on(set, i, SET.length)}>
            <span class="k mono">{k}</span>
            <span class="v mono">
              {#if v === '6.0 GB'}
                <!-- 自适应那一句，让它自己发生一次：这是全节唯一会动的数字。 -->
                <span class="was" class:off={tuned}>{v}</span>
                <span class="now" class:on={tuned}>→ {TUNED}</span>
              {:else}
                {v}
              {/if}
            </span>
          </div>
        {/each}
      </div>

      <span class="wire left" style="transform:scaleY({spine})"></span>
      <span class="wire right" style="transform:scaleY({spine})"></span>
      <span class="wire link" style="transform:scaleX({link})"></span>
      <!--
        回授走虚线，而且走满整幅：实测数据绕回左边，成为它读到的又一样东西。
        虚线是因为它比上面那条慢——是统计出来的，不是这一次算出来的。
      -->
      <span class="wire back" style="transform:scaleX({loop})"></span>
      <span class="wire riser" style="transform:scaleY({loop > 0.7 ? 1 : 0})"></span>
      <span class="tip" style="opacity:{loop > 0.85 ? 1 : 0}"></span>
      <p class="feed mono" style="opacity:{loop > 0.5 ? 1 : 0}">实测反馈 · 近 8 次会话</p>
    </div>

    <!--
      示意图讲的是机制（它凭什么这么定），这一条讲的是结果，而结果不需要画：
      产品里那根尺本身就说清楚了。**幽灵刻度是它真正的价值**——自动会给多少、
      上次实际用到多少，都画在同一根尺上，所以「内存不用你操心」这句话在这里
      是看得见的，不是我们说的。
    -->
    <div class="proof" use:reveal={{ delay: 120 }}>
      <p class="proof-cap mono">定下的 · 内存</p>
      <div class="proof-in fern fern-dark">
        <MemoryMeter
          label="内存"
          physicalMb={19456}
          ceilingMb={12288}
          valueMb={6144}
          marks={[
            { at: 6144, label: '自动分配' },
            { at: 4300, label: '上次峰值' }
          ]}
        />
      </div>
    </div>
  </div>
</section>

<style>
  /* 真的那根尺。它是给深色界面画的，所以自带一块地面。 */
  .proof {
    /* 它是上面那张图的结论，横跨两列摆在下面——落在左列里会读成正文的一部分。 */
    grid-column: 1 / -1;
    margin-top: clamp(28px, 3.4vw, 44px);
  }
  .proof-cap {
    margin-bottom: 14px;
    font-size: 11px;
    letter-spacing: 0.18em;
    color: var(--mut);
  }
  .proof-in {
    max-width: 560px;
    padding: 22px 24px 18px;
    border-radius: 16px;
    background: var(--pine);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.1),
      0 20px 56px rgba(20, 32, 26, 0.14);
  }

  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 440px);
    gap: clamp(32px, 5vw, 64px);
    align-items: center;
  }
  .text h2 {
    margin-top: 14px;
  }
  .text p {
    margin-top: 22px;
    color: var(--mut);
    font-size: 17px;
    max-width: 44ch;
  }

  /* ---------- 示意图 ---------- */

  .rig {
    position: relative;
    display: grid;
    /* 中间那 56px 是走线的地盘，两栏各占一半，所以竖线正好落在 50% ∓ 28px。 */
    grid-template-columns: minmax(0, 1fr) 56px minmax(0, 1fr);
    align-items: center;
    /* 给回授那条线让出的高度 */
    padding-bottom: 46px;
  }
  .side {
    grid-column: 1;
    padding-right: 14px;
  }
  .side:nth-of-type(2) {
    grid-column: 3;
    padding: 0 0 0 14px;
  }
  /*
   * 左栏的值靠着竖线右对齐（它们往里走），右栏的值贴着竖线左对齐（它们从
   * 里面出来）。方向感全在这一点上，别两栏都撑满。
   */
  .side:nth-of-type(2) .row {
    justify-content: flex-start;
    gap: 18px;
  }
  .side:nth-of-type(2) .k {
    min-width: 52px;
  }

  .cap {
    margin-bottom: 10px;
    font-size: 10px;
    letter-spacing: 0.18em;
    color: var(--mut);
  }

  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    height: 30px;
    opacity: 0;
    transform: translateY(4px);
    transition:
      opacity 320ms ease,
      transform 320ms ease;
  }
  .row.on {
    opacity: 1;
    transform: none;
  }
  .k {
    flex: none;
    font-size: 12px;
    color: var(--mut);
  }
  .v {
    font-size: 12px;
    color: var(--ink);
    white-space: nowrap;
  }

  /* 旧值不划掉，退到背景里去——它没有错，只是不再是最好的那个了。 */
  .was {
    transition: color 400ms ease;
  }
  .was.off {
    color: var(--mut);
  }
  .now {
    color: var(--fern);
    opacity: 0;
    transition: opacity 400ms ease;
  }
  .now.on {
    opacity: 1;
  }

  /* ---------- 走线 ---------- */

  .wire {
    position: absolute;
    background: var(--line);
  }
  /*
   * 别叫 .in——reveal 进场时会往元素上挂一个全局的 .in，同一个 scope 里撞上
   * 就会把整段正文当成一条 1px 的竖线来画。
   */
  .left,
  .right {
    top: 4px;
    bottom: 0;
    width: 1px;
    transform-origin: top;
  }
  .left {
    left: calc(50% - 28px);
  }
  .right {
    left: calc(50% + 28px);
  }
  .link {
    top: calc((100% - 46px) / 2);
    left: calc(50% - 28px);
    width: 56px;
    height: 1px;
    transform-origin: left;
  }
  /* 从右边那根竖线的脚下起，一路走回左栏底下。 */
  .back {
    bottom: 0;
    left: 0;
    width: calc(50% + 28px);
    height: 0;
    background: none;
    border-top: 1px dashed var(--line);
    /* 回授是往回走的，所以它从右边长出来。 */
    transform-origin: right;
  }
  .riser {
    left: 0;
    bottom: 0;
    width: 0;
    height: 30px;
    background: none;
    border-left: 1px dashed var(--line);
    transform-origin: bottom;
    transition: transform 300ms ease;
  }
  /* 直角折角当箭头用，不画三角形。朝上，指回它读的那一栏。 */
  .tip {
    position: absolute;
    left: 0;
    bottom: 30px;
    width: 5px;
    height: 5px;
    border-top: 1px solid var(--mut);
    border-left: 1px solid var(--mut);
    rotate: 45deg;
    translate: -50% -1px;
    transition: opacity 300ms ease;
  }

  .feed {
    position: absolute;
    bottom: 0;
    left: calc(25% + 14px);
    translate: -50% 50%;
    padding: 0 10px;
    /* 压在虚线上，把它咬断一截 */
    background: var(--paper);
    font-size: 10px;
    letter-spacing: 0.12em;
    color: var(--mut);
    white-space: nowrap;
    transition: opacity 400ms ease;
  }

  @media (max-width: 860px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .rig {
      margin-top: 12px;
    }
  }

  @media (max-width: 560px) {
    .rig {
      grid-template-columns: minmax(0, 1fr) 36px minmax(0, 1fr);
    }
    .k,
    .v {
      font-size: 11px;
    }
    .side {
      padding-right: 10px;
    }
    .side:nth-of-type(2) {
      padding-left: 10px;
    }
    .left {
      left: calc(50% - 18px);
    }
    .right {
      left: calc(50% + 18px);
    }
    .link {
      left: calc(50% - 18px);
      width: 36px;
    }
    .back {
      width: calc(50% + 18px);
    }
    /* 窄屏上这行字比那条线还长，压上去等于把线整条盖掉——让到线下面去。 */
    .feed {
      left: 0;
      translate: 0 100%;
      padding: 0;
      background: none;
    }
  }
</style>
