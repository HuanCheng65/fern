<script lang="ts">
  /**
   * 一个人一张卡。
   *
   * 头像用的是同一套群系生成器，种子取 NodeID——所以每个人的色块是他自己的，
   * 换台机器还是那一张。文档里这里写的是皮肤渲染，等接了账户体系再换；在那
   * 之前用生成图，比灰色圆圈加首字母诚实，也和整个界面同族。
   *
   * 卡上只说三件事：谁、这条路好不好、多快。NAT 类型、候选地址、打洞轮次
   * 都不在这里——玩家要的是「能不能玩」，不是一份网络报告。
   */
  import { onMount } from 'svelte'
  import { paint } from 'fern-kit/ui/biome'
  import { renderBiome, supportsBiomeWorker } from '../lib/biome-client'
  import { isConnected, type Peer } from '../lib/pearl-session.svelte'
  import { PATH_LABEL, PATH_QUALITY, PUNCH_STAGE_LABEL, type PathState } from '../lib/pearl-types'

  interface Props {
    peer: Peer
    /** 中转路径上那个人的名字，用来说明是谁在帮忙。 */
    carrierName?: string
  }

  let { peer, carrierName }: Props = $props()

  let avatar = $state<HTMLCanvasElement>()

  onMount(() => {
    if (!avatar) return
    avatar.width = 96
    avatar.height = 96
    const options = { name: peer.id, hours: 40 }
    if (!supportsBiomeWorker) {
      paint(avatar, options, 0, 0.6)
      return
    }

    // 画在 Worker 里,主线程只负责把回来的位图贴上去。Worker 起不来就退回
    // 同步画——头像必须出现,慢一点好过没有。
    const request = renderBiome(avatar.width, avatar.height, options, 0, 0.6)
    request.promise
      .then((bitmap) => {
        const ctx = avatar?.getContext('2d')
        if (!ctx || !avatar) {
          bitmap.close()
          return
        }
        ctx.clearRect(0, 0, avatar.width, avatar.height)
        ctx.drawImage(bitmap, 0, 0, avatar.width, avatar.height)
        bitmap.close()
      })
      .catch(() => {
        if (avatar) paint(avatar, options, 0, 0.6)
      })
    return request.cancel
  })

  const path = $derived(
    isConnected(peer) && peer.state !== 'connected' ? (peer.state as PathState) : null,
  )
  const quality = $derived(path ? PATH_QUALITY[path] : null)
  const progress = $derived(
    peer.stage && peer.stageDone != null && peer.stageTotal
      ? Math.round((peer.stageDone / peer.stageTotal) * 100)
      : null,
  )
</script>

<article class="card">
  <canvas bind:this={avatar} class="avatar" aria-hidden="true"></canvas>

  <div class="body">
    <div class="name">{peer.name}</div>

    {#if path}
      <div class="line">
        <span class="dot {quality}"></span>
        <!-- 中转是能玩的，但花的是中间那个人的带宽，所以要说出来是谁——
             直接说在状态的位置，一行说完，不另起一行。 -->
        <span class="path">{path === 'via' ? `由 ${carrierName ?? '其他玩家'} 中转` : PATH_LABEL[path]}</span>
        {#if peer.rttMs != null}
          <span class="rtt">{peer.rttMs}<i>ms</i></span>
        {/if}
      </div>
    {:else if peer.state === 'failed' || peer.state === 'path_lost'}
      <div class="line">
        <span class="dot fail"></span>
        <span class="path">{peer.state === 'failed' ? '无法连接' : '正在重连'}</span>
      </div>
      {#if peer.detail}
        <div class="note">{peer.detail}</div>
      {/if}
    {:else if peer.state === 'disconnected'}
      <div class="line"><span class="dot fail"></span><span class="path">已离开</span></div>
    {:else}
      <div class="line">
        <span class="dot working"></span>
        <span class="path">{peer.stage ? PUNCH_STAGE_LABEL[peer.stage] : '正在连接'}</span>
        {#if progress != null}<span class="rtt">{progress}<i>%</i></span>{/if}
      </div>
      {#if progress != null}
        <div class="bar"><div class="fill" style:width="{progress}%"></div></div>
      {/if}
    {/if}
  </div>
</article>

<style>
  .card {
    display: flex;
    gap: var(--s4);
    align-items: center;
    padding: var(--s3);
    border-radius: var(--r2);
    background: var(--glass);
    backdrop-filter: blur(18px);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.05),
      var(--shadow-1);
  }

  .avatar {
    width: 48px;
    height: 48px;
    border-radius: var(--r1);
    flex: none;
    display: block;
  }

  .body {
    min-width: 0;
    flex: 1;
  }

  .name {
    font-size: var(--t-lead);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line {
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    margin-top: 2px;
    font-size: var(--t-cap);
    color: var(--ink-2);
    letter-spacing: 0.06em;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    flex: none;
    align-self: center;
    background: var(--ink-3);
  }

  .dot.best {
    background: var(--c4);
  }

  .dot.good {
    background: var(--c3);
  }

  /* 中转和失败都不用红色：红色是「坏了」，中转只是「不理想」。 */
  .dot.fallback {
    background: var(--c2);
    box-shadow: 0 0 0 3px rgba(255, 255, 255, 0.05);
  }

  .dot.fail {
    background: var(--ink-3);
  }

  .dot.working {
    background: var(--c3);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }

  .path {
    color: var(--ink-2);
  }

  /* 数字直接当视觉元素用：数值用等宽体，单位缩小压低。 */
  .rtt {
    margin-left: auto;
    font-family: var(--mono);
    font-size: var(--t-mono);
    color: var(--ink);
    font-variant-numeric: tabular-nums;
  }

  .rtt i {
    font-style: normal;
    font-size: 9px;
    color: var(--ink-3);
    margin-left: 1px;
  }

  .note {
    margin-top: 2px;
    font-size: var(--t-cap);
    color: var(--ink-3);
  }

  .bar {
    margin-top: var(--s2);
    height: 2px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--c3);
    transition: width var(--soft);
  }
</style>
