<script lang="ts">
  /**
   * 一根滑杆。
   *
   * 内存那一节有一根尺（MemoryMeter），但它量的是三个嵌套的区间，几何本身在
   * 说话。多数设置要的只是「在一段范围里选一个数」，为它套一把带幽灵刻度的尺
   * 是在承诺一些并不存在的信息。
   *
   * 数字写在哪由调用方决定——一个数该配什么单位、要不要「恢复默认」，是那一行
   * 的知识，不是滑杆的。这里只负责那条线和它接受的输入。
   */
  interface Props {
    value: number
    min?: number
    max: number
    step?: number
    /** 按住 PageUp/PageDown 走多大一步。不给就是 `step` 的四倍。 */
    page?: number
    label: string
    /** 读屏器听到的那句话。不给就念数字本身。 */
    text?: string
    onchange: (value: number) => void
  }

  let { value, min = 0, max, step = 1, page, label, text, onchange }: Props = $props()

  let track = $state<HTMLElement>()
  let dragging = $state(false)

  const clamp = (raw: number) =>
    Math.min(max, Math.max(min, min + Math.round((raw - min) / step) * step))
  const at = $derived(`${((clamp(value) - min) / Math.max(max - min, 1)) * 100}%`)

  function valueAt(clientX: number) {
    if (!track) return value
    const box = track.getBoundingClientRect()
    return clamp(min + ((clientX - box.left) / box.width) * (max - min))
  }

  function grab(event: PointerEvent) {
    // 指针捕获：拖出这根线的范围之后手指还是连着它的，松开也收得回来。
    track?.setPointerCapture(event.pointerId)
    dragging = true
    onchange(valueAt(event.clientX))
  }

  function drag(event: PointerEvent) {
    if (dragging) onchange(valueAt(event.clientX))
  }

  function drop(event: PointerEvent) {
    dragging = false
    track?.releasePointerCapture(event.pointerId)
  }

  /** 键盘要能走完这根线：它是个滑杆，不是一张图。 */
  function keys(event: KeyboardEvent) {
    const big = page ?? step * 4
    const jump: Record<string, number> = {
      ArrowLeft: -step,
      ArrowDown: -step,
      ArrowRight: step,
      ArrowUp: step,
      PageDown: -big,
      PageUp: big,
    }
    if (event.key in jump) onchange(clamp(value + jump[event.key]!))
    else if (event.key === 'Home') onchange(min)
    else if (event.key === 'End') onchange(max)
    else return
    event.preventDefault()
  }
</script>

<div
  class="track"
  bind:this={track}
  role="slider"
  aria-label={label}
  aria-valuemin={min}
  aria-valuemax={max}
  aria-valuenow={clamp(value)}
  aria-valuetext={text}
  tabindex="0"
  onpointerdown={grab}
  onpointermove={drag}
  onpointerup={drop}
  onpointercancel={drop}
  onkeydown={keys}
>
  <span class="fill" style:width={at}></span>
  <span class="grip" style:left={at}></span>
</div>

<style>
  /* 几何和内存那根尺一致：两处读起来该是同一种东西。 */
  .track {
    position: relative;
    height: 10px;
    margin-top: var(--s3);
    border-radius: 999px;
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
    cursor: pointer;
  }

  .track:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 999px;
    background: var(--accent);
    transition: width var(--t-fast) var(--ease);
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

  /* 拖动时不要过渡：那会让线追着指针跑，手感变成一段延迟。 */
  .track:active .fill,
  .track:active .grip {
    transition: none;
  }
</style>
