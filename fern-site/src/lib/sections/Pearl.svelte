<script>
  import PeerCard from 'fern-kit/parts/PeerCard.svelte';
  import { reveal, track } from '$lib/scroll.js';

  /*
   * 真的卡片，真的连接过程，而且不装在盒子里。
   *
   * `PeerCard` 就是联机页里的那一张，从 fern-kit 引进来。上一版这里是两个点加一条
   * 线——那种图任何一个做内网穿透的产品都能画，它没有说出 Fern 做了什么。
   *
   * 但只把卡片端出来还不够：上一节（备份）已经是「左边文字、右边一块深色面板」，
   * 挨着再来一次就成了模板。**所以这里把卡片从窗口里拆出来**，直接落在地面上，
   * 用一根脊线和几条横档把它们连回同一个原点——线是几何，卡片是产品，讲故事的部分
   * 和证明的部分各站各的位置。走线只走直角，和标志同一套几何。
   *
   * 随着滚动，三个人依次从「正在连接」落到各自的结果。这是产品里真会发生的两秒钟，
   * 摊到一段滚动上。**最后一个停在中转**——打不通的时候确实要借道，卡片会说出是谁
   * 在帮忙。把这一格画成三条直连是在撒谎，而那正好是这套卡片最值得看的地方。
   */
  let p = $state(0);

  const FRIENDS = [
    { id: 'f7a31c9e42', name: '小满', at: 0.34 },
    { id: 'b28d1f4a6c', name: '阿哲', at: 0.46 },
    { id: '5e9c07b3da', name: 'Nyx', at: 0.58 }
  ];

  /** 落定之后各自走通的那条路。 */
  const SETTLED = [
    { state: 'lan', rttMs: 3 },
    { state: 'punched', rttMs: 24 },
    { state: 'via', rttMs: 61, via: 'f7a31c9e42' }
  ];

  const STAGES = ['direct', 'mappings', 'guessing', 'waiting'];

  /** 这个人此刻是什么样。落定之前是打洞的进度，之后是结果。 */
  function peerAt(index) {
    const friend = FRIENDS[index];
    if (p >= friend.at) return { id: friend.id, name: friend.name, ...SETTLED[index] };

    const from = friend.at - 0.28;
    const done = Math.max(0, Math.min(1, (p - from) / (friend.at - from)));
    return {
      id: friend.id,
      name: friend.name,
      state: 'connecting',
      stage: STAGES[Math.min(STAGES.length - 1, Math.floor(done * STAGES.length))],
      stageDone: Math.round(done * 12),
      stageTotal: 12
    };
  }

  const peers = $derived(FRIENDS.map((_, i) => peerAt(i)));
  const nameOf = (id) => FRIENDS.find((f) => f.id === id)?.name;

  /** 脊线随滚动长出来，长到最后一个人接上为止。 */
  const spine = $derived(Math.max(0, Math.min(1, (p - 0.14) / 0.46)));

  /* 横档是「开始尝试」，不是「已经连上」——所以它跟着这个人自己的时间线出现，
     和卡片里那根进度条同时开始动。 */
  const stub = (i) => (p >= FRIENDS[i].at - 0.28 ? 1 : 0);

  /* 脊线到最后一条横档为止。卡片高度不一样（正在连接的那张多一根进度条），
     算不出来，所以量出来——一根停在半空的线看着像画坏了。 */
  let cards = $state();
  let railEnd = $state(0);

  $effect(() => {
    if (!cards) return;
    const measure = () => {
      const nodes = cards.querySelectorAll('.node');
      const last = nodes[nodes.length - 1];
      if (!last) return;
      railEnd = cards.offsetHeight - (last.offsetTop + last.offsetHeight / 2);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(cards);
    return () => observer.disconnect();
  });
</script>

<section id="pearl" class="dark" use:track={{ onprogress: (v) => (p = v) }}>
  <div class="wrap">
    <div class="head">
      <div class="eyebrow" use:reveal>Pearl</div>
      <h2 use:reveal={{ delay: 40 }}>远一点，也像在身边。</h2>
    </div>

    <div class="body">
      <div class="text">
        <p class="lede" use:reveal={{ delay: 80 }}>创建房间，分享邀请码。</p>
        <p use:reveal={{ delay: 120 }}>
          Pearl 会自动寻找合适的连接方式，让远方的朋友也能加入你的 Minecraft 局域网世界。
        </p>
        <p use:reveal={{ delay: 160 }}>无需单独搭建服务器，也无需手动配置复杂的网络环境。</p>

        <!-- 邀请码是这件事的全部操作：把它发出去，剩下的不归你管。 -->
        <div class="invite" use:reveal={{ delay: 200 }}>
          <span class="invite-cap mono">邀请码</span>
          <span class="code mono">481 502</span>
        </div>
      </div>

      <!--
        卡片就摆在地面上，没有窗口也没有盒子。左边那根脊线是它们共同的原点——
        你的世界只有一个，路有三条。
      -->
      <div class="cluster fern fern-dark" use:reveal={{ delay: 120 }}>
        <div class="origin">
          <span class="pip"></span>
          <span class="origin-cap mono">你的世界</span>
        </div>

        <div class="rail" aria-hidden="true" style="bottom:{railEnd}px">
          <span class="spine" style="transform:scaleY({spine})"></span>
        </div>

        <div class="cards" bind:this={cards}>
          {#each peers as peer, i (peer.id)}
            <div class="node">
              <span class="stub" aria-hidden="true" style="transform:scaleX({stub(i)})"></span>
              <PeerCard {peer} hour={18} carrierName={peer.via ? nameOf(peer.via) : undefined} />
            </div>
          {/each}
        </div>
      </div>
    </div>
  </div>
</section>

<style>
  .head h2 {
    margin-top: 14px;
    max-width: 16ch;
  }

  .body {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
    gap: clamp(40px, 7vw, 96px);
    align-items: start;
    margin-top: clamp(48px, 6vw, 82px);
  }

  .text .lede {
    color: var(--paper);
  }
  .text p:not(.lede) {
    margin-top: 18px;
    color: var(--on-dark-mut);
    font-size: 17px;
    max-width: 40ch;
  }

  .invite {
    display: inline-flex;
    align-items: baseline;
    gap: 16px;
    margin-top: 36px;
    padding: 14px 22px;
    border: 1px solid var(--on-dark-line);
    border-radius: 14px;
  }
  .invite-cap {
    font-size: 10px;
    letter-spacing: 0.18em;
    color: var(--on-dark-mut);
  }
  .code {
    font-size: 22px;
    font-weight: 500;
    letter-spacing: 0.14em;
    color: var(--sprout);
  }

  /* ---- 卡片群 ---- */

  .cluster {
    position: relative;
    padding-left: 68px;
  }

  .origin {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: -68px;
    padding-bottom: 18px;
  }
  .pip {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: var(--sprout);
    flex: none;
  }
  .origin-cap {
    font-size: 10px;
    letter-spacing: 0.18em;
    color: var(--on-dark-mut);
  }

  /* 脊线从原点那一格往下长。 */
  .rail {
    position: absolute;
    left: 4px;
    top: 9px;
    width: 1px;
    background: var(--on-dark-line);
    overflow: hidden;
  }
  .spine {
    display: block;
    height: 100%;
    background: var(--sprout);
    transform-origin: top;
    will-change: transform;
    transition: transform 240ms linear;
  }

  .cards {
    display: grid;
    gap: 18px;
    /* 卡片是一列名单，不是一条横幅——拉到 600 px 宽之后，名字和延迟之间会空出
       半张卡的距离。 */
    max-width: 520px;
  }

  .node {
    position: relative;
  }
  /* 横档从脊线伸到卡片，落在卡片的竖直中线上。 */
  .stub {
    position: absolute;
    left: -64px;
    top: 50%;
    width: 56px;
    height: 1px;
    background: var(--sprout);
    transform-origin: left;
    transition: transform 320ms cubic-bezier(0.2, 0.8, 0.3, 1);
  }

  @media (max-width: 900px) {
    .body {
      grid-template-columns: 1fr;
    }
    .cluster {
      margin-top: 40px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spine,
    .stub {
      transform: none !important;
    }
  }
</style>
