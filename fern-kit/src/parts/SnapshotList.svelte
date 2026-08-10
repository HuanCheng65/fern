<script lang="ts">
  /**
   * 快照名单。
   *
   * 排版按时间轴组织而不是一张平铺的表——理由见 `snapshots.ts` 的 `byDay`。
   *
   * 一行只回答三件事，从左到右：**什么时候、为什么在这里、有多大**。别的都收进
   * 浮层——恢复要做的选择（恢复哪一部分、覆盖还是另存）是一次决定，不是一行里塞
   * 得下的三颗按钮。二十行各三颗按钮的列表没法扫读。
   *
   * 所以整行是一个按钮：点进去做决定，名单本身只用来找。给了 `onpick` 才可点；
   * 没给就是一张只读的名单（官网上的那种）。
   */
  import { CircleAlert, Pin } from 'lucide-svelte'
  import { byDay, clock, type SnapshotRow } from './snapshots'

  interface Props {
    rows: SnapshotRow[]
    /** 给了整行才可点。 */
    onpick?: (row: SnapshotRow) => void
  }

  let { rows, onpick }: Props = $props()

  const groups = $derived(byDay(rows))
</script>

{#each groups as group (group.key)}
  <div class="group">
    <h3 class="day">{group.label}</h3>
    <ul class="list">
      {#each group.items as item (item.id)}
        <li>
          <svelte:element
            this={onpick ? 'button' : 'div'}
            class="row"
            onclick={onpick ? () => onpick(item) : undefined}
          >
            <span class="t-mono when">{clock(item.takenAt)}</span>
            <span class="what">
              {item.title}
              {#if item.pinned}
                <span class="pin" title="永久保留">
                  <Pin size={11} strokeWidth={2} />
                </span>
              {/if}
              {#if item.inconsistent}
                <span class="flag" title="拍摄时文件仍在变动，内容可能不一致">
                  <CircleAlert size={11} strokeWidth={2} />
                </span>
              {/if}
            </span>
            {#if item.meta}<span class="t-mono meta">{item.meta}</span>{/if}
          </svelte:element>
        </li>
      {/each}
    </ul>
  </div>
{/each}

<style>
  .group + .group {
    margin-top: var(--s5);
  }

  /* 日期是分隔，不是标题——它不该和实例名争视觉层级。 */
  .day {
    margin: 0 0 var(--s1);
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-weight: 500;
    letter-spacing: 0.04em;
  }

  .list {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: var(--s4);
    width: 100%;
    /* 一屏几十行时靠伪元素向外够会互相重叠，行高自己抬到点击区下限。 */
    min-height: var(--hit);
    padding: var(--s2) var(--s2);
    margin-left: calc(var(--s2) * -1);
    border-radius: var(--r1);
    text-align: left;
    box-shadow: inset 0 -1px 0 var(--hairline-2);
    transition: background var(--t-fast) var(--ease);
  }

  button.row:hover {
    background: var(--tint-1);
    box-shadow: none;
  }

  .when {
    flex: none;
    width: 6ch;
    color: var(--ink-3);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  .what {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: 1;
    min-width: 0;
    color: var(--ink-2);
    font-size: var(--t-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pin,
  .flag {
    display: inline-grid;
    place-items: center;
    flex: none;
  }

  .pin {
    color: var(--ink-4);
  }

  .flag {
    color: var(--danger);
  }

  .meta {
    flex: none;
    color: var(--ink-4);
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }

  @media (max-width: 640px) {
    .meta {
      display: none;
    }
  }
</style>
