<script lang="ts">
  import { ArrowRight } from 'lucide-svelte'
  import { push } from 'svelte-spa-router'

  let step: 'welcome' | 'profile' = 'welcome'
  let playerName = localStorage.getItem('fern.account.name') ?? ''
  let error = ''

  function continueToProfile() {
    step = 'profile'
    error = ''
  }

  function createProfile() {
    const value = playerName.trim()
    if (!/^[A-Za-z0-9_]{3,16}$/.test(value)) {
      error = '用户名需要 3–16 个字母、数字或下划线'
      return
    }
    localStorage.setItem('fern.account.name', value)
    localStorage.setItem('fern.landing.seen', '1')
    window.dispatchEvent(new CustomEvent('fern-settings-change', { detail: { accountName: value } }))
    void push('/workspace')
  }
</script>

<section class="landing-page" aria-label="Fern">
  <div class="landing-mark"><span></span></div>
  {#if step === 'welcome'}
    <div class="landing-copy">
      <p class="eyebrow">Fern</p>
      <h1>万千世界，<br /><em>一个入口。</em></h1>
      <p class="landing-lede">欢迎使用 Fern。几步设置之后，就可以出发了。</p>
      <button class="landing-enter" onclick={continueToProfile}>开始设置 <ArrowRight size={17} /></button>
    </div>
  {:else}
    <div class="landing-copy profile-step">
      <p class="eyebrow">账户设置</p>
      <h1>建立你的<br /><em>第一个档案</em></h1>
      <p class="landing-lede">请设置一个玩家名称。该名称将用于离线游戏，并作为后续账户验证的基础。</p>
      <label class="profile-field"><span>玩家名称</span><input bind:value={playerName} maxlength="16" autocomplete="nickname" placeholder="输入玩家名称" onkeydown={(event) => event.key === 'Enter' && createProfile()} /></label>
      {#if error}<p class="profile-error" role="alert">{error}</p>{/if}
      <div class="profile-actions"><button class="back-link" onclick={() => (step = 'welcome')}>上一步</button><button class="landing-enter" onclick={createProfile}>完成设置 <ArrowRight size={17} /></button></div>
    </div>
  {/if}
  <div class="landing-foot"><span>离线优先</span><span>·</span><span>本地实例</span><span>·</span><span>开放协议</span></div>
</section>

<style>
  .landing-page { position: fixed; inset: 0; z-index: 12; display: grid; grid-template-columns: minmax(90px, 1fr) minmax(300px, 1.1fr) minmax(170px, .75fr); align-items: center; gap: 5vw; padding: 10vh 8vw 7vh; }
  .landing-mark { align-self: start; width: 56px; height: 56px; margin-top: 3vh; border: 1px solid rgba(255,255,255,.25); border-radius: 16px; background: var(--c4); box-shadow: 0 18px 60px rgba(0,0,0,.24); position: relative; transform: rotate(-8deg); }
  .landing-mark span { position: absolute; inset: 15px; border-radius: 5px; background: var(--c0); opacity: .55; }
  .landing-copy { max-width: 620px; animation: landing-in 700ms cubic-bezier(.16,1.06,.28,1) both; }
  .eyebrow { margin: 0 0 12px; color: var(--ink-3); font: 11px/16px var(--mono); letter-spacing: .14em; text-transform: uppercase; }
  .landing-copy h1 { margin: 0; color: var(--ink); font-size: clamp(52px, 8vw, 104px); line-height: .92; letter-spacing: -.07em; text-wrap: balance; }
  .landing-copy h1 em { color: var(--c4); font-style: normal; }
  .landing-lede { max-width: 40ch; margin: 32px 0 48px; color: var(--ink-2); font-size: 15px; line-height: 1.8; }
  .landing-enter { display: inline-flex; align-items: center; gap: 12px; min-height: 50px; padding: 0 24px; border: 1px solid rgba(255,255,255,.18); border-radius: 12px; color: var(--on-accent); background: var(--c4); font-weight: 650; box-shadow: var(--shadow-1); transition: transform 200ms cubic-bezier(.22,1,.36,1), filter 200ms cubic-bezier(.22,1,.36,1); }
  .landing-enter:hover { filter: brightness(1.08); transform: translateY(-2px); }
  .landing-enter:active { transform: translateY(1px) scale(.98); }
  .profile-step { max-width: 580px; }
  .profile-field { display: grid; gap: 8px; max-width: 420px; margin: 32px 0 8px; }
  .profile-field span { color: var(--ink-3); font: 10px var(--mono); letter-spacing: .12em; text-transform: uppercase; }
  .profile-field input { min-height: 50px; padding: 0 16px; border: 1px solid var(--line); border-radius: 12px; outline: 0; background: rgba(10,14,16,.35); color: var(--ink); font-size: 16px; }
  .profile-field input:focus { border-color: var(--c4); box-shadow: 0 0 0 3px rgba(255,255,255,.05); }
  .profile-field input::placeholder { color: var(--ink-3); }
  .profile-error { margin: 8px 0 0; color: #efb2a5; font: 11px var(--mono); }
  .profile-actions { display: flex; align-items: center; gap: 20px; margin-top: 28px; }
  .back-link { padding: 8px 0; color: var(--ink-3); font-size: 12px; }
  .back-link:hover { color: var(--ink); }
  .landing-foot { align-self: end; justify-self: end; display: flex; gap: 8px; color: var(--ink-3); font: 10px var(--mono); letter-spacing: .08em; }
  @keyframes landing-in { from { opacity: 0; transform: translateY(18px); } }
  @media (max-width: 760px) {
    .landing-page { grid-template-columns: 1fr; align-content: center; gap: 24px; padding: 16vh 24px 10vh; }
    .landing-mark { margin-top: 0; }
    .landing-copy h1 { font-size: clamp(52px, 16vw, 78px); }
    .landing-foot { justify-self: start; align-self: auto; }
  }
</style>
