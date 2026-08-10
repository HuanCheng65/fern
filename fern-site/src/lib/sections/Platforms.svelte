<script>
  import OsMark from '$lib/OsMark.svelte';
  import { reveal } from '$lib/scroll.js';

  /*
   * 跨平台。
   *
   * 这里只说「三个系统都是同一个 Fern」。包格式、架构那些是**下载**的事，写在这儿
   * 会让人以为这一块能点下去——真正能下载的是页尾那一块，信息就该长在那里。
   *
   * 所以这一栏只有标志和名字。撑住它的是排印和留白，不是往里塞字。
   */
  const OS = ['windows', 'macos', 'linux'];
  const NAME = { windows: 'Windows', macos: 'macOS', linux: 'Linux' };
</script>

<section id="platforms">
  <div class="wrap">
    <div class="head" use:reveal>
      <div class="eyebrow">跨平台</div>
      <h2>到哪，都一样顺手。</h2>
    </div>

    <ul class="grid">
      {#each OS as os, i (os)}
        <li use:reveal={{ delay: i * 100 }}>
          <span class="mark"><OsMark {os} size={34} /></span>
          <p class="name">{NAME[os]}</p>
        </li>
      {/each}
    </ul>

    <p class="tail" use:reveal>
      <span>相同的设计语言，相同的操作逻辑。换一台机器，不用重新学。</span>
    </p>
  </div>
</section>

<style>
  .head h2 {
    margin-top: 14px;
  }

  /*
   * 三栏，中间用竖发丝线隔开。
   *
   * 上一版是三个名字挤在一行 flex 里、底下一条横线——那读起来是一句话被拆成了三段，
   * 而这里三个系统是并列的三件事，各有各的包格式和架构。竖线比横线更能说这件事。
   */
  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    margin: clamp(50px, 7vw, 90px) 0 clamp(34px, 4.6vw, 58px);
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .grid li {
    padding: clamp(30px, 3.6vw, 48px) clamp(16px, 2.4vw, 32px) clamp(4px, 1vw, 10px) 0;
  }
  /* 线在栏与栏之间，不在两头——第一栏左边和最后一栏右边都不该有边。 */
  .grid li + li {
    padding-left: clamp(20px, 3vw, 40px);
    border-left: 1px solid var(--line);
  }

  .mark {
    display: block;
    color: var(--ink);
    opacity: 0.85;
  }

  .name {
    margin-top: clamp(20px, 2.6vw, 32px);
    font-size: clamp(26px, 3.6vw, 44px);
    font-weight: 650;
    letter-spacing: -0.025em;
    line-height: 1.1;
  }

  /* 线走整幅，字只占一栏——线短一截会像是把这句话框了起来。 */
  .tail {
    padding-top: clamp(30px, 4vw, 46px);
    border-top: 1px solid var(--line);
    font-size: 17px;
    color: var(--mut);
  }
  .tail span {
    display: block;
    max-width: 46ch;
    /* 两行分匀，别让最后剩一个字吊在第二行。 */
    text-wrap: balance;
  }

  @media (max-width: 720px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .grid li {
      padding: clamp(22px, 6vw, 30px) 0;
    }
    .grid li + li {
      padding-left: 0;
      border-left: 0;
      border-top: 1px solid var(--line);
    }
  }
</style>
