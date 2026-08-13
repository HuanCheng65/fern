<script lang="ts">
  /**
   * 一节的抬头：左边说这是什么，右边是能对它做的事。
   *
   * 模组、存档、快照三处逐字写了三遍同样的十三行 CSS 和同样的结构。它们看起来
   * 像各自的样式，其实是同一个约定——「标题 + 一句次要的计数 + 右侧操作」。
   * 每加一节列表就多抄一份，而抄的那份没有任何东西盯着它别走样。
   *
   * `note` 和 `actions` 收 snippet 而不是字符串：三处的次要文字分别是「12/30
   * 启用」「3 个世界」「5 份 · 1.2 GB」，右边分别是一排按钮、一个链接、一个
   * 主按钮。这一层不该知道那些是什么。
   */
  import type { Snippet } from 'svelte'

  interface Props {
    /** 这一节叫什么。 */
    title: string
    /**
     * 标题后面那一小行次要信息，通常是计数或体积。
     *
     * **没有就别传。** 一个写着「0 个」的抬头不如没有——空列表下面已经有一句
     * 完整的话在解释了。
     */
    note?: Snippet
    /** 右侧操作。 */
    actions?: Snippet
  }

  let { title, note, actions }: Props = $props()
</script>

<div class="head">
  <span class="label">
    {title}
    {@render note?.()}
  </span>
  {@render actions?.()}
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .label {
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
  }

  /*
   * `:global`，因为 `note` 里的标记属于调用方，带的是调用方的作用域哈希——
   * 一条普通的 `.label small` 在这里一个元素都选不中。范围仍然被 `.label`
   * 锁死，越不出这个抬头。
   */
  .label :global(small) {
    margin-left: var(--s2);
    font-weight: 400;
  }
</style>
