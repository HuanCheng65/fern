<script lang="ts">
  /**
   * 布局五：表单。
   *
   * 左侧锚点目录，右侧单列表单流。见 docs/UI_DESIGN.md 十。
   *
   * 锚点和浏览布局的筛选栏都在左边，职责是一致的——都在回答「眼前这个长
   * 东西，我要看哪一段」，所以「纵向改视图」这条语法没有破。
   *
   * 列宽压在 640px 上下保证可读性，右侧留白。空白是排印的一部分。
   */
  import type { Snippet } from 'svelte'

  interface Props {
    sections: { id: string; label: string }[]
    section: string
    onsection: (id: string) => void
    head?: Snippet
    children: Snippet
  }

  let { sections, section, onsection, head, children }: Props = $props()
</script>

<div class="form scroll">
  {#if head}
    <header>{@render head()}</header>
  {/if}

  <div class="layout">
    <nav aria-label="分节">
      {#each sections as item (item.id)}
        <button
          class:on={section === item.id}
          aria-current={section === item.id ? 'true' : undefined}
          onclick={() => onsection(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </nav>
    <div class="content">{@render children()}</div>
  </div>
</div>

<style>
  .form {
    height: 100%;
    min-height: 0;
    padding-right: var(--s2);
  }

  .layout {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: clamp(var(--s5), 5vw, var(--s8));
    padding-bottom: var(--s8);
  }

  /* 第三级重量，和浏览布局的筛选栏同一档。 */
  nav {
    display: flex;
    flex-direction: column;
    align-items: start;
    gap: var(--s1);
    position: sticky;
    top: 0;
    align-self: start;
  }

  nav button {
    padding: var(--s1) 0;
    color: var(--ink);
    font-size: var(--t-small);
    opacity: 0.4;
    transition: opacity var(--t-fast) var(--ease);
  }

  nav button:hover {
    opacity: 0.75;
  }

  nav button.on {
    opacity: 1;
  }

  .content {
    max-width: 640px;
  }

  @media (max-width: 720px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--s4);
    }

    nav {
      position: static;
      flex-direction: row;
      flex-wrap: wrap;
      gap: var(--s4);
    }
  }
</style>
