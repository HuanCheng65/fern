<script>
  import DownloadButton from '$lib/DownloadButton.svelte';
  import Notes from '$lib/Notes.svelte';
  import OsMark from '$lib/OsMark.svelte';
  import {
    CHANNELS,
    CHANNEL_NAME,
    RELEASES,
    channelOnce,
    detectOs,
    filesFor,
    osName,
    readableDate
  } from '$lib/downloads.js';

  /*
   * 下载页。
   *
   * 两条通道，各自去问自己的清单，互不影响——这样「正式版还没有」和「测试版取不到」
   * 不会互相掩盖。三种状态对应三句不同的话：有版本、通道尚无版本、取不到。
   *
   * **取不到 ≠ 没有。** 网络不通、跨域被挡、清单损坏，都不能说成「本项目尚未发布」；
   * 那种时候给出发布记录的地址，让人自己去拿。
   *
   * 正式版存在时，正式版在前、测试版收在下面；正式版尚无版本时，页面直说仍在早期
   * 测试阶段，只提供测试版。判断依据是端点的 404，不是写死的开关。
   */
  const ORDER = ['windows', 'macos', 'linux'];

  let here = $state(null);
  let found = $state([]);
  let loading = $state(true);

  $effect(() => {
    here = detectOs();
    Promise.all(CHANNELS.map((c) => channelOnce(c))).then((all) => {
      found = all;
      loading = false;
    });
  });

  const byChannel = $derived(Object.fromEntries(found.map((f) => [f.channel, f])));
  const stable = $derived(byChannel.stable);
  const beta = $derived(byChannel.beta);

  /* 正式版还没有过版本，而测试版有：这时整页以测试版为主。 */
  const earlyAccess = $derived(stable?.state === 'absent' && beta?.state === 'ready');
  const unreachable = $derived(
    !loading && found.length > 0 && found.every((f) => f.state === 'offline')
  );

  const ready = $derived(found.filter((f) => f.state === 'ready'));
</script>

<svelte:head>
  <title>下载 Fern</title>
  <meta name="description" content="获取适用于 Windows、macOS 与 Linux 的 Fern。" />
</svelte:head>

