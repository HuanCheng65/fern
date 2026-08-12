/**
 * 群系：生成式背景。
 *
 * 从原型的封面生成器移植，算法逐行照搬——场、色带、色调曲线、环境光的
 * 参数都是调出来的，改一个数字就是另一张画。这里只做了三件事：拆成模块、
 * 补上类型、把画布从全局单例改成按调用者传入，好让缩略图和背景共用一套。
 *
 * 三层种子（见 docs/frond-design-system.md）：
 *   - 恒定种子：名字的哈希，决定构图骨架和色系，永不改变
 *   - 生长种子：累积时长，缓慢增加构图密度
 *   - 环境种子：真实时间，映射到昼夜循环，只动色温和明度
 *
 * 完全确定性：同样的种子生成同样的画。
 */

export interface Biome {
  name: string
  stops: [string, string, string, string, string]
}

/** 色板借群系的色彩记忆命名。 */
export const BIOMES = {
  ocean: { name: '深海', stops: ['#050D16', '#0A2436', '#12566E', '#3FA2AA', '#BCE6DC'] },
  jungle: { name: '丛林', stops: ['#08150E', '#14301E', '#2E5C35', '#82AC58', '#E0EBC2'] },
  nether: { name: '下界', stops: ['#110A0A', '#331312', '#76241B', '#C35C2D', '#EFB44C'] },
  end: { name: '末地', stops: ['#09070F', '#1D1630', '#3E2A5E', '#7E6CA8', '#DAD6A6'] },
  badland: { name: '恶地', stops: ['#150F0B', '#341F18', '#6D3921', '#B4723A', '#E9C68E'] },
  snowy: { name: '雪原', stops: ['#0A1017', '#1A2735', '#35526D', '#82A0B8', '#E2ECF3'] },
  swamp: { name: '沼泽', stops: ['#0A110C', '#162419', '#2E452F', '#617445', '#ADB27A'] },
  cherry: { name: '樱花', stops: ['#190E15', '#34202E', '#6D3A51', '#B67382', '#F2D8D6'] },
} as const satisfies Record<string, Biome>

export type BiomeKey = keyof typeof BIOMES
export const BIOME_KEYS = Object.keys(BIOMES) as BiomeKey[]

/** 构图模板，有限集合。 */
export const FIELDS = { stratus: '层云', vortex: '涡流', aurora: '极光' } as const
export type FieldKey = keyof typeof FIELDS
export const FIELD_KEYS = Object.keys(FIELDS) as FieldKey[]

export type RGB = [number, number, number]

