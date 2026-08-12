<script lang="ts">
  /**
   * 实例场景——曲库。
   *
   * 启动场景是「正在播放」，这里是「曲库」：所有实例的生成封面排在一起，
   * 封面就是它们的脸。上一版是左列表右详情，那正是设计文档点名要避开的
   * SaaS 形状；网格让封面成为主视觉，也让这一屏有自己的主视觉。
   *
   * 卡片上只有封面、名称、版本与加载器，克制到此为止。
   *
   * 搜索交给命令面板，但这一屏要有一个**看得见的入口**：⌘K 是老用户才知道的
   * 事，而三十张卡片铺开时，找一个实例是这一屏最常发生的动作。所以那一格长
   * 得像搜索框，按下去却是打开面板——它不是第二个搜索框，是同一个搜索的入口。
   *
   * 两个动作要分清：点卡片是「看」（推入详情），悬停时那颗按钮是「玩」。
   * 「设为当前」在详情里——它会改变启动场景上摆着的是谁，不该是随手一点
   * 就发生的事。
   */
  import { FolderOpen, Plus, Search } from 'lucide-svelte'
  import { palette } from 'fern-kit/parts/palette'
  import AdoptDirectory from '../components/AdoptDirectory.svelte'
  import InstanceCard from 'fern-kit/parts/InstanceCard.svelte'
  import Loading from '../components/Loading.svelte'
  import Collection from '../layouts/Collection.svelte'
  import InstanceDetail from './InstanceDetail.svelte'
  import NewInstance from './NewInstance.svelte'
  import { CREATE, EXISTING, instances } from '../lib/instances.svelte'
  import { launch } from '../lib/launch.svelte'
  import { nav } from '../lib/nav.svelte'
  import { platform } from '../lib/frame.svelte'
  import { expand, riseIn } from '../lib/motion'
  import { prefs } from '../lib/prefs.svelte'
  import Button from 'fern-kit/ui/Button.svelte'

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

  /** 面板带着「只找实例」这个范围打开，和点实例名呼出的切换器是同一个东西。 */
  const onfind = () => {
    palette.open({ kind: 'subjects', type: 'instance', label: '实例' })
    nav.show('palette')
  }
  const findKeys = platform === 'macos' ? '⌘K' : 'Ctrl K'

  /**
   * 这一屏叫什么，由它自己说。
   *
   * 页面标题和顶栏面包屑必须是同一个字符串——上一版是两处各写一遍，于是这
   * 一个页面同时有「添加现有游戏」「添加现有目录」「添加已有游戏」三个名字。
   */
  const ADOPT = '添加现有游戏'
  $effect(() => {
    if (adopting) nav.name(ADOPT)
  })

  // 地址里指着一个已经不存在的实例（删掉了、手改了地址栏）就退回网格，
  // 而不是留在一屏空白上。
  $effect(() => {
    if (nav.detail && !creating && !adopting && !instances.loading && !viewing) nav.up()
  })
</script>

{#if creating}
  <div class="depth" in:expand>
    <NewInstance />
  </div>
{:else if adopting}
  <div class="depth scroll existing" data-page-scroll in:expand>
    <h1 class="t-h1">{ADOPT}</h1>
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
          <FolderOpen size={14} strokeWidth={1.8} />添加现有游戏
        </Button>
      </div>
    {/if}
    {#if instances.error}<div class="alert">{instances.error}</div>{/if}
  </section>
{:else}
  <Collection>
    {#snippet controls()}
      <span class="t-quiet">{instances.list.length} 个实例</span>
      <div class="acts">
        <button class="find" onclick={onfind}>
          <Search size={13} strokeWidth={1.9} />
          <span>搜索实例</span>
          <kbd>{findKeys}</kbd>
        </button>
        <Button variant="link" onclick={oncreate}><Plus size={14} />新建实例</Button>
        <Button variant="link" onclick={onadopt}>
          <FolderOpen size={14} strokeWidth={1.8} />添加现有游戏
        </Button>
      </div>
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

  .acts {
    display: flex;
    align-items: center;
    gap: var(--s4);
  }

  /* 长得像一格搜索框，做的是「打开面板」——所以它不收键盘输入，也不留光标。 */
  .find {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 28px;
    padding: 0 var(--s2) 0 var(--s3);
    border-radius: var(--r1);
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
    color: var(--ink-4);
    font-size: var(--t-micro);
    transition: color var(--t-fast) var(--ease);
  }

  .find:hover {
    color: var(--ink-2);
  }

  .find span {
    padding-right: var(--s3);
  }

  .find kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 18px;
    padding: 0 5px;
    border-radius: 5px;
    background: var(--tint-1);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
    color: var(--ink-4);
    font-family: var(--mono);
    font-size: 10px;
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
