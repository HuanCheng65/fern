/**
 * 强调色的可读带。
 *
 * 强调色向背景学（支点规则），但背景是会变的：下界、恶地这类深色群系，加上
 * 深夜环境光把明度压到 0.56 倍，色板交出来的最亮一格会沉成一块泥色——压在上
 * 面的字、按它画的按钮和选中态一起发糊。「个性化不得破坏可读性」，所以色相照
 * 学，明度和彩度只许落在一个带里：
 *
 *   明度 L 0.78–0.87   深底上的强调色要亮，但不能亮到和正文白字分不开
 *   彩度 C 0.10–0.15   低于它就成了灰，高于它在深底上刺眼
 *
 * 在 OKLCH 里夹，是因为它的 L 就是感知明度：同一个 L，蓝和黄看上去一样亮。
 * 按 sRGB 通道去夹会把色相一起拧歪。
 */

import type { RGB } from './biome'

const L_MIN = 0.78
const L_MAX = 0.87
const C_MIN = 0.1
const C_MAX = 0.15
/**
 * 低于这个彩度的输入没有可信的色相，不替它编一个。
 *
 * 要压得很低：深夜的环境光会把色板褪到 C 0.01 上下，那时色相仍然是群系自己
 * 的——正该把它的彩度找回来。
 */
const C_HUELESS = 0.003

type Lab = [number, number, number]

const toLinear = (v: number) => {
  const c = v / 255
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}

const fromLinear = (c: number) =>
  255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055)

function toOklab([r, g, b]: RGB): Lab {
  const R = toLinear(r)
  const G = toLinear(g)
  const B = toLinear(b)
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B)
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B)
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

/** 线性 sRGB。可能越出 0–1，调用方据此判断在不在色域里。 */
function oklabToLinear([L, a, b]: Lab): Lab {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const inGamut = (c: Lab) => c.every((v) => v >= -1e-4 && v <= 1 + 1e-4)

const toRgb = (c: Lab): RGB =>
  c.map((v) => Math.round(fromLinear(Math.min(1, Math.max(0, v))))) as RGB

/** OKLCH 的明度和彩度。测试和读数用。 */
export function lightnessChroma(c: RGB): { l: number; c: number } {
  const [L, a, b] = toOklab(c)
  return { l: L, c: Math.hypot(a, b) }
}

/**
 * 把一个颜色夹进强调色的可读带，色相不动。
 *
 * 高明度的蓝紫在 sRGB 里给不出 0.10 的彩度。那时降彩度找回色域，而不是把越界
 * 的通道截掉——截通道会让色相跑偏。
 */
export function clampAccent(color: RGB): RGB {
  const [L0, a, b] = toOklab(color)
  const C0 = Math.hypot(a, b)
  const L = Math.min(L_MAX, Math.max(L_MIN, L0))
  if (C0 < C_HUELESS) return toRgb(oklabToLinear([L, a, b]))

  const h = Math.atan2(b, a)
  const at = (C: number) => oklabToLinear([L, C * Math.cos(h), C * Math.sin(h)])
  let lo = 0
  let hi = Math.min(C_MAX, Math.max(C_MIN, C0))
  if (inGamut(at(hi))) return toRgb(at(hi))
  // 二分找色域边界上的最大彩度。
  for (let i = 0; i < 16; i++) {
    const mid = (lo + hi) / 2
    if (inGamut(at(mid))) lo = mid
    else hi = mid
  }
  return toRgb(at(lo))
}
