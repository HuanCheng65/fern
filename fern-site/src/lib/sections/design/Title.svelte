<script>
  import { reveal } from '$lib/scroll.js';

  /*
   * 设计这一章的题幕。整整一屏，只有一句话。
   *
   * 上一段 Bento 结束之后，这里要先空一下：一章开始的地方不该紧接着上一章的最后一
   * 个格子。所以它占满视口、居中、别的什么都没有——滚进来的人先读到「新意舒展」，
   * 再看见任何界面。
   */
</script>

<section id="design" class="title">
  <div class="wrap">
    <div class="eyebrow" use:reveal>设计</div>
    <h2 use:reveal={{ delay: 80 }}>新意舒展。</h2>
  </div>
</section>

<style>
  .title {
    display: grid;
    place-items: center;
    min-height: 100vh;
    padding: 0;
    text-align: center;
  }

  .eyebrow {
    margin-bottom: clamp(20px, 2.6vw, 34px);
  }

  h2 {
    font-size: clamp(48px, 8vw, 128px);
    letter-spacing: -0.03em;
  }
</style>
