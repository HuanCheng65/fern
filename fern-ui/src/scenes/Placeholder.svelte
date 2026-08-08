<script lang="ts">
  /**
   * 还没长出来的场景。
   *
   * 空状态必须单独设计好，这是「内容即装饰」的代价（见 docs/UI_DESIGN.md 一）。
   * 这里只给三样东西：一块属于这个场景的色、一句说明现状的话、一个出口。
   * 不编造数据，也不写「敬请期待」这类替代内容的文案。
   */
  import { ArrowRight } from 'lucide-svelte'
  import Cover from 'fern-kit/Cover.svelte'

  interface Props {
    /** 生成色块的种子，用场景名——每个场景有自己的一块颜色。 */
    seed: string
    title: string
    note: string
    onback: () => void
  }

  let { seed, title, note, onback }: Props = $props()
</script>

<section class="placeholder">
  <div class="mark"><Cover {seed} quality={0.5} /></div>
  <h1 class="t-h1">{title}</h1>
  <p class="note">{note}</p>
  <button class="btn btn--link" onclick={onback}>返回启动<ArrowRight size={14} /></button>
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

  .mark {
    width: 46px;
    height: 46px;
    overflow: hidden;
    border-radius: var(--r2);
    margin-bottom: var(--s2);
  }

  .note {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .btn--link {
    margin-top: var(--s3);
  }
</style>
