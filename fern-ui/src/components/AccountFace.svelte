<script lang="ts">
  /**
   * 把一个账户接到 kit 的那张脸上。
   *
   * 画的部分在 `fern-kit/parts/AccountFace.svelte`；这里只做产品才知道的两件事：
   * 从皮肤缓存里取出这个账户的脸，以及在还没取过时发一次请求。四处调用点因此都
   * 不用改，照旧传一个 `account`。
   */
  import Face from 'fern-kit/parts/AccountFace.svelte'
  import type { Account } from '../lib/accounts.svelte'
  import { skins } from '../lib/skins.svelte'

  interface Props {
    account: Account
    size: number
    round?: boolean
  }

  let { account, size, round = false }: Props = $props()

  const face = $derived(skins.face(account))
  $effect(() => {
    void skins.request(account)
  })
</script>

<Face {face} {size} {round} />
