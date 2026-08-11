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
  import { FolderOpen, Plus } from 'lucide-svelte'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import InstanceCard from 'fern-kit/parts/InstanceCard.svelte'
  import Loading from '../components/Loading.svelte'
  import Collection from '../layouts/Collection.svelte'
  import InstanceDetail from './InstanceDetail.svelte'
  import NewInstance from './NewInstance.svelte'
  import { instances } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { expand, riseIn } from '../lib/motion'
  import { prefs } from '../lib/prefs.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

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
   *
   * 只有这一个「已有」入口：官方那一系和 Prism / MultiMC 由后端认，不该由
   * 用户先回答「我用的是哪个启动器」。
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
    <h1 class="t-h1">添加现有游戏</h1>
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
      <p class="note">创建一个实例之后，它的封面会出现在这里。已经有游戏目录的话，也可以直接添加——官方启动器和 Prism / MultiMC 的都认得。</p>
      <div class="ways">
        <Button variant="ghost" onclick={oncreate}><Plus size={15} />新建实例</Button>
        <Button variant="link" onclick={onadopt}>
          <FolderOpen size={14} strokeWidth={1.8} />添加现有目录
        </Button>
      </div>
    {/if}
    {#if instances.error}<div class="alert">{instances.error}</div>{/if}
  </section>
{:else}
  <Collection>
    {#snippet controls()}
      <span class="t-quiet">{instances.list.length} 个实例</span>
      <Button variant="link" onclick={oncreate}><Plus size={14} />新建实例</Button>
      <Button variant="link" onclick={onadopt}>
        <FolderOpen size={14} strokeWidth={1.8} />添加现有目录
      </Button>
    {/snippet}

    <div class="grid">
      {#each instances.recent as item, index (item.id)}
        <div in:riseIn={{ index }}>
          <InstanceCard
            name={item.name}
            cover={item.cover}
            detail={`${item.gameVersion} · ${item.loader}`}
            current={instances.current?.id === item.id}
            busy={launch.occupied(item.id)}
            onopen={() => nav.open(item.id)}
            onlaunch={() => void launch.launch(item.id)}
          />
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

  /* 列数跟着窗口走，不写断点。一张卡长什么样是 InstanceCard 的事。 */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: var(--s4);
    align-content: start;
  }

  .alert {
    margin-bottom: var(--s4);
  }
</style>