const hash32 = (s: string): number => {
  let h = 2166136261 >>> 0
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

function hash2(x: number, y: number, seed: number): number {
  let n = x * 374761393 + y * 668265263 + seed * 69069
  n = (n ^ (n >>> 13)) >>> 0
  n = Math.imul(n, 1274126177) >>> 0
  return ((n ^ (n >>> 16)) >>> 0) / 4294967296
}

const smooth = (t: number) => t * t * (3 - 2 * t)

function vnoise(x: number, y: number, seed: number): number {
  const xi = Math.floor(x)
  const yi = Math.floor(y)
  const xf = x - xi
  const yf = y - yi
  const a = hash2(xi, yi, seed)
  const b = hash2(xi + 1, yi, seed)
  const c = hash2(xi, yi + 1, seed)
  const d = hash2(xi + 1, yi + 1, seed)
  const u = smooth(xf)
  const v = smooth(yf)
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

function fbm(x: number, y: number, seed: number, oct: number): number {
  let v = 0
  let amp = 0.5
  let f = 1
  let n = 0
  for (let i = 0; i < oct; i++) {
    v += amp * vnoise(x * f, y * f, seed + i * 131)
    n += amp
    amp *= 0.5
    f *= 2
  }
  return v / n
}

const hex2rgb = (h: string): RGB => [
  parseInt(h.slice(1, 3), 16),
  parseInt(h.slice(3, 5), 16),
  parseInt(h.slice(5, 7), 16),
]

function rampAt(st: RGB[], t: number): RGB {
  t = Math.min(0.9999, Math.max(0, t))
  const g = t * (st.length - 1)
  const i = Math.floor(g)
  const f = g - i
  const A = st[i]!
  const B = st[i + 1]!
  return [A[0] + (B[0] - A[0]) * f, A[1] + (B[1] - A[1]) * f, A[2] + (B[2] - A[2]) * f]
}

export interface Env {
  mul: number
  sat: number
  ta: number
  tint: RGB
}

/** 昼夜循环：清晨偏冷偏亮，正午最清透，黄昏染暖橙，深夜整体沉下去。 */
const ENV: (Env & { h: number })[] = [
  { h: 0, mul: 0.56, tint: [28, 44, 88], ta: 0.2, sat: 0.86 },
  { h: 5, mul: 0.76, tint: [118, 158, 212], ta: 0.13, sat: 0.92 },
  { h: 8, mul: 0.92, tint: [204, 222, 236], ta: 0.05, sat: 1.0 },
  { h: 13, mul: 1.0, tint: [255, 250, 236], ta: 0.04, sat: 1.03 },
  { h: 17, mul: 0.94, tint: [255, 148, 68], ta: 0.15, sat: 1.06 },
  { h: 20, mul: 0.7, tint: [66, 58, 112], ta: 0.16, sat: 0.94 },
  { h: 24, mul: 0.56, tint: [28, 44, 88], ta: 0.2, sat: 0.86 },
]

export function envAt(hr: number): Env {
  for (let i = 0; i < ENV.length - 1; i++) {
    const a = ENV[i]!
    const b = ENV[i + 1]!
    if (hr >= a.h && hr <= b.h) {
      const t = (hr - a.h) / (b.h - a.h)
      const L = (p: number, q: number) => p + (q - p) * t
      return {
        mul: L(a.mul, b.mul),
        sat: L(a.sat, b.sat),
        ta: L(a.ta, b.ta),
        tint: [
          L(a.tint[0], b.tint[0]),
          L(a.tint[1], b.tint[1]),
          L(a.tint[2], b.tint[2]),
        ],
      }
    }
  }
  return ENV[0]!
}

function applyEnv(c: RGB, e: Env): RGB {
  let r = c[0] * e.mul
  let g = c[1] * e.mul
  let b = c[2] * e.mul
  const l = r * 0.299 + g * 0.587 + b * 0.114
  r = l + (r - l) * e.sat
  g = l + (g - l) * e.sat
  b = l + (b - l) * e.sat
  r += (e.tint[0] - r) * e.ta
  g += (e.tint[1] - g) * e.ta
  b += (e.tint[2] - b) * e.ta
  const clamp = (v: number) => Math.max(0, Math.min(255, v))
  return [clamp(r), clamp(g), clamp(b)]
}

interface Field {
  data: Float32Array
  lo: number
  hi: number
}

const fieldCache = new Map<string, Field>()

function percentiles(a: Float32Array): { lo: number; hi: number } {
  const n = a.length
  let st = Math.max(1, (n / 4000) | 0)
  if (st % 2 === 0) st++
  const s: number[] = []
  for (let i = 0; i < n; i += st) s.push(a[i]!)
  s.sort((x, y) => x - y)
  const lo = s[(s.length * 0.01) | 0]!
  const hi = s[Math.min(s.length - 1, (s.length * 0.99) | 0)]!
  return { lo, hi: hi - lo < 1e-4 ? lo + 1e-4 : hi }
}

/**
 * 极光带的骨架常数,每条带六个数。
 *
 * 单独抽出来是给 GPU 路径用的:这些哈希的自变量是种子乘上大素数,乘积超过
 * 2^53,JS 在浮点里的舍入方式参与了结果——GPU 的整数运算不会舍入,重算一遍
 * 得到的是另一组带。所以骨架永远在 CPU 上算好,当 uniform 传过去;着色器里
 * 只算逐像素的部分,那部分的哈希自变量都很小,两边严格一致。
 */
export interface AuroraBand {
  y0: number
  slope: number
  amp: number
  freq: number
  wid: number
  str: number
}

export function auroraBands(seed: number, oct: number): AuroraBand[] {
  const nb = 2 + Math.floor(oct / 2)
  const bands: AuroraBand[] = []
  for (let k = 0; k < nb; k++) {
    bands.push({
      y0: 0.2 + 0.58 * hash2(seed + k * 17, 3, seed),
      slope: (hash2(seed + k * 31, 9, seed) - 0.5) * 0.4,
      amp: 0.05 + 0.11 * hash2(seed + k * 7, 5, seed),
      freq: 0.9 + 1.8 * hash2(seed + k * 13, 11, seed),
      wid: 0.03 + 0.055 * hash2(seed + k * 23, 13, seed),
      str: 0.55 + 0.45 * hash2(seed + k * 3, 7, seed),
    })
  }
  return bands
}

function scalarField(
  seed: number,
  fk: FieldKey,
  oct: number,
  warp: number,
  W: number,
  H: number,
  ph: number,
): Field {
  const key = `${seed}|${fk}|${oct}|${warp.toFixed(3)}|${W}x${H}|${ph.toFixed(2)}`
  const cached = fieldCache.get(key)
  if (cached) return cached

  const out = new Float32Array(W * H)
  let bands: { yc: Float32Array; wv: Float32Array; ray: Float32Array; str: number }[] = []

  if (fk === 'aurora') {
    for (const [k, band] of auroraBands(seed, oct).entries()) {
      const { y0, slope, amp, freq, wid, str } = band
      const yc = new Float32Array(W)
      const wv = new Float32Array(W)
      const ray = new Float32Array(W)
      for (let x = 0; x < W; x++) {
        const u = x / W
        yc[x] =
          y0 +
          slope * (u - 0.5) +
          (fbm(u * freq * 3 + k * 40 + ph * 0.7, 0.5 + ph * 0.3, seed + k * 97, Math.min(3, oct)) -
            0.5) *
            amp *
            2.4
        wv[x] = wid * (0.65 + 0.75 * fbm(u * 2.2 + k * 11 + ph * 0.5, 1.5, seed + k * 61, 2))
        ray[x] =
          0.35 + 0.95 * fbm(u * 13 + k * 5 + ph * 0.9, 2.5 + ph * 0.4, seed + k * 29, Math.min(3, oct))
      }
      bands.push({ yc, wv, ray, str })
    }
  }

  for (let y = 0; y < H; y++) {
    const ny = (y / H) * 2
    const vy = y / H
    for (let x = 0; x < W; x++) {
      const nx = (x / W) * 3
      let t: number
      if (fk === 'stratus') {
        const w1 = fbm(nx * 1.2 + 40 + ph * 0.55, ny * 1.2 + ph * 0.3, seed, oct)
        const w2 = fbm(nx * 2.6 + 90 + ph * 0.85, ny * 2.6 + 30 + ph * 0.45, seed + 53, Math.max(2, oct - 1))
        t = 1 - (vy + (w1 - 0.5) * warp + (w2 - 0.5) * 0.1)
      } else if (fk === 'vortex') {
        const wo = oct < 3 ? oct : 3
        const qx = fbm(nx * 1.1 + 11 + ph * 0.6, ny * 1.1 + 7 + ph * 0.35, seed + 17, wo)
        const qy = fbm(nx * 1.1 + 59 + ph * 0.4, ny * 1.1 + 23 + ph * 0.6, seed + 71, wo)
        const v = fbm(nx * 1.6 + (qx - 0.5) * warp * 7, ny * 1.6 + (qy - 0.5) * warp * 7, seed + 5, oct)
        t = 0.52 * v + 0.48 * (1 - vy)
      } else {
        t = (1 - vy) * 0.15
        for (let k = 0; k < bands.length; k++) {
          const b = bands[k]!
          const dy = vy - b.yc[x]!
          const w = dy < 0 ? b.wv[x]! * 2.6 : b.wv[x]!
          const d = dy / w
          t += (Math.exp(-d * d) * b.ray[x]! + Math.exp(-d * d * 0.14) * 0.2) * b.str
        }
        t = 1 - Math.exp(-t * 1.35)
      }
      out[y * W + x] = t
    }
  }

  const pc = percentiles(out)
  const res: Field = { data: out, lo: pc.lo, hi: pc.hi }
  // The cache is what makes the breathing phase affordable; it is bounded
  // because every window size and every phase is a key of its own.
  if (fieldCache.size > 360) fieldCache.delete(fieldCache.keys().next().value!)
  fieldCache.set(key, res)
  return res
}

const SK = 0.68
const SS = 1.8
const SN = 1 - Math.exp(-SS)
const NORM_BLEND = 0.6
const NORM_TOP = 0.82
const NORM_FLOOR = 0.02

function toneCurve(u: number): number {
  u = u < 0 ? 0 : u > 1 ? 1 : u
  if (u > SK) {
    const x = (u - SK) / (1 - SK)
    u = SK + (1 - SK) * ((1 - Math.exp(-x * SS)) / SN)
  }
  return u
}

/** 生长种子：新实例疏朗，玩久了层次更密，像年轮。 */
const growthOf = (h: number) => Math.min(0.96, 1 - Math.exp(-h / 260))

export interface BiomeOptions {
  /** 恒定种子的来源。房间用房间码，实例用实例名。 */
  name: string
  /** 生长种子，累积小时数。 */
  hours?: number
  /** 环境种子，0–24。默认取当前时间。 */
  hour?: number
  /** 锁定群系，留空则由名字决定。 */
  biomeKey?: BiomeKey | ''
  /** 锁定构图，留空则由名字决定。 */
  fieldKey?: FieldKey | ''
}

export interface Resolved {
  seed: number
  bk: BiomeKey
  fk: FieldKey
  g: number
  oct: number
  warp: number
  tMax: number
  env: Env
}

export function resolve(o: BiomeOptions): Resolved {
  const seed = hash32(o.name || 'x')
  const bk = o.biomeKey || BIOME_KEYS[seed % BIOME_KEYS.length]!
  const fk = o.fieldKey || FIELD_KEYS[(seed >>> 11) % FIELD_KEYS.length]!
  const g = growthOf(o.hours ?? 0)
  return {
    seed,
    bk,
    fk,
    g,
    oct: 2 + Math.round(g * 3),
    warp: 0.2 + g * 0.3 + hash2(seed, 7, seed) * 0.1,
    tMax: 0.86,
    env: envAt(o.hour ?? new Date().getHours()),
  }
}

type CanvasSurface = HTMLCanvasElement | OffscreenCanvas
type SurfaceContext = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D

let scratch: CanvasSurface | null = null
let sctx: SurfaceContext | null = null

function createScratch(): CanvasSurface {
  if (typeof document !== 'undefined') return document.createElement('canvas')
  return new OffscreenCanvas(1, 1)
}

/**
 * 画一张。`q` 是场的分辨率倍率——场按低分辨率算完再放大，因为它本来就是
 * 大色块，没有细节可损失，而每个像素都要跑几次 fbm。
 */
export function paint(
  cv: CanvasSurface,
  o: BiomeOptions,
  ph = 0,
  q = 0.5,
): { r: Resolved; stops: RGB[] } {
  const W = cv.width
  const H = cv.height
  const fw = Math.max(24, Math.round(W * q))
  const fh = Math.max(14, Math.round(H * q))
  const r = resolve(o)
  const f = scalarField(r.seed, r.fk, r.oct, r.warp, fw, fh, ph)
  const stops = BIOMES[r.bk].stops.map(hex2rgb)

  const lut = new Uint32Array(256)
  for (let i = 0; i < 256; i++) {
    const c = applyEnv(rampAt(stops, toneCurve(i / 255) * r.tMax), r.env)
    lut[i] = (255 << 24) | (Math.round(c[2]) << 16) | (Math.round(c[1]) << 8) | Math.round(c[0])
  }

  if (!scratch) {
    scratch = createScratch()
    sctx = scratch.getContext('2d', { willReadFrequently: true }) as SurfaceContext | null
  }
  if (scratch.width !== fw || scratch.height !== fh) {
    scratch.width = fw
    scratch.height = fh
  }
  const img = sctx!.createImageData(fw, fh)
  const u32 = new Uint32Array(img.data.buffer)
  const data = f.data
  const lo = f.lo
  const span = f.hi - f.lo
  const n = fw * fh
  for (let i = 0; i < n; i++) {
    let t = data[i]!
    t = t < 0 ? 0 : t > 1 ? 1 : t
    let u = ((t - lo) / span) * NORM_TOP + NORM_FLOOR
    u = t + (u - t) * NORM_BLEND
    u = u < 0 ? 0 : u > 1 ? 1 : u
    u32[i] = lut[(u * 255) | 0]!
  }
  sctx!.putImageData(img, 0, 0)

  const ctx = cv.getContext('2d') as SurfaceContext
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(scratch, 0, 0, fw, fh, 0, 0, W, H)
  return { r, stops }
}

/** Render one worker-owned surface and transfer the finished pixels to the UI. */
export function paintBitmap(
  width: number,
  height: number,
  o: BiomeOptions,
  ph = 0,
  q = 0.5,
): ImageBitmap {
  const cv = new OffscreenCanvas(width, height)
  paint(cv, o, ph, q)
  return cv.transferToImageBitmap()
}

/**
 * 场的归一化区间,从一张很小的探针场里量出来。
 *
 * 1%/99% 分位数是全局统计,不是逐像素运算,搬不进片元着色器。原版的
 * `percentiles` 本来就是稀疏抽样(步进约四千个点),所以在一张 64×40 的
 * 小场上量,统计上就是同一件事,代价不到一毫秒——GPU 路径把量出来的
 * lo/hi 当 uniform 用。
 */
export function fieldRange(r: Resolved, ph: number): { lo: number; hi: number } {
  const f = scalarField(r.seed, r.fk, r.oct, r.warp, 64, 40, ph)
  return { lo: f.lo, hi: f.hi }
}

/** 群系的五段色停,解析成数值,给需要插值的调用者。 */
export function stopsOf(bk: BiomeKey): RGB[] {
  return BIOMES[bk].stops.map(hex2rgb)
}

/**
 * 背景交出的那份色板。
 *
 * 这是支点规则（见 docs/frond-design-system.md）：任何背景源都必须交出一份色板，
 * 导航、按钮、卡片全部跟着它走，UI 才会看起来像背景原生的一部分。
 *
 * 返回数值而不是字符串,因为换房间时色板要在两套颜色之间逐帧插值——
 * 格式化是最后一步,交给写 CSS 变量的那一行。
 */
export function paletteOf(stops: RGB[], env: Env, tMax: number): RGB[] {
  return [0, 0.25, 0.5, 0.78, 1].map((t) => applyEnv(rampAt(stops, toneCurve(t) * tMax), env))
}

/** 一次性的噪点贴图，铺在最上面压掉大色块的色带。 */
export function grainDataUrl(size = 110): string {
  const c = document.createElement('canvas')
  c.width = c.height = size
  const x = c.getContext('2d')!
  const im = x.createImageData(size, size)
  for (let i = 0; i < im.data.length; i += 4) {
    const v = Math.random() * 255
    im.data[i] = im.data[i + 1] = im.data[i + 2] = v
    im.data[i + 3] = 255
  }
  x.putImageData(im, 0, 0)
  return c.toDataURL()
}
