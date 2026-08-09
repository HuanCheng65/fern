<script lang="ts">
  /**
   * 一个账户的脸。全应用只此一处画它。
   *
   * 画的永远是**皮肤**：有真皮肤就是那一张，没有就是游戏本身会给的默认
   * Steve/Alex（按 UUID 的哈希奇偶选，规则和游戏一致）。生成式色块不进这里——
   * 那套图形是实例的脸，拿它当没有皮肤时的头像，等于告诉玩家「你长这样」，而
   * 他在游戏里根本不长这样。
   *
   * **头部在这里裁，不在 Rust 里裁。** 皮肤图的 (8,8) 起 8×8 是头，(40,8) 起
   * 的同样大小是帽子层，64×32 的老皮肤这两块也在同样的位置——两层背景加一次
   * 放大就够了，为此在后端引一个图像库不值得。放大必须是 `pixelated`：这是像素
   * 画，平滑插值等于把它毁掉。
   */
  import type { Account } from '../lib/accounts.svelte'
  import { skins } from '../lib/skins.svelte'

  interface Props {
    account: Account
    /** 边长，px。 */
    size: number
    /** 圆的还是跟着行走方角。题头里是圆的，名单里是方的。 */
    round?: boolean
  }

  let { account, size, round = false }: Props = $props()

  const face = $derived(skins.face(account))
  $effect(() => {
    void skins.request(account)
  })

  /**
   * 上面一层是帽子（40,8），下面一层是头（8,8）。CSS 的背景是先写的盖在后写的
   * 上面，所以帽子写在前。
   *
   * 老皮肤没有帽子这一层可叠——它那块是不透明的纯黑（见 `Face.hat`），叠上去
   * 就是一颗黑头。那时只画头。
   */
  const layers = $derived(face.hat ? [-size * 5, -size] : [-size])
</script>

<span
  class="face"
  class:round
  style:--face={`${size}px`}
  style:background-image={layers.map(() => `url("${face.url}")`).join(', ')}
  style:background-size={`${size * 8}px auto`}
  style:background-position={layers.map((x) => `${x}px ${-size}px`).join(', ')}
></span>

<style>
  .face {
    display: block;
    flex: none;
    width: var(--face);
    height: var(--face);
    /* 像素画的放大只有一种正确做法。 */
    image-rendering: pixelated;
    background-repeat: no-repeat;
    border-radius: calc(var(--r1) * 0.8);
  }

  .round {
    border-radius: 999px;
  }
</style>
