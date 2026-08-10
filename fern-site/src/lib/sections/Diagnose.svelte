<script>
  import Advice from 'fern-kit/parts/Advice.svelte';
  import CrashPanel from 'fern-kit/parts/CrashPanel.svelte';
  import { reveal } from '$lib/scroll.js';

  /*
   * 两边都不是编的，连措辞都不是。
   *
   * 左边四条对应 preflight.rs 里真的会发出来的四种 finding，标题和正文是
   * `i18n/zh-CN.ts` 里那几行模板填好参数之后的样子；顺序也是真的——findings
   * 按 severity 排过，blocking 在前，warning 在后。
   *
   * 有没有那颗按钮不是这里决定的，是 `label()` 决定的。所以「需要另一个版本的
   * Java」这条虽然是 blocking、后端也给了 use-java 动作，界面上照样没有按钮：
   * 那个动作现在做不了，宁可只说一句诊断，也不摆一颗点了没反应的按钮。
   */
  const CHECKS = [
    {
      title: '缺少前置模组：Fabric API',
      detail: 'Sodium 需要 Fabric API，当前实例中没有安装。',
      tone: 'blocking',
      action: { kind: 'install-mod', query: 'Fabric API' }
    },
    {
      title: 'Rubidium 不适用于 Fabric',
      detail: '它是 Forge 的模组，而这个实例使用 Fabric。',
      tone: 'blocking',
      action: { kind: 'remove-mod', file: 'rubidium-0.7.1.jar' }
    },
    {
      title: 'Create 需要另一个版本的 Java',
      detail:
        '它声明的 Java 版本是 >=21，而这个实例会使用 Java 17。加载器会因此拒绝启动。可以在实例设置中更换 Java。',
      tone: 'blocking',
      action: { kind: 'use-java', major: 21 }
    },
    {
      title: 'Iris 可能不适配 1.21.1',
      detail: '它声明支持的版本是 1.20.1。',
      tone: 'warning'
    }
  ];

  /*
   * 右边这一份有出处：`fern-core/rules/fixtures/mixin-failure-named.txt`，
   * 仓库里那份用来锁规则的真实崩溃日志（Fabric，1.21.1，mixin 配置名 sodium）。
   *
   * 两条诊断同时命中也是真的，`rules.rs` 里有测试盯着：exact 的
   * `mixin-failure-named` 排前面，generic 的 `mixin-failure` 落到「次要的那几
   * 条」里。这一条后端没给动作，所以这块板子上没有按钮——就是它本来的样子。
   */
  const FOUND = [
    {
      id: 'mixin-failure-named',
      title: 'sodium 的修改没能应用',
      detail: 'sodium 要修改的游戏代码与它预期的不一致。通常是与另一个模组冲突，或者它不适配当前游戏版本。'
    },
    {
      id: 'mixin-failure',
      title: '模组之间冲突',
      detail:
        '有模组要修改的游戏代码与它预期的不一致。常见于两个模组修改了同一处，或某个模组不适配当前游戏版本。'
    }
  ];
  const SUSPECTS = [{ modId: 'sodium', name: 'Sodium', version: '0.6.5' }];
  /* 日志也照抄那份 fixture。默认折着，但折着的东西也不该是编的。 */
  const EXCERPT = `[22:14:07] [main/ERROR] [mixin]: Mixin apply failed
org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: @Inject annotation on renderLevel could not find any targets matching 'render' in net.minecraft.client.renderer.LevelRenderer. [PREINJECT Applicator Phase -> sodium.mixins.json:features.render.MixinLevelRenderer]
\tat org.spongepowered.asm.mixin.injection.struct.InjectionInfo.readAnnotation(InjectionInfo.java:434)`;
</script>

<section id="diagnose" class="diag">
  <div class="wrap">
    <div class="eyebrow" use:reveal>Fern 诊断</div>

    <h2>
      <span use:reveal>在问题发生之前，</span>
      <span use:reveal={{ threshold: 0.6, delay: 120 }}>也在问题发生之后。</span>
    </h2>

    <div class="pair">
      <div class="one" use:reveal>
        <div class="tick mono">之前</div>
        <p>启动之前，Fern 会检查版本、加载器、模组与依赖中的已知问题。</p>
        <!--
          inert：这几条是真组件，按钮也是真按钮，但站上没有实例可以修。让它
          彻底不可点也不可聚焦，比摆一颗点了没反应的按钮诚实。
        -->
        <div class="art checks fern fern-dark" inert>
          <p class="cap">启动前检查</p>
          {#each CHECKS as c}
            <Advice title={c.title} detail={c.detail} tone={c.tone} action={c.action} />
          {/each}
        </div>
      </div>

      <div class="one" use:reveal={{ delay: 90 }}>
        <div class="tick mono">之后</div>
        <p>
          游戏异常退出后，<strong>Fern 诊断</strong
          >会综合日志、崩溃报告与模组信息，帮助定位原因，并在可以处理时给出对应操作。
        </p>
        <div class="art crash fern fern-dark" inert>
          <CrashPanel
            found={FOUND}
            exit="退出码 1"
            suspects={SUSPECTS}
            reportPath="crash-reports/crash-2026-08-08_22.14.07-client.txt"
            excerpt={EXCERPT}
          />
        </div>
      </div>
    </div>
  </div>
</section>

<style>
  h2 {
    margin-top: 16px;
    max-width: 18ch;
  }
  h2 span {
    display: block;
  }
  h2 span:last-child {
    color: var(--mut);
  }

  .pair {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: clamp(28px, 5vw, 72px);
    margin-top: clamp(52px, 7vw, 96px);
  }
  .one {
    padding-top: 26px;
    border-top: 1px solid var(--line);
  }
  .tick {
    font-size: 11px;
    letter-spacing: 0.18em;
    color: var(--mut);
  }
  .one p {
    margin-top: 16px;
    color: var(--mut);
    font-size: 17px;
  }

  /* 这些组件是给深色界面画的，纸白上没有它们站的地方，所以各自带一块地面。 */
  .art {
    margin-top: 30px;
    background: var(--pine);
  }

  /*
   * 之前是一列还在往下走的检查，所以齐着栏宽、压着下沿裁开；
   * 之后是一件打断你的事，所以它是一块完整的板子，浮在纸上。
   * 两个时刻的形状不一样，图也就不该一样。
   */
  .checks {
    padding: 16px 16px 0;
    border-radius: 14px;
    max-height: 350px;
    overflow: hidden;
  }
  .cap {
    margin: 0 0 4px;
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.18em;
    color: rgba(246, 244, 236, 0.42);
  }

  .crash {
    padding-bottom: 22px;
    border-radius: 16px;
    box-shadow:
      0 1px 2px rgba(20, 32, 26, 0.12),
      0 30px 70px rgba(20, 32, 26, 0.2);
  }

  @media (max-width: 720px) {
    .pair {
      grid-template-columns: 1fr;
    }
  }
</style>
