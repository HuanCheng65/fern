<script>
  import { renderNotes } from '$lib/markdown.js';

  /*
   * 更新日志。内容是 Markdown（见 markdown.js 的注释），在那里转义过再拼成标签。
   *
   * 排版按「一列变化」来，不是按文章：小标题只作分组，条目才是主体，所以标题比
   * 条目更轻、更小，靠间距把组分开，不靠字号压人。
   */
  let { text = '' } = $props();

  const html = $derived(renderNotes(text));
</script>

{#if html}
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  <div class="notes">{@html html}</div>
{/if}

<style>
  /* {@html} 出来的东西不带作用域标记，只能用 :global 够到。 */
  .notes {
    margin-top: 20px;
    max-width: 62ch;
    font-size: 15px;
    line-height: 1.85;
    color: var(--mut);
  }

  .notes :global(h3),
  .notes :global(h4),
  .notes :global(h5) {
    margin-top: 22px;
    font-size: 13px;
    font-weight: 620;
    letter-spacing: 0.02em;
    color: var(--ink);
  }
  .notes :global(:first-child) {
    margin-top: 0;
  }

  .notes :global(ul),
  .notes :global(ol) {
    margin: 8px 0 0;
    padding-left: 1.1em;
  }
  .notes :global(li) {
    margin-top: 2px;
  }
  /* 条目的点按品牌色，和正文的灰分开：一眼能数清这一版改了几件事。 */
  .notes :global(li)::marker {
    color: var(--fern);
  }

  .notes :global(p) {
    margin-top: 10px;
  }

  .notes :global(strong) {
    color: var(--ink);
  }

  .notes :global(code) {
    padding: 1px 5px;
    border-radius: 5px;
    background: rgba(45, 95, 62, 0.08);
    font-family: var(--mono);
    font-size: 0.88em;
  }

  .notes :global(a) {
    color: inherit;
    text-decoration-color: var(--line);
    text-underline-offset: 3px;
  }
</style>
