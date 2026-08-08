<script lang="ts">
  /**
   * 实例场景——曲库。
   *
   * 启动场景是「正在播放」，这里是「曲库」：所有实例的生成封面排在一起，
   * 封面就是它们的脸。上一版是左列表右详情，那正是设计文档点名要避开的
   * SaaS 形状；网格让封面成为主视觉，也让这一屏有自己的主视觉。
   *
   * 卡片上只有封面、名称、版本与加载器，克制到此为止。以这个启动器面向的
   * 实例数量，搜索交给 ⌘K 就够，场景内不放搜索框。
   *
   * 两个动作要分清：点卡片是「看」（推入详情），悬停时那颗按钮是「玩」。
   * 「设为当前」在详情里——它会改变启动场景上摆着的是谁，不该是随手一点
   * 就发生的事。
   */
  import { FolderOpen, Play, Plus } from 'lucide-svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import Cover from 'fern-kit/Cover.svelte'
  import Loading from '../components/Loading.svelte'
  import Collection from '../layouts/Collection.svelte'
  import InstanceDetail from './InstanceDetail.svelte'
  import NewInstance from './NewInstance.svelte'
  import { instances } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { expand, riseIn } from '../lib/motion'
  import { prefs } from '../lib/prefs.svelte'

  /** 新建页和添加现有目录各占一个纵深位。实例 id 是随机发的，撞不上这两个词。 */
  const CREATE = 'new'
  const EXISTING = 'existing'

  const creating = $derived(nav.detail === CREATE)
  const adopting = $derived(nav.detail === EXISTING)
  const viewing = $derived(instances.list.find((item) => item.id === nav.detail))
  const oncreate = () => nav.open(CREATE)
  /**
   * 从零建一个，和把已有的接进来，产出的是同一种东西——一个实例。所以两个
   * 入口并排，而不是把后者藏进设置里：想让 Fern 用自己那个 .minecraft 的人，
   * 会在这一屏找它。
   */
  const onadopt = () => nav.open(EXISTING)

  // 地址里指着一个已经不存在的实例（删掉了、手改了地址栏）就退回网格，
  // 而不是留在一屏空白上。
  $effect(() => {
    if (nav.detail && !creating && !adopting && !instances.loading && !viewing) nav.back()
  })
</script>

{#if creating}
  <div class="depth" in:expand>
    <NewInstance />
  </div>
{:else if adopting}
  <div class="depth scroll existing" data-page-scroll in:expand>
    <h1 class="t-h1">现有游戏目录</h1>
    <AdoptDirectory />
  </div>
{:else if viewing}
  <!-- 往深处走是就地展开，不是横移——两种导航要能分得清。 -->
  <div class="depth" in:expand>
    <InstanceDetail instance={viewing} />
  </div>
{:else if instances.list.length === 0}
  <section class="blank">
    {#if instances.loading}
      <Loading note="读取实例" />
    {:else}
      <h1 class="t-h1">暂无实例</h1>
      <p class="note">创建一个实例之后，它的封面会出现在这里。已有 .minecraft 目录的话，也可以直接添加其中的版本。</p>
      <div class="ways">
        <button class="btn btn--ghost" onclick={oncreate}><Plus size={15} />新建实例</button>
        <button class="btn btn--link" onclick={onadopt}>
          <FolderOpen size={14} strokeWidth={1.8} />添加现有目录
        </button>
      </div>
    {/if}
    {#if instances.error}<div class="alert">{instances.error}</div>{/if}
  </section>
{:else}
  <Collection>
    {#snippet controls()}
      <span class="t-quiet">{instances.list.length} 个实例</span>
      <button class="btn btn--link" onclick={oncreate}><Plus size={14} />新建实例</button>
      <button class="btn btn--link" onclick={onadopt}>
        <FolderOpen size={14} strokeWidth={1.8} />添加现有目录
      </button>
    {/snippet}

    <div class="grid">
      {#each instances.recent as item, index (item.id)}
        <div class="card" class:on={instances.current?.id === item.id} in:riseIn={{ index }}>
          <button class="face" onclick={() => nav.open(item.id)} title="打开 {item.name}">
            <Cover seed={item.cover} quality={0.55} />
          </button>

          <!-- 「我就想立刻玩这个」的那条路径，不必先进详情。 -->
          <button
            class="go"
            aria-label="启动 {item.name}"
            title="启动"
            disabled={launch.occupied(item.id)}
            onclick={() => void launch.launch(item.id)}
          >
            <Play size={14} fill="currentColor" strokeWidth={0} />
          </button>

          <button class="text" onclick={() => nav.open(item.id)}>
            <strong>{item.name}</strong>
            <small class="t-mono">{item.gameVersion} · {item.loader}</small>
          </button>
        </div>
      {/each}
    </div>
  </Collection>
{/if}

<style>
  .depth {
    height: 100%;
    min-height: 0;
  }

  .existing {
    max-width: 640px;
    padding-bottom: var(--s8);
  }

  .existing h1 {
    margin: 0 0 var(--s5);
  }

  .ways {
    display: flex;
    align-items: center;
    gap: var(--s4);
  }

  .blank {
    display: grid;
    align-content: center;
    justify-items: start;
    gap: var(--s3);
    height: 100%;
    max-width: 46ch;
  }

  .note {
    margin: 0;
    color: var(--ink-3);
    font-size: var(--t-body);
    line-height: 1.65;
  }

  /* 列数跟着窗口走，不写断点。 */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: var(--s4);
    align-content: start;
  }

  .card {
    position: relative;
    display: grid;
    gap: var(--s2);
  }

  .face {
    display: block;
    width: 100%;
    aspect-ratio: 4 / 3;
    padding: 0;
    overflow: hidden;
    border-radius: var(--r2);
    background: var(--tint-1);
    transition:
      transform var(--t-base) var(--ease),
      box-shadow var(--t-base) var(--ease);
  }

  .card:hover .face {
    transform: translateY(-2px);
  }

  /* 当前实例只用一道描边标出来，不加角标——封面本身已经在说它是谁。 */
  .card.on .face {
    box-shadow: 0 0 0 1.5px var(--accent);
  }

  .go {
    position: absolute;
    top: var(--s2);
    right: var(--s2);
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: rgba(10, 14, 16, 0.6);
    color: #f3f6f6;
    opacity: 0;
    transform: scale(0.9);
    -webkit-backdrop-filter: blur(8px);
    backdrop-filter: blur(8px);
    transition:
      opacity var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .card:hover .go,
  .go:focus-visible {
    opacity: 1;
    transform: none;
  }

  .go:disabled {
    display: none;
  }

  .text {
    display: grid;
    gap: 1px;
    padding: 0;
    min-width: 0;
    text-align: left;
  }

  .text strong {
    overflow: hidden;
    color: var(--ink);
    font-size: var(--t-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }
</style>
