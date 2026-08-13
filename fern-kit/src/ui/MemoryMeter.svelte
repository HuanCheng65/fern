<script lang="ts">
  import Button from './Button.svelte'
  import { gigabytes } from './units'
  /**
   * 一条尺。
   *
   * 分配、上限、物理内存本来就是三个嵌套的区间——用三句话说要读三遍，画成一根
   * 尺一眼就完。这是内存那一节「全是字」的直接解药。
   *
   * **只读和可拖是同一根尺。** 几何完全一样，区别只是接不接受拖动。上一版自动
   * 是一段文字、手动是一根滑杆，切换时控件换人、高度跟着跳；现在切换只改变它
   * 能不能拖，那条线一动不动。
   *
   * 幽灵刻度才是这根尺真正的价值：自动会给多少、上次实际用到多少，都画在你正要
   * 拖的那条线上——做这个决定要看的两个数，就在决定发生的地方。**没有数据就不
   * 画**：一条编出来的刻度比没有刻度更糟。
   *
   * 上限之外那一段仍然画出来，只是明显不属于可用区。它回答的是「为什么推不动
   * 了」——一根到头就停、却没说为什么的滑杆，会让人以为是坏的。
   */
  interface Mark {
    /** 落在哪，MB。 */
    at: number
    label: string
  }

  interface Props {
    /** 底衬的总量：这台机器有多少内存。0 表示读不到，那时不画这一段。 */
    physicalMb: number
    /**
     * 此刻已被别的东西占住的内存。
     *
     * **从右端往左画**，不从左端起。两条都从 0 起的话，只要上限比已用小，填充
     * 就永远落在暗色里——而那时机器明明还空着一大片，等于画出一个假的告警。
     * 靠右之后中间那道缝就是真正的余量，**填充撞上暗色才是超售**，一句话都不
     * 用写。
     *
     * 只在尺的量程就是这台机器时才成立（设置页那根）。实例那根量的是上限，
     * 两个坐标系不同，混在一起就是骗人。
     */
    usedMb?: number
    /** 交给游戏的上限。可拖的右端就是它，没有旁路。 */
    ceilingMb: number
    valueMb: number
    minMb?: number
    stepMb?: number
    marks?: Mark[]
    /** 给了才可以拖。不给就是一把只读的尺。 */
    onchange?: (mb: number) => void
    /** 上限那个标签的去处。给了它才可点。 */
    onceiling?: () => void
    /**
     * 画不画那堵墙。
     *
     * 设置页里拖的**就是**上限本身，右端只是这台机器的物理内存——那里没有墙，
     * 硬画一道再标上「上限」就是在说一句同义反复。
     */
    showCeiling?: boolean
    label: string
  }

  let {
    physicalMb,
    usedMb,
    ceilingMb,
    valueMb,
    minMb = 1024,
    stepMb = 256,
    marks = [],
    onchange,
    onceiling,
    showCeiling = true,
    label,
  }: Props = $props()

  let track = $state<HTMLElement>()
  let dragging = $state(false)

  /**
   * 尺的全长代表多少内存。
   *
   * **不按物理内存铺满。** 那样在一台 1 TB 的机器上，8 GB 的堆会被压成一根看
   * 不见的线，连那堵墙都贴在最左边——尺量的是「这个实例拿到多少」，不是「这台
   * 机器有多大」。所以量程跟着上限走，墙外多留四分之一：足够让「推不动了是因为
   * 有一堵墙」看得见，又不会让墙外那段喧宾夺主。
   *
   * 设置页里拖的就是上限本身，右端没有墙，那时全长就是它自己。机器总量始终写在
   * 下面那行字里——它是一个事实，不需要占几何。
   */
  const span = $derived(Math.max(showCeiling ? ceilingMb * 1.25 : ceilingMb, valueMb, 1))
  const at = (mb: number) => `${Math.min(100, Math.max(0, (mb / span) * 100))}%`

  const clamp = (mb: number) =>
    Math.min(ceilingMb, Math.max(minMb, Math.round(mb / stepMb) * stepMb))

  function valueAt(clientX: number) {
    if (!track) return valueMb
    const box = track.getBoundingClientRect()
    return clamp(((clientX - box.left) / box.width) * span)
  }

  function grab(event: PointerEvent) {
    if (!onchange) return
    // 指针捕获：拖出这根尺的范围之后手指还是连着它的，松开也收得回来。
    track?.setPointerCapture(event.pointerId)
    dragging = true
    onchange(valueAt(event.clientX))
  }

  function drag(event: PointerEvent) {
    if (dragging && onchange) onchange(valueAt(event.clientX))
  }

  function drop(event: PointerEvent) {
    dragging = false
    track?.releasePointerCapture(event.pointerId)
  }

  /** 键盘要能走完这根尺：它是个滑杆，不是一张图。 */
  function keys(event: KeyboardEvent) {
    if (!onchange) return
    const jump: Record<string, number> = {
      ArrowLeft: -stepMb,
      ArrowDown: -stepMb,
      ArrowRight: stepMb,
      ArrowUp: stepMb,
      PageDown: -stepMb * 4,
      PageUp: stepMb * 4,
    }
    if (event.key in jump) onchange(clamp(valueMb + jump[event.key]!))
    else if (event.key === 'Home') onchange(minMb)
    else if (event.key === 'End') onchange(ceilingMb)
    else return
    event.preventDefault()
  }

