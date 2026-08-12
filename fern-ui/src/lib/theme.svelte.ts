/**
 * 外观（见 docs/frond-design-system.md）。
 *
 * 玩家能动的东西全部收在一个对象里，序列化出来就是一份「主题包」——
 * 几百字节的 JSON，可分享、可回滚、可复现。这是文档里那条贯穿性技术哲学
 * 在这一版的最小落地：不做可视化编辑器，先把状态做成声明式的。
 *
 * 存在数据目录的 settings.json 里（见 lib/persist.ts），身份和网络偏好各存
 * 各的——主题码是拿来分享的，不该带上用户名。
 *
 * CSS 变量写在设计系统的作用域元素上（见 fern-kit/src/scope.ts）。背景层每隔
 * 几秒会把算出来的色板刷进同一个元素（那是支点规则，界面向背景学色彩），
 * 两边写的是不同的变量名——它交色板（--c*），这里交主题层（--accent 等，
 * 默认转发给色板）。所以玩家锁定强调色只要把转发那几行改掉，不必去动背景层。
 */

import { host } from 'fern-kit/host'
import { patch, snapshot } from './persist'
import { scopeRoot } from 'fern-kit/scope'

export type AccentMode = 'biome' | 'locked'
export type Density = 'compact' | 'default' | 'roomy'
export type Radius = 'sharp' | 'default' | 'round'
export type Motion = 'full' | 'reduced' | 'off'

export interface Theme {
  /** biome：跟着背景的群系走。locked：用下面这个颜色。 */
  accentMode: AccentMode
  accent: string
  density: Density
  radius: Radius
  motion: Motion
}

/**
 * 强调色预设直接用群系色板的高光段，换背景也不会脱节。
 *
 * 头一个是品牌色本身（见 docs/fern-brand-system.html 03）——它是「我不要跟着
 * 背景走，我要这个启动器本来的样子」这个选择的落点。默认档仍然是跟随背景：
 * 出厂时 UI 向背景学色彩，那是这套设计的根。
 */
export const ACCENT_PRESETS: { key: string; name: string; value: string }[] = [
  { key: 'sprout', name: '嫩芽', value: '#bfe4b2' },
  { key: 'ocean', name: '深海', value: '#bce6dc' },
  { key: 'jungle', name: '丛林', value: '#e0ebc2' },
  { key: 'nether', name: '下界', value: '#efb44c' },
  { key: 'end', name: '末地', value: '#dad6a6' },
  { key: 'badland', name: '恶地', value: '#e9c68e' },
  { key: 'snowy', name: '雪原', value: '#e2ecf3' },
  { key: 'swamp', name: '沼泽', value: '#adb27a' },
  { key: 'cherry', name: '樱花', value: '#f2d8d6' },
]

const DENSITY_SCALE: Record<Density, number> = { compact: 0.82, default: 1, roomy: 1.2 }
const RADIUS_SCALE: Record<Radius, number> = { sharp: 0.15, default: 1, round: 1.7 }
const MOTION_SCALE: Record<Motion, number> = { full: 1, reduced: 0.65, off: 0 }

const DEFAULTS: Theme = {
  accentMode: 'biome',
  accent: '#bce6dc',
  density: 'default',
  radius: 'default',
  motion: 'full',
}

