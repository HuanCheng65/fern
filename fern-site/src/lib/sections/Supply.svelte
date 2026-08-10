<script>
  import SupplyCard from 'fern-kit/parts/SupplyCard.svelte';
  import { reveal, track } from '$lib/scroll.js';

  let p = $state(0.5);

  /*
   * 真的卡片，真的项目。
   *
   * `SupplyCard` 就是补给页里的那一张，从 fern-kit 引进来，吃的也是它自己的
   * Hit 结构——图标、两行截断的描述、下载量的写法，全是组件自己的行为。
   *
   * 图标本地留一份，不从 Modrinth 的 CDN 拉。下载量是取整的量级，不是实时数
   * ——这一格是讲「能直接发现这些东西」，不是一块计数牌。
   */
  const HITS = [
    {
      projectId: 'AANobbMI',
      slug: 'sodium',
      title: 'Sodium',
      description: '现代化的渲染引擎，大幅提升帧率，同时修掉不少原版的画面瑕疵。',
      author: 'jellysquid3',
      downloads: 52_000_000,
      iconUrl: '/content/sodium.webp',
      categories: ['optimization']
    },
    {
      projectId: '1KVo5zza',
      slug: 'fabulously-optimized',
      title: 'Fabulously Optimized',
      description: '开箱即用的性能整合包，保持原版手感的同时把帧数拉起来。',
      author: 'Robotkoer',
      downloads: 12_000_000,
      iconUrl: '/content/fabulously-optimized.webp',
      categories: ['optimization']
    },
    {
      projectId: 'BVzZfTc1',
      slug: 'faithful-32x',
      title: 'Faithful 32×',
      description: '在两倍分辨率下忠实还原原版美术风格的资源包。',
      author: 'Faithful Team',
      downloads: 2_400_000,
      iconUrl: '/content/faithful-32x.png',
      categories: ['realistic']
    },
    {
      projectId: 'Q1vvjJYV',
      slug: 'bsl-shaders',
      title: 'BSL Shaders',
      description: '柔和光照与体积光的光影包，风格自然，兼容性也好。',
      author: 'capttatsu',
      downloads: 3_100_000,
      iconUrl: '/content/bsl-shaders.webp',
      categories: ['shader']
    }
  ];
</script>

<section id="supply" use:track={{ onprogress: (v) => (p = v) }}>
  <div class="wrap grid">
    <div class="text" use:reveal>
      <div class="eyebrow">补给</div>
      <h2>喜欢的，就带回去。</h2>
      <p>模组、整合包、资源包与光影，都可以直接在 Fern 中发现。</p>
      <p>
        选择内容，再选择它要加入的实例。Fern 会匹配合适的版本、处理必要的依赖，并在安装前清楚展示即将发生的变化。
      </p>
    </div>

    <!--
      不是截图，是那几张卡片本身，从窗口里端出来摆在纸上。所以它得自带一块
      地面——这些卡是给深色界面画的，纸白上没有它们站的地方。
    -->
    <div class="art" use:reveal={{ delay: 80 }}>
      <div class="stack fern fern-dark" style="transform:translateY({(0.5 - p) * 34}px)">
        <p class="cap">补给</p>
        {#each HITS as hit}
          <SupplyCard {hit} />
        {/each}
      </div>
    </div>
  </div>
</section>

<style>
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
    gap: clamp(36px, 6vw, 88px);
    align-items: center;
  }
  .text h2 {
    margin-top: 14px;
  }
  .text p {
    margin-top: 22px;
    color: var(--mut);
    font-size: 17px;
    max-width: 40ch;
  }

  .stack {
    padding: 18px 10px 0;
    border-radius: 18px;
    background: var(--pine);
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.1),
      0 24px 70px rgba(20, 32, 26, 0.14);
    will-change: transform;
    /* 最后一张压着下沿出画：这是从一页里端出来的一截，不是一张完整的图。
       裁在卡片中间，不裁在某一行字中间——后者看着像渲染坏了。 */
    max-height: 360px;
    overflow: hidden;
  }

  .cap {
    margin: 0 0 8px 12px;
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.18em;
    color: rgba(246, 244, 236, 0.42);
  }

  @media (max-width: 860px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .stack {
      transform: none !important;
    }
  }
</style>
