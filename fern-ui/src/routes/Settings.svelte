<script lang="ts">
  import { ArrowLeft, ArrowRight } from 'lucide-svelte'
  import { push } from 'svelte-spa-router'

  const sections = [
    { id: 'general', label: '常规', note: '启动器行为' },
    { id: 'appearance', label: '外观与动效', note: '环境和窗口' },
    { id: 'java', label: 'Java 与运行时', note: '版本和内存' },
    { id: 'account', label: '账户', note: '离线身份' },
    { id: 'downloads', label: '下载', note: '源和缓存' },
    { id: 'advanced', label: '高级', note: '诊断与数据' },
  ] as const
  type Section = (typeof sections)[number]['id']

  let section = $state<Section>('general')
  let accountName = $state(localStorage.getItem('fern.account.name') ?? 'FernPlayer')
  let reducedEffects = $state(localStorage.getItem('fern.effects.reduced') === '1')

  function sectionFromHash() {
    const value = window.location.hash.split('/')[2] as Section | undefined
    section = sections.some((item) => item.id === value) ? value! : 'general'
  }

  function updateEffects(value: boolean) {
    reducedEffects = value
    localStorage.setItem('fern.effects.reduced', value ? '1' : '0')
    window.dispatchEvent(new CustomEvent('fern-settings-change', { detail: { reducedEffects } }))
  }

  function updateAccount(value: string) {
    accountName = value
    localStorage.setItem('fern.account.name', value)
    window.dispatchEvent(new CustomEvent('fern-settings-change', { detail: { accountName } }))
  }

  $effect(() => {
    sectionFromHash()
    window.addEventListener('hashchange', sectionFromHash)
    return () => window.removeEventListener('hashchange', sectionFromHash)
  })
</script>

