<script lang="ts">
  /**
   * 游戏崩了之后说的那句话。
   *
   * 顺序是「发生了什么 → 能做什么 → 原始报告」，而且只有前两样默认可见。用户
   * 在这个时刻要的是下一步，不是一屏栈——栈是给他愿意深究、或者要贴给别人看的
   * 时候准备的，所以折叠着，但一定要在。
   *
   * 认不出原因时照实说认不出，不编一个听起来很像那么回事的诊断——写错的诊断
   * 比没有诊断更浪费时间，用户会顺着错的方向排查很久。这种时候「崩在哪个模组
   * 的代码里」往往是唯一的线索，所以嫌疑模组独立于诊断显示。
   *
   * 只负责画。诊断已经翻成句子了才递进来（`describe()` 在产品那边），浮层、
   * 页脚按钮和「打开日志目录」也都在外面——那些是对话框的事，不是这块板子的。
   */
  import { ChevronRight } from 'lucide-svelte'
  import Advice from './Advice.svelte'
  import { target } from './advice'
  import type { Diagnosed, Suspect } from './crash'

  interface Props {
    /** 已经翻成句子的诊断，认得越具体的排越前面。空表示一条都没认出来。 */
    found: Diagnosed[]
    /** 「退出码 1」或者「进程被系统终止」。怎么说由调用方决定。 */
    exit: string
    suspects?: Suspect[]
    reportPath?: string
    hsErrPath?: string
    excerpt?: string
    onfix?: () => Promise<void> | void
  }

  let {
    found,
    exit,
    suspects = [],
    reportPath,
    hsErrPath,
    excerpt = '',
    onfix
  }: Props = $props()

  let showRaw = $state(false)
</script>

<header>
  <h2>{found[0]?.title ?? '游戏异常退出'}</h2>
  <p class="exit">{exit}</p>
</header>

<div class="body">
  {#if found.length > 0}
    <p class="detail">{found[0].detail}</p>
    {#if found[0].action}
      <div class="fix">
        <!-- 标题上面已经说过了，这一行只说要动的是哪一样东西。 -->
        <Advice title={target(found[0].action)} detail="" action={found[0].action} {onfix} />
      </div>
    {/if}
  {:else}
    <p class="detail">没有匹配到已知的崩溃原因。下面是日志末尾，可用于进一步排查或反馈。</p>
  {/if}

  {#if suspects.length > 0}
    <p class="suspects">
      崩溃发生在{#each suspects.slice(0, 3) as suspect, index (suspect.modId)}{index > 0
          ? '、'
          : ' '}<strong>{suspect.name}{suspect.version ? ` ${suspect.version}` : ''}</strong
        >{/each} 的代码中。
    </p>
  {/if}

  <!-- 次要的那几条：可能同时成立，但不该抢第一条的位置。 -->
  {#if found.length > 1}
    <ul class="others">
      {#each found.slice(1) as item (item.id)}
        <li><strong>{item.title}</strong>{item.detail}</li>
      {/each}
    </ul>
  {/if}

  {#if reportPath}<p class="path">{reportPath}</p>{/if}
  {#if hsErrPath}<p class="path">{hsErrPath}</p>{/if}

  <button class="raw" onclick={() => (showRaw = !showRaw)}>
    <ChevronRight size={13} strokeWidth={2} class={showRaw ? 'turned' : ''} />
    {showRaw ? '收起原始日志' : '查看原始日志'}
  </button>

  {#if showRaw}
    <pre class="excerpt">{excerpt || '没有捕获到日志'}</pre>
  {/if}
</div>

<style>
  header {
    padding: var(--s5) var(--s5) 0;
  }

  header h2 {
    margin: 0;
    font-size: var(--t-h2);
    font-weight: 600;
    line-height: 1.25;
    letter-spacing: -0.014em;
    color: var(--ink);
  }

  .exit {
    margin: var(--s1) 0 0;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .body {
    padding: 0 var(--s5);
  }

  .detail {
    margin: var(--s3) 0 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  .fix {
    margin-top: var(--s2);
  }

  .suspects {
    margin: var(--s3) 0 0;
    color: var(--ink-2);
    font-size: var(--t-small);
    line-height: 1.65;
  }

  .suspects strong {
    font-weight: 500;
  }

  .others {
    display: grid;
    gap: var(--s2);
    margin: var(--s4) 0 0;
    padding: 0;
    list-style: none;
    color: var(--ink-3);
    font-size: var(--t-small);
    line-height: 1.6;
  }

  .others strong {
    margin-right: var(--s2);
    color: var(--ink-2);
    font-weight: 500;
  }

  /* 路径要能选中复制——这一句是拿去贴给别人的。 */
  .path {
    margin: var(--s3) 0 0;
    color: var(--ink-4);
    font-family: var(--mono);
    font-size: var(--t-small);
    overflow-wrap: anywhere;
    user-select: text;
  }

  .raw {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    margin: var(--s4) 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--ink-3);
    font: inherit;
    font-size: var(--t-small);
    cursor: pointer;
  }

  .raw:hover {
    color: var(--ink);
  }

  /* 箭头转 90 度表示展开，比换一个图标更安静。 */
  .raw :global(svg) {
    transition: transform var(--t-base) var(--ease);
  }

  .raw :global(svg.turned) {
    transform: rotate(90deg);
  }

  .excerpt {
    max-height: 40vh;
    margin: var(--s3) 0 0;
    padding: var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
    color: var(--ink-3);
    font-family: var(--mono);
    font-size: var(--t-micro);
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--tint-3) transparent;
    user-select: text;
  }
</style>
