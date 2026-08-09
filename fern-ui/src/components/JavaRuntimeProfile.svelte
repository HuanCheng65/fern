<script lang="ts">
  /**
   * 一份 Java 安装的档案。设置里的二级页。
   *
   * 和账户档案是同一套机制（`nav.focus` 的第三段），理由也是同一条：**一份运行时
   * 不是一个值。** 它有完整版本号、发行版、架构、是套件还是运行时、装在哪、占多
   * 大、从哪儿来的，还有「哪些实例会用到它」和一个删除。全塞进名单的一行里，就
   * 只能塞成一段就地展开的东西——而那会把下面所有的行往下顶一截。
   *
   * 名单因此可以安静下来：一行只说「Java 21 · Adoptium · 180 MB ›」。
   */
  import { javaLabel, megabytes, type JavaGroup, type JavaRuntime } from '../lib/java'

  interface Props {
    home: string
    groups: JavaGroup[]
    /** 删掉或取消登记之后，名单要重读。 */
    onchanged: () => void
    /** 这一份没了，这一页也就没有内容了。 */
    ongone: () => void
    remove: (home: string) => Promise<void>
    forget: (home: string) => Promise<void>
  }

  let { home, groups, onchanged, ongone, remove, forget }: Props = $props()

  const group = $derived(groups.find((item) => item.runtimes.some((rt) => rt.home === home)))
  const runtime = $derived<JavaRuntime | undefined>(
    group?.runtimes.find((item) => item.home === home),
  )
  /**
   * 会用到它的实例。
   *
   * 只有这一组里「自动会选中」的那一份才真的会被用上——同一个大版本装了三份，
   * 另外两份其实没人在用，而那正是「能不能删」的答案。
   */
  const used = $derived(group?.preferred === home ? (group?.requiredBy ?? []) : [])

  let confirming = $state(false)

  async function drop() {
    if (!runtime) return
    await (runtime.managed ? remove(home) : forget(home))
    onchanged()
    ongone()
  }
</script>

{#if !runtime}
  <p class="t-quiet">这一份 Java 已经不在名单里了。</p>
{:else}
  <div class="profile">
    <!-- 版本号已经是这一页的标题（二级页的头部会写），不再重复一遍。 -->
    <p class="kind t-quiet">{javaLabel(runtime)}</p>

    <dl class="facts">
      <div><dt>大版本</dt><dd class="t-mono">Java {runtime.major}</dd></div>
      <div>
        <dt>架构</dt>
        <dd class="t-mono">
          {runtime.arch}{#if !runtime.native}<span class="warn">（与本机不一致，性能会下降）</span>{/if}
        </dd>
      </div>
      <div><dt>位置</dt><dd class="t-mono selectable path">{runtime.home}</dd></div>
      {#if runtime.sizeBytes > 0}
        <div><dt>占用</dt><dd class="t-mono">{megabytes(runtime.sizeBytes)}</dd></div>
      {/if}
    </dl>

    <!--
      「谁在用它」必须说。删掉一份三个实例正靠着的运行时，那三个实例下次启动
      会重新下载一遍——这件事该在按下删除之前就看得见。
    -->
    <section>
      <h3>会用到它的实例</h3>
      {#if used.length === 0}
        <p class="t-quiet">
          {group && group.preferred !== home
            ? '同一大版本中另有一份优先选用，当前没有实例会用到这一份。'
            : '当前没有实例需要这个大版本。'}
        </p>
      {:else}
        <ul class="used">
          {#each used as name (name)}<li>{name}</li>{/each}
        </ul>
      {/if}
    </section>

    {#if runtime.managed || runtime.added}
      <section class="danger-zone">
        <div class="row">
          <span class="t-quiet">
            {#if confirming}
              {runtime.managed
                ? '这份运行时会从磁盘上删除，需要时会重新下载。'
                : '仅从名单中移除，磁盘上的文件保留。'}
            {:else}
              {runtime.managed ? '删除这份运行时' : '不再登记这个位置'}
            {/if}
          </span>
          {#if confirming}
            <span class="confirm">
              <button class="btn btn--ghost" onclick={() => (confirming = false)}>取消</button>
              <button class="btn danger" onclick={() => void drop()}>
                {runtime.managed ? '确认删除' : '确认移除'}
              </button>
            </span>
          {:else}
            <button class="btn btn--ghost" onclick={() => (confirming = true)}>
              {runtime.managed ? '删除' : '移除登记'}
            </button>
          {/if}
        </div>
      </section>
    {:else}
      <p class="t-quiet system">系统自带的安装不由 Fern 管理，需通过系统的包管理器卸载。</p>
    {/if}
  </div>
{/if}

<style>
  .profile {
    display: grid;
    gap: var(--s5);
  }

  .kind {
    margin: 0;
    font-size: var(--t-small);
  }

  .facts {
    display: grid;
    gap: var(--s3);
    margin: 0;
  }

  .facts div {
    display: grid;
    grid-template-columns: 5em 1fr;
    gap: var(--s3);
    align-items: baseline;
  }

  dt {
    color: var(--ink-3);
    font-size: var(--t-small);
  }

  dd {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  .path {
    overflow-wrap: anywhere;
  }

  .warn {
    color: var(--danger);
  }

  h3 {
    margin: 0 0 var(--s2);
    color: var(--ink-2);
    font-size: var(--t-small);
    font-weight: 500;
  }

  .used {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
    font-size: var(--t-small);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }

  .confirm {
    display: flex;
    gap: var(--s2);
  }

  /* 删除是这一页唯一不可撤销的动作，给它唯一的红。 */
  .btn.danger {
    color: #fff;
    background: #c42b1c;
  }

  .btn.danger:hover {
    background: #d8402f;
  }

  .system {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
    font-size: var(--t-small);
  }
</style>