<section class="settings-page" aria-label="设置">
  <div class="settings-page-head">
    <button class="back-button" onclick={() => void push('/workspace')}><ArrowLeft size={16} />返回工作台</button>
    <div><p class="eyebrow">Fern / preferences</p><h1>设置</h1></div>
    <span class="settings-version">0.1.0</span>
  </div>
  <div class="settings-layout">
    <nav class="settings-nav" aria-label="设置分类">
      {#each sections as item (item.id)}
        <button class:active={section === item.id} onclick={() => void push(`/settings/${item.id}`)}><span>{item.label}</span><small>{item.note}</small><ArrowRight size={14} /></button>
      {/each}
    </nav>
    <div class="settings-content">
      {#if section === 'general'}
        <div class="settings-intro"><p class="eyebrow">常规</p><h2>把 Fern 调成你的节奏</h2><p>启动器会把实例配置保存在本机，打开后直接回到最近使用的世界。</p></div>
        <div class="settings-group"><label class="setting-row"><span><strong>启动后保持在后台</strong><small>游戏窗口关闭后保留 Fern 进程</small></span><input type="checkbox" /></label><label class="setting-row"><span><strong>启动时检查文件</strong><small>每次启动都校验版本文件和资源</small></span><input type="checkbox" checked /></label></div>
      {:else if section === 'appearance'}
        <div class="settings-intro"><p class="eyebrow">外观与动效</p><h2>让画面退后，让世界进来</h2><p>背景使用当前实例的群系色板。动效开关会同时影响粒子和指针视差。</p></div>
        <div class="settings-group"><label class="setting-row"><span><strong>环境粒子</strong><small>保留群系背景里的微小运动</small></span><input type="checkbox" checked={!reducedEffects} onchange={(event) => updateEffects(!(event.currentTarget as HTMLInputElement).checked)} /></label><label class="setting-row"><span><strong>指针视差</strong><small>背景跟随指针产生轻微深度</small></span><input type="checkbox" checked={!reducedEffects} onchange={(event) => updateEffects(!(event.currentTarget as HTMLInputElement).checked)} /></label></div>
      {:else if section === 'java'}
        <div class="settings-intro"><p class="eyebrow">Java 与运行时</p><h2>每个版本使用合适的 Java</h2><p>当前实例会优先使用系统 PATH 中的 Java。自定义路径和自动运行时下载将在这里配置。</p></div>
        <div class="settings-group"><div class="setting-static"><span>当前发现</span><strong>系统 Java</strong><small>启动时自动检测</small></div><div class="setting-static"><span>实例覆盖</span><strong>跟随版本要求</strong><small>Java 8 / 17 / 21</small></div></div>
      {:else if section === 'account'}
        <div class="settings-intro"><p class="eyebrow">账户</p><h2>离线身份</h2><p>这个名字会生成稳定的离线 UUID，用于本地世界和离线服务器。</p></div>
        <div class="settings-group"><label class="form-field"><span>玩家名称</span><input value={accountName} oninput={(event) => updateAccount((event.currentTarget as HTMLInputElement).value)} maxlength="16" /></label><div class="account-card"><span class="account-avatar">{accountName.slice(0, 2).toUpperCase()}</span><div><strong>{accountName}</strong><small>离线账户 · 本地身份</small></div></div></div>
      {:else if section === 'downloads'}
        <div class="settings-intro"><p class="eyebrow">下载</p><h2>文件补给从哪里来</h2><p>Fern 会在官方源和 BMCLAPI 镜像之间自动切换，完成的文件会留在本机缓存中。</p></div>
        <div class="settings-group"><div class="setting-static"><span>默认源</span><strong>官方 → BMCLAPI</strong><small>失败时自动切换</small></div><div class="setting-static"><span>并发任务</span><strong>64 个文件</strong><small>按网络情况调整</small></div></div>
      {:else}
        <div class="settings-intro"><p class="eyebrow">高级</p><h2>数据和诊断</h2><p>定位问题时，把下面的路径提供给 Fern 的诊断工具。</p></div>
        <div class="settings-group"><div class="setting-static"><span>数据目录</span><strong class="selectable">{localStorage.getItem('fern.data.root') ?? '由系统决定'}</strong><small>实例、资源和日志都存放在这里</small></div></div>
      {/if}
    </div>
  </div>
</section>

<style>
  .settings-page { position: fixed; inset: 0; z-index: 12; overflow: auto; padding: 11vh max(6vw, 32px) 8vh; animation: page-in 360ms cubic-bezier(.22,1,.36,1) both; }
  .settings-page-head { display: grid; grid-template-columns: 1fr auto 1fr; align-items: start; gap: 32px; max-width: 1180px; margin: 0 auto 48px; }
  .settings-page-head h1 { margin: 0; color: var(--ink); font-size: clamp(40px, 5vw, 64px); letter-spacing: -.06em; line-height: 1; text-align: center; }
  .eyebrow { margin: 0 0 12px; color: var(--ink-3); font: 11px/16px var(--mono); letter-spacing: .14em; text-transform: uppercase; }
  .back-button { display: inline-flex; align-items: center; gap: 8px; justify-self: start; padding: 8px 0; color: var(--ink-2); font-size: 12px; transition: color 200ms, transform 200ms; }
  .back-button:hover { color: var(--ink); transform: translateX(-2px); }
  .settings-version { justify-self: end; color: var(--ink-3); font: 10px var(--mono); }
  .settings-layout { display: grid; grid-template-columns: minmax(220px, .38fr) minmax(0, 1fr); gap: clamp(32px, 8vw, 120px); max-width: 1180px; margin: 0 auto; }
  .settings-nav { display: flex; flex-direction: column; gap: 4px; padding-top: 8px; }
  .settings-nav button { display: grid; grid-template-columns: 1fr auto; gap: 2px 12px; padding: 12px 16px; border-left: 2px solid transparent; border-radius: 0 8px 8px 0; color: var(--ink-2); text-align: left; transition: color 200ms, background 200ms, border-color 200ms; }
  .settings-nav button small { color: var(--ink-3); font: 10px var(--mono); }
  .settings-nav button:hover, .settings-nav button.active { border-color: var(--c4); color: var(--ink); background: rgba(255,255,255,.08); }
  .settings-content { min-width: 0; max-width: 640px; }
  .settings-intro { padding-bottom: 32px; border-bottom: 1px solid var(--line); }
  .settings-intro h2 { margin: 0; color: var(--ink); font-size: clamp(28px, 4vw, 46px); letter-spacing: -.05em; line-height: 1; text-wrap: balance; }
  .settings-intro > p:last-child { max-width: 54ch; margin: 16px 0 0; color: var(--ink-2); line-height: 1.75; }
  .settings-group { padding: 24px 0; border-bottom: 1px solid var(--line); }
  .setting-row { display: flex; justify-content: space-between; align-items: center; gap: 20px; padding: 16px 0; color: var(--ink-2); }
  .setting-row span { display: flex; flex-direction: column; gap: 3px; }
  .setting-row strong { color: var(--ink); font-size: 14px; font-weight: 600; }
  .setting-row small, .setting-static small { color: var(--ink-3); font: 10px/16px var(--mono); }
  .setting-row input { appearance: none; width: 32px; height: 18px; flex: none; border-radius: 999px; background: rgba(255,255,255,.14); position: relative; transition: background 200ms; }
  .setting-row input::before { content: ''; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 999px; background: var(--ink); transition: transform 200ms; }
  .setting-row input:checked { background: var(--c3); }
  .setting-row input:checked::before { transform: translateX(14px); }
  .setting-static { display: grid; gap: 4px; padding: 16px 0; border-bottom: 1px solid var(--line-2); }
  .setting-static:last-child { border-bottom: 0; }
  .setting-static > span { color: var(--ink-3); font: 10px var(--mono); text-transform: uppercase; letter-spacing: .12em; }
  .setting-static strong { color: var(--ink); font-size: 16px; font-weight: 600; overflow-wrap: anywhere; }
  .form-field { display: grid; gap: 8px; margin-bottom: 16px; color: var(--ink-2); font-size: 12px; }
  .form-field > span { color: var(--ink-3); font: 10px var(--mono); letter-spacing: .12em; text-transform: uppercase; }
  .form-field input { width: 100%; min-height: 42px; padding: 0 12px; border: 1px solid var(--line); border-radius: 9px; outline: 0; background: rgba(255,255,255,.06); color: var(--ink); }
  .form-field input:focus { border-color: var(--c4); }
  .account-card { display: flex; align-items: center; gap: 12px; padding: 12px; border-radius: 10px; background: rgba(255,255,255,.05); }
  .account-avatar { display: grid; place-items: center; width: 34px; height: 34px; border-radius: 10px; color: var(--on-accent); background: var(--c4); font: 11px var(--mono); }
  .account-card div { display: flex; flex-direction: column; }
  .account-card small { color: var(--ink-3); font: 10px var(--mono); }
  @keyframes page-in { from { opacity: 0; transform: translateY(10px); } }
  @media (max-width: 760px) {
    .settings-page { padding: 8vh 20px 8vh; }
    .settings-page-head { grid-template-columns: 1fr auto; gap: 16px; margin-bottom: 32px; }
    .settings-page-head > div { grid-column: 1 / -1; grid-row: 1; order: -1; }
    .settings-page-head h1 { text-align: left; }
    .settings-version { grid-column: 2; grid-row: 2; }
    .settings-layout { grid-template-columns: 1fr; gap: 20px; }
    .settings-nav { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .settings-nav button { border-left: 0; border-bottom: 2px solid transparent; border-radius: 8px; }
    .settings-nav button.active { border-color: var(--c4); }
  }
</style>
