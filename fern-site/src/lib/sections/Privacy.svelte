<script>
  import { reveal } from '$lib/scroll.js';
</script>

<section id="privacy" class="quiet">
  <div class="wrap">
    <div class="box" use:reveal>
      <div class="eyebrow">安全与隐私</div>
      <h3>安心，自有分寸。</h3>
      <p>Fern 认真保护你的账户、游戏数据与个人信息。</p>
      <p>
        系统级凭据保护、内容完整性验证，以及覆盖内容安装与游戏运行的安全检查，共同守护重要的数据与游戏环境。
      </p>
    </div>
  </div>
</section>

<style>
  .quiet {
    padding: clamp(64px, 9vw, 120px) 0;
  }
  .box {
    max-width: 52ch;
    margin: 0 auto;
    text-align: center;
  }
  .box h3 {
    margin-top: 14px;
  }
  .box p {
    margin-top: 20px;
    color: var(--mut);
    font-size: 16px;
  }
</style>
