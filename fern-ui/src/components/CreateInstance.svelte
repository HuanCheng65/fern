<script lang="ts">
  /**
   * 新建实例。
   *
   * 版本不用原生 `<select>`：Mojang 的清单有八百多个版本，下拉框在里面
   * 找一个版本是灾难，而且原生控件在深色界面里长得和别处完全不是一套。
   * 这里做成「正式版 / 快照」两档 + 搜索 + 一列可滚动的版本，行为和
   * 命令面板一致——同一个交互模型在启动器里只学一次。
   */
  import { Check, Plus, X } from 'lucide-svelte'
  import Overlay from './Overlay.svelte'
  import Choice from './Choice.svelte'
  import { instances } from '../lib/instances.svelte'

  interface Props {
    onclose: () => void
    /** 建好之后跳到哪里去，由调用方决定。 */
    oncreated: () => void
  }

  let { onclose, oncreated }: Props = $props()

  type Kind = 'release' | 'snapshot'

  let name = $state('')
  let kind = $state<Kind>('release')
  let query = $state('')
  let picked = $state('')
  let busy = $state(false)
  let error = $state('')

  const shown = $derived(
    instances.versions
      .filter((v) => (kind === 'release' ? v.kind === 'release' : v.kind !== 'release'))
      .filter((v) => v.id.toLowerCase().includes(query.trim().toLowerCase()))
      .slice(0, 400),
  )

  // 打开就默认选中最新正式版：绝大多数人要的就是它。
  $effect(() => {
    if (!picked && shown.length > 0) picked = shown[0]!.id
  })

  const day = (iso: string) => iso.slice(0, 10)

  async function submit() {
    const trimmed = name.trim()
    if (!trimmed) return (error = '给实例起个名字')
    if (!picked) return (error = '选择一个 Minecraft 版本')
    busy = true
    error = ''
    try {
      await instances.create(trimmed, picked)
      oncreated()
      onclose()
    } catch (cause) {
      error = String(cause)
    } finally {
      busy = false
    }
  }

  void instances.loadVersions()
</script>

<Overlay label="新建实例" width="460px" {onclose}>
  <header>
    <h2 class="t-h2">新建实例</h2>
    <button class="btn btn--icon" aria-label="关闭" onclick={onclose}><X size={16} /></button>
  </header>

  <div class="body">
    <div class="field">
      <label for="new-instance-name">名称</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="new-instance-name"
        class="input"
        bind:value={name}
        autofocus
        maxlength="64"
        placeholder="余烬谷"
        onkeydown={(event) => event.key === 'Enter' && void submit()}
      />
    </div>

    <div class="field">
      <div class="version-head">
        <label for="version-filter">版本</label>
        <Choice
          label="版本类型"
          value={kind}
          onchange={(next) => {
            kind = next
            picked = ''
          }}
          options={[
            { value: 'release', label: '正式版' },
            { value: 'snapshot', label: '快照' },
          ]}
        />
      </div>
      <input
        id="version-filter"
        class="input"
        bind:value={query}
        spellcheck="false"
        placeholder="筛选版本号"
      />
      <div class="versions scroll">
        {#if instances.versionsLoading}
          <p class="hint">正在读取版本清单</p>
        {:else if shown.length === 0}
          <p class="hint">没有匹配的版本</p>
        {:else}
          {#each shown as version (version.id)}
            <button
              class="version"
              class:on={picked === version.id}
              onclick={() => (picked = version.id)}
            >
              <span class="t-mono id">{version.id}</span>
              <span class="t-mono date">{day(version.releaseTime)}</span>
              {#if picked === version.id}<Check size={14} strokeWidth={2.4} />{/if}
            </button>
          {/each}
        {/if}
      </div>
    </div>

    {#if error}<div class="alert">{error}</div>{/if}
  </div>

  <footer>
    <button class="btn" onclick={onclose}>取消</button>
    <button class="btn btn--primary" disabled={busy} onclick={() => void submit()}>
      <Plus size={15} />{busy ? '创建中' : '创建实例'}
    </button>
  </footer>
</Overlay>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--s4) var(--s4) var(--s3) var(--s5);
  }

  .body {
    display: grid;
    gap: var(--s4);
    padding: 0 var(--s5) var(--s5);
    min-height: 0;
  }

  .version-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s4);
  }

  .version-head :global(.choice) {
    width: 150px;
  }

  .versions {
    max-height: 216px;
    padding: var(--s1);
    border-radius: var(--r1);
    background: rgba(0, 0, 0, 0.2);
    box-shadow: inset 0 0 0 1px var(--hairline-2);
  }

  .version {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: 7px var(--s3);
    border-radius: calc(var(--r1) * 0.8);
    color: var(--ink-2);
    text-align: left;
  }

  .version:hover {
    background: var(--tint-1);
  }

  .version.on {
    color: var(--ink);
    background: var(--tint-2);
  }

  .version .id {
    flex: 1;
    min-width: 0;
    font-size: var(--t-body);
  }

  .version .date {
    color: var(--ink-4);
    font-size: var(--t-micro);
  }

  .version :global(svg) {
    color: var(--accent);
  }

  .hint {
    margin: 0;
    padding: var(--s5) 0;
    color: var(--ink-4);
    font-size: var(--t-small);
    text-align: center;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s2);
    padding: var(--s3) var(--s5) var(--s4);
    box-shadow: inset 0 1px 0 var(--hairline-2);
  }
</style>
