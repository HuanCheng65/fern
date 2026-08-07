<script lang="ts">
  /**
   * 布局一：舞台。
   *
   * 全出血内容，零页面导航，UI 浮在艺术之上。见 docs/UI_DESIGN.md 十。
   * 启动场景专用——它本质上就是当前实例的封面艺术本身。
   *
   * 内容坐在左下角，上方和右方是留白。这不是没排满，是画框的意思。
   */
  import type { Snippet } from 'svelte'

  interface Props {
    children: Snippet
  }

  let { children }: Props = $props()
</script>

<section class="stage-layout">
  <div class="copy">{@render children()}</div>
</section>

<style>
  .stage-layout {
    display: flex;
    align-items: flex-end;
    height: 100%;
    padding-bottom: var(--s2);
  }

  .copy {
    width: min(620px, 100%);
  }
</style>
