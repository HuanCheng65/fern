<script lang="ts">
  /**
   * 一个账户的脸。全应用只此一处画它。
   *
   * 画的永远是**皮肤**：有真皮肤就是那一张，没有就是游戏本身会给的默认 Steve/Alex。
   * 生成式色块不进这里——那套图形是实例的脸，拿它当没有皮肤时的头像，等于告诉玩家
   * 「你长这样」，而他在游戏里根本不长这样。
   *
   * **头部在这里裁，不在后端裁。** 皮肤图的 (8,8) 起 8×8 是头，(40,8) 起的同样大小
   * 是帽子层，64×32 的老皮肤这两块也在同样的位置——两层背景加一次放大就够了，为此
   * 在后端引一个图像库不值得。放大必须是 `pixelated`：这是像素画，平滑插值等于把它
   * 毁掉。
   *
   * 拿到的是一张脸，不是一个账户：取皮肤要请求、要缓存、要认识账户体系，那些是产品
   * 那边的事。这里只负责把它画对。
   */
  interface Face {
    url: string
    /** 有没有可叠的帽子层。老皮肤那块是不透明的纯黑，叠上去就是一颗黑头。 */
    hat: boolean
  }

  interface Props {
    face: Face
    /** 边长，px。 */
    size: number
    /** 圆的还是跟着行走方角。题头里是圆的，名单里是方的。 */
    round?: boolean
  }

  let { face, size, round = false }: Props = $props()

  /**
   * 上面一层是帽子（40,8），下面一层是头（8,8）。CSS 的背景是先写的盖在后写的
   * 上面，所以帽子写在前。
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