/** 只认识的字段才收下。设置文件是用户能手改的，读进来必须当外部输入。 */
function sanitize(raw: unknown): Theme {
  const next = { ...DEFAULTS }
  if (!raw || typeof raw !== 'object') return next
  const value = raw as Partial<Theme>
  if (value.accentMode === 'biome' || value.accentMode === 'locked') next.accentMode = value.accentMode
  if (typeof value.accent === 'string' && /^#[0-9a-f]{6}$/i.test(value.accent)) next.accent = value.accent
  if (value.density && value.density in DENSITY_SCALE) next.density = value.density
  if (value.radius && value.radius in RADIUS_SCALE) next.radius = value.radius
  if (value.motion && value.motion in MOTION_SCALE) next.motion = value.motion
  return next
}

const hex2rgb = (hex: string): [number, number, number] => {
  const value = hex.replace('#', '')
  const full = value.length === 3 ? [...value].map((c) => c + c).join('') : value
  const n = Number.parseInt(full.slice(0, 6) || '000000', 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

/** 强调色上压什么颜色的字，由亮度决定——锁定的颜色可深可浅，不能写死。 */
function inkOn(hex: string) {
  const [r, g, b] = hex2rgb(hex)
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255 > 0.55 ? '#0a0f12' : '#f3f6f6'
}

class ThemeStore {
  #theme = $state<Theme>({ ...DEFAULTS })

  get accentMode() {
    return this.#theme.accentMode
  }
  get accent() {
    return this.#theme.accent
  }
  get density() {
    return this.#theme.density
  }
  get radius() {
    return this.#theme.radius
  }
  get motion() {
    return this.#theme.motion
  }

  /** 给 JS 动画（Svelte transition）用的时长，和 CSS 里的 --motion 同一把尺。 */
  get motionScale() {
    return MOTION_SCALE[this.#theme.motion]
  }

  /** 特效开关跟着动效档位走，不再单开一个设置项去解释它和动效的关系。 */
  get particles() {
    return this.#theme.motion === 'full'
  }
  get parallax() {
    return this.#theme.motion !== 'off'
  }

  /** 从磁盘读到的设置装进来。App 启动时调一次。 */
  hydrate() {
    this.#theme = sanitize(snapshot().appearance)
    this.apply()
  }

  set<K extends keyof Theme>(key: K, value: Theme[K]) {
    if (this.#theme[key] === value) return
    this.#theme = { ...this.#theme, [key]: value }
    this.store()
  }

  /** 主题包：导出一段可以贴给别人的文本。 */
  export(): string {
    return btoa(
      String.fromCharCode(...new TextEncoder().encode(JSON.stringify(this.#theme))),
    ).replace(/=+$/, '')
  }

  import(code: string): boolean {
    try {
      const trimmed = code.trim().replace(/\s+/g, '')
      const json = new TextDecoder().decode(
        Uint8Array.from(
          atob(trimmed + '='.repeat((4 - (trimmed.length % 4)) % 4)),
          (character) => character.charCodeAt(0),
        ),
      )
      this.#theme = sanitize(JSON.parse(json))
      this.store()
      return true
    } catch {
      return false
    }
  }

  reset() {
    this.#theme = { ...DEFAULTS }
    this.store()
  }

  private store() {
    const theme = this.#theme
    patch((doc) => (doc.appearance = { ...theme }))
    this.apply()
  }

  /** 把主题写成 CSS 变量。整个界面只从变量取值，所以改动是立刻全局的。 */
  apply() {
    const theme = this.#theme
    const style = scopeRoot().style
    style.setProperty('--density', String(DENSITY_SCALE[theme.density]))
    style.setProperty('--radius', String(RADIUS_SCALE[theme.radius]))
    style.setProperty('--motion', String(MOTION_SCALE[theme.motion]))
    // fern-kit 里的组件（浮层、标志）也要认这个档位，但它们读不到这个 store。
    host.motionScale = MOTION_SCALE[theme.motion]
    if (theme.accentMode === 'locked') {
      const [r, g, b] = hex2rgb(theme.accent)
      style.setProperty('--accent', theme.accent)
      style.setProperty('--accent-ink', inkOn(theme.accent))
      style.setProperty('--accent-soft', `rgba(${r}, ${g}, ${b}, 0.34)`)
    } else {
      // 转发给背景层交出的色板。
      style.setProperty('--accent', 'var(--c4)')
      style.setProperty('--accent-ink', 'var(--on-accent)')
      style.setProperty('--accent-soft', 'var(--accent-glow)')
    }
  }
}

export const theme = new ThemeStore()
