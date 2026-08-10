<script>
  import Mark from 'fern-kit/ui/Mark.svelte';
  import DownloadButton from '$lib/DownloadButton.svelte';
  import { RELEASES } from '$lib/downloads.js';
  import { reveal } from '$lib/scroll.js';

  /*
   * 页尾这一块回答一个问题：**现在按哪儿。**
   *
   * 按钮本身是 `DownloadButton`，和首页顶上、下载页顶上那颗是同一个组件。
   */
</script>

<section class="dark get" id="get">
  <div class="wrap">
    <div class="card" use:reveal>
      <Mark size={52} pad={0} color="var(--sprout)" />
      <h2>好看，好用，好好玩。</h2>
      <p class="lede">管理游戏，发现内容，进入世界，与朋友一起玩。</p>

      <div class="act">
        <DownloadButton meta more="查看全部下载选项" />
      </div>
    </div>

    <footer>
      <p>
        Fern 是一款非官方 Minecraft 启动器，与 Mojang Studios、Microsoft
        及其关联公司不存在隶属或官方合作关系。Minecraft 及相关商标归其各自权利人所有。
      </p>
      <p>Fern 不支持 Minecraft 中国版，也不提供相关功能。</p>
      <p>
        文中出现的第三方项目名称、图标、操作系统商标及 Minecraft
        默认角色形象归其各自权利人所有，此处仅用于标识与功能示意。
      </p>
      <p class="cr mono">
        Copyright © Astral Studio · <a href={RELEASES}>发布记录</a>
      </p>
    </footer>
  </div>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 26px;
    padding-bottom: clamp(64px, 9vw, 120px);
  }
  .card h2 {
    margin-top: 6px;
  }
  .card .lede {
    max-width: 40ch;
  }

  .act {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    margin-top: 10px;
  }

  footer {
    border-top: 1px solid var(--on-dark-line);
    padding-top: clamp(32px, 4vw, 48px);
    max-width: 68ch;
  }
  footer p {
    font-size: 13px;
    line-height: 2;
    color: var(--on-dark-mut);
  }
  footer p + p {
    margin-top: 14px;
  }
  footer a {
    color: inherit;
  }
  .cr {
    margin-top: 26px !important;
    font-size: 11px;
    letter-spacing: 0.14em;
  }
</style>