</script>

<div class="meter" class:live={onchange !== undefined}>
  <!--
    两条抑制的理由是同一个：这块东西**可拖的时候就是一根滑杆**，`role` 和
    `tabindex` 是一起变的，而静态检查看不穿这个三元。只读那一档 role 是 img、
    tabindex 是 -1，既不进 tab 序也不接受键盘，是对的。
  -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="track"
    bind:this={track}
    role={onchange ? 'slider' : 'img'}
    aria-label={label}
    aria-valuemin={onchange ? minMb : undefined}
    aria-valuemax={onchange ? ceilingMb : undefined}
    aria-valuenow={onchange ? valueMb : undefined}
    aria-valuetext={gigabytes(valueMb)}
    tabindex={onchange ? 0 : -1}
    onpointerdown={grab}
    onpointermove={drag}
    onpointerup={drop}
    onpointercancel={drop}
    onkeydown={keys}
  >
    <!-- 可用区：0 到上限。它之外的那一段留着，但明显不是给游戏的。 -->
    <span class="reach" style:width={at(ceilingMb)}></span>
    <span class="fill" style:width={at(valueMb)}></span>
    {#if usedMb !== undefined}
      <span class="used" style:width={at(usedMb)}></span>
    {/if}
    {#each marks as mark (mark.label)}
      <span class="tick" style:left={at(mark.at)}></span>
    {/each}
    {#if showCeiling}
      <span class="wall" style:left={at(ceilingMb)}></span>
    {/if}
    {#if onchange}
      <span class="grip" style:left={at(valueMb)}></span>
    {/if}
  </div>

  <p class="legend">
    {#if usedMb !== undefined}
      <span class="note"><i class="swatch"></i>已用 {gigabytes(usedMb)}</span>
    {/if}
    {#each marks as mark (mark.label)}
      <span class="note"><i class="dot"></i>{mark.label}</span>
    {/each}
    <span class="ceiling">
      {#if showCeiling}
        {#if onceiling}
          <Button variant="link" onclick={onceiling}>上限 {gigabytes(ceilingMb)}</Button>
        {:else}
          上限 {gigabytes(ceilingMb)}
        {/if}
      {/if}
      {#if physicalMb > 0}
        <span class="t-quiet">{showCeiling ? '／' : ''}本机 {gigabytes(physicalMb)}</span>
      {/if}
    </span>
  </p>
</div>

<style>
  .meter {
    margin-top: var(--s3);
  }

  .track {
    position: relative;
    height: 10px;
    border-radius: 999px;
    /* 上限之外那一段：在，但暗。 */
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
  }

  .meter.live .track {
    cursor: pointer;
  }

  .track:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }

  .reach,
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 999px;
  }

  .reach {
    background: var(--tint-2);
  }

  .fill {
    background: var(--accent);
    transition: width var(--t-fast) var(--ease);
  }

  /* 拖动时不要过渡：那会让线追着指针跑，手感变成一段延迟。 */
  .meter.live .track:active .fill {
    transition: none;
  }

  /* 已用：从右端往左的一道暗色。中间那道缝就是余量。 */
  .used {
    position: absolute;
    inset: 0 0 0 auto;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.45);
  }

  /* 实测与自动值的刻度。压在填充上面，所以要比它亮。 */
  .tick {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--ink);
    opacity: 0.55;
  }

  /* 上限是一堵墙，不是一个刻度——它的高度和分量都该更像边界。 */
  .wall {
    position: absolute;
    top: -5px;
    bottom: -5px;
    width: 2px;
    margin-left: -1px;
    background: var(--ink-3);
  }

  .grip {
    position: absolute;
    top: 50%;
    width: 16px;
    height: 16px;
    margin: -8px 0 0 -8px;
    border-radius: 999px;
    background: var(--accent);
    box-shadow:
      0 0 0 3px var(--panel),
      var(--shadow-1);
    transition: left var(--t-fast) var(--ease);
  }

  .meter.live .track:active .grip {
    transition: none;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--s2) var(--s4);
    margin: var(--s3) 0 0;
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  .note {
    display: inline-flex;
    align-items: baseline;
    gap: 5px;
  }

  /* 和尺上那道刻度同一个东西，所以长得一样。 */
  .dot {
    align-self: center;
    width: 2px;
    height: 9px;
    border-radius: 1px;
    background: var(--ink);
    opacity: 0.55;
  }

  /* 同理：和尺上那一段同色，不必再写一句「就是那道暗的」。 */
  .swatch {
    align-self: center;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.45);
  }

  .ceiling {
    margin-left: auto;
    display: inline-flex;
    align-items: baseline;
    gap: 2px;
    white-space: nowrap;
  }
</style>
