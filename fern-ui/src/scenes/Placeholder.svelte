<script lang="ts">
  /**
   * 还没长出来的场景。
   *
   * 空状态必须单独设计好，这是「内容即装饰」的代价（见 docs/UI_DESIGN.md 一）。
   * 这里只给两样东西：一句说明现状的话，一个出口。不编造数据，也不写「敬请
   * 期待」这类替代内容的文案。
   *
   * **不放生成式色块。** 那套图形是实例的脸，一旦被拿去填任何缺图的位置，它就
   * 不再指代任何东西；而在这里它更糟——一块精心生成的图会让人以为这个场景已经
   * 有内容了。空状态该靠排版和留白撑住，那也是文档里写的做法。
   */
  import { ArrowRight } from 'lucide-svelte'
  import Button from 'fern-kit/ui/Button.svelte'

  interface Props {
    title: string
    note: string
    onback: () => void
  }

  let { title, note, onback }: Props = $props()
</script>

<section class="placeholder">
  <h1 class="t-h1">{title}</h1>
  <p class="note">{note}</p>
  <div class="back">
    <Button variant="link" onclick={onback}>返回启动<ArrowRight size={14} /></Button>
  </div>
</section>

<style>
  .placeholder {
    display: grid;
    align-content: center;
    justify-items: start;
    gap: var(--s3);
    height: 100%;
    max-width: 46ch;
  }

  .note {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .back {
    margin-top: var(--s3);
  }
</style>