<main class="page">
  <div class="wrap">
    <header class="head">
      <div class="eyebrow">下载</div>
      <h1>获取 Fern</h1>
      <p class="lede">适用于 Windows、macOS 与 Linux。</p>

      <!--
        当前系统的那个包就摆在标题底下。底下的全平台清单是给「换一台机器下」和
        「要另一种格式」准备的，不该是所有人都得先滚过去的一段路。
      -->
      <div class="now">
        <DownloadButton meta fallback={false} />
      </div>
    </header>

    {#if loading}
      <p class="state mono">正在读取版本信息。</p>
    {:else if unreachable}
      <div class="notice">
        <p>暂时无法读取版本信息。</p>
        <p class="sub">请前往发布记录获取安装包。</p>
        <a class="cta" href={RELEASES}>打开发布记录</a>
      </div>
    {:else}
      {#if earlyAccess}
        <div class="notice">
          <p>Fern 仍处于早期测试阶段，尚未发布正式版。</p>
          <p class="sub">下列为测试版本，可能包含未完成的功能与缺陷。</p>
        </div>
      {/if}

      {#each ready as rel (rel.channel)}
        <section class="channel" class:minor={!earlyAccess && rel.channel === 'beta'}>
          <div class="bar">
            <h2>{CHANNEL_NAME[rel.channel]}</h2>
            <p class="ver mono">
              {rel.version}{#if rel.date}<span class="dot">·</span>{readableDate(rel.date)}{/if}
            </p>
          </div>

          <Notes text={rel.notes} />

          <ul class="platforms">
            {#each ORDER as os (os)}
              <li class:mine={os === here}>
                <div class="who">
                  <span class="mark"><OsMark {os} size={22} /></span>
                  <span class="name">{osName(os)}</span>
                  {#if os === here}<span class="tag">当前系统</span>{/if}
                </div>

                <div class="files">
                  {#each filesFor(rel.version)[os] as file (file.id)}
                    <a href={file.url} download>
                      <span class="label">{file.label}{#if file.ext}<em class="mono">{file.ext}</em>{/if}</span>
                      <span class="note mono">{file.note}</span>
                    </a>
                  {/each}
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/each}

      {#if !earlyAccess && beta?.state === 'absent'}
        <p class="state mono">测试通道当前没有可用版本。</p>
      {/if}
    {/if}

    <footer class="foot">
      <a href="/">返回首页</a>
      <a href={RELEASES}>全部发布记录</a>
    </footer>
  </div>
</main>

<style>
  .page {
    padding: clamp(96px, 12vw, 150px) 0 clamp(80px, 10vw, 130px);
  }

  .head {
    max-width: 44ch;
  }
  h1 {
    margin-top: 14px;
    font-size: clamp(38px, 5vw, 66px);
  }
  .lede {
    margin-top: 20px;
  }

  .now {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    margin-top: clamp(28px, 3.4vw, 40px);
  }

  .state {
    margin-top: clamp(44px, 6vw, 72px);
    font-size: 13px;
    letter-spacing: 0.04em;
    color: var(--mut);
  }

  /* 需要先读的一段话。它不是卡片，只是一块被围起来的说明。 */
  .notice {
    margin-top: clamp(44px, 6vw, 72px);
    padding: clamp(20px, 2.4vw, 28px) clamp(22px, 2.6vw, 32px);
    border-radius: 14px;
    background: rgba(45, 95, 62, 0.07);
    box-shadow: inset 0 0 0 1px rgba(45, 95, 62, 0.16);
  }
  .notice p {
    font-size: 16px;
    line-height: 1.75;
  }
  .notice .sub {
    margin-top: 6px;
    color: var(--mut);
  }
  .notice .cta {
    margin-top: 18px;
  }

  .channel {
    margin-top: clamp(44px, 6vw, 76px);
  }
  /* 有正式版时，测试版是次要的那一段：小一号，压低一档，但内容一样全。 */
  .channel.minor {
    margin-top: clamp(50px, 7vw, 88px);
    opacity: 0.78;
  }

  .bar {
    display: flex;
    align-items: baseline;
    gap: 16px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--line);
  }
  .bar h2 {
    font-size: clamp(22px, 2.4vw, 30px);
  }
  .channel.minor .bar h2 {
    font-size: clamp(19px, 2vw, 24px);
  }
  .ver {
    font-size: 12px;
    color: var(--mut);
  }
  .dot {
    margin: 0 8px;
    opacity: 0.5;
  }

  .platforms {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }

  .platforms li {
    display: grid;
    grid-template-columns: minmax(150px, 200px) minmax(0, 1fr);
    gap: clamp(16px, 3vw, 40px);
    align-items: start;
    padding: clamp(18px, 2.2vw, 26px) 0;
    border-bottom: 1px solid var(--line);
  }

  .who {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-top: 8px;
  }
  .mark {
    color: var(--ink);
    opacity: 0.8;
  }
  .name {
    font-size: 17px;
    font-weight: 620;
  }
  /* 当前系统那一行标出来就够了，不改变顺序——顺序稳定比省一次寻找重要。 */
  .tag {
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--sprout);
    color: var(--pine);
    font-size: 11px;
    letter-spacing: 0.02em;
  }

  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  .files a {
    display: grid;
    gap: 3px;
    min-width: 200px;
    padding: 12px 16px;
    border-radius: 12px;
    color: inherit;
    text-decoration: none;
    box-shadow: inset 0 0 0 1px var(--line);
    transition:
      box-shadow 180ms ease,
      background 180ms ease;
  }
  .files a:hover {
    background: rgba(45, 95, 62, 0.05);
    box-shadow: inset 0 0 0 1px rgba(45, 95, 62, 0.32);
  }
  .label {
    font-size: 15px;
    font-weight: 560;
  }
  .label em {
    margin-left: 7px;
    font-size: 12px;
    font-style: normal;
    color: var(--mut);
  }
  .note {
    font-size: 11px;
    color: var(--mut);
  }

  .foot {
    display: flex;
    gap: 28px;
    margin-top: clamp(56px, 8vw, 90px);
    padding-top: clamp(26px, 3vw, 36px);
    border-top: 1px solid var(--line);
    font-size: 15px;
  }
  .foot a {
    color: var(--mut);
    text-decoration: none;
    border-bottom: 1px solid transparent;
    transition: color 180ms ease;
  }
  .foot a:hover {
    color: var(--ink);
    border-bottom-color: currentColor;
  }

  @media (max-width: 680px) {
    .platforms li {
      grid-template-columns: minmax(0, 1fr);
      gap: 14px;
    }
    .who {
      padding-top: 0;
    }
    .files a {
      min-width: 0;
      flex: 1 1 100%;
    }
  }
</style>
