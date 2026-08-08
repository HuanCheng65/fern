/**
 * 群系背景的 GPU 路径。
 *
 * 场的数学几乎全是逐像素独立的,天生适合片元着色器:噪声、色带、色调曲线、
 * 环境光全部搬进 GPU,每帧实时算,不需要场缓存,窗口缩放也不再有重算成本。
 * 呼吸因此是真正连续的相位推进,不再靠两张图交叉淡入。
 *
 * 两个例外留在 CPU:
 *   - 归一化区间(1%/99% 分位数)是全局统计,由 `fieldRange` 在一张小探针场
 *     上量出来,当 uniform 传入;
 *   - 极光带骨架的哈希自变量超出 2^53,JS 的浮点舍入参与了结果,GPU 整数
 *     运算复现不了,由 `auroraBands` 算好传入。
 *
 * 着色器里剩下的哈希自变量都是小整数,uint 运算与 JS 的 int32 语义逐位一致,
 * 所以同一个种子在 GPU 和 CPU 上是同一张画。
 *
 * 换房间、换色板的连续动画也在这里:着色器常驻两套参数(from/to),每个
 * 像素各算一遍再按 blend 混合。稳态时 blend=1,from 那一遍被跳过。
 */

import {
  FIELD_KEYS,
  auroraBands,
  fieldRange,
  resolve,
  stopsOf,
  type AuroraBand,
  type BiomeOptions,
  type Env,
  type RGB,
  type Resolved,
} from 'fern-kit/biome'

/** 一套完整的画面参数:构图、色板、归一化区间。 */
export interface GlLayer {
  r: Resolved
  /** FIELD_KEYS 里的下标,着色器按它选构图分支。 */
  field: number
  stops: RGB[]
  bands: AuroraBand[]
  lo: number
  hi: number
}

export function layerOf(o: BiomeOptions, ph: number): GlLayer {
  const r = resolve(o)
  const range = fieldRange(r, ph)
  return {
    r,
    field: FIELD_KEYS.indexOf(r.fk),
    stops: stopsOf(r.bk),
    bands: r.fk === 'aurora' ? auroraBands(r.seed, r.oct) : [],
    lo: range.lo,
    hi: range.hi,
  }
}

export interface DrawState {
  from: GlLayer
  to: GlLayer
  /** 0–1,已经过缓动。1 表示稳态,只画 to。 */
  blend: number
  phase: number
  /** 环境光按当帧时刻算,from/to 共用——昼夜是连续量,不参与交叉淡化。 */
  env: Env
}

const VERT = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
`

const FRAG = `#version 300 es
precision highp float;
precision highp int;

uniform vec2 u_res;
uniform float u_phase;
uniform float u_blend;
uniform float u_envMul;
uniform float u_envSat;
uniform float u_envTa;
uniform vec3 u_envTint;

uniform uint u_seedA; uniform int u_fieldA; uniform int u_octA; uniform float u_warpA;
uniform float u_loA; uniform float u_hiA; uniform float u_tMaxA;
uniform vec3 u_stopsA[5];
uniform vec4 u_bandA[4]; uniform vec2 u_bandWsA[4]; uniform int u_nbA;

uniform uint u_seedB; uniform int u_fieldB; uniform int u_octB; uniform float u_warpB;
uniform float u_loB; uniform float u_hiB; uniform float u_tMaxB;
uniform vec3 u_stopsB[5];
uniform vec4 u_bandB[4]; uniform vec2 u_bandWsB[4]; uniform int u_nbB;

out vec4 fragColor;

// 与 biome.ts 的 hash2 逐位一致:JS 的位运算把和强转成 int32,这里的 uint
// 运算按 2^32 取模,两者同余。
float hash2(int x, int y, uint seed) {
  uint n = uint(x) * 374761393u + uint(y) * 668265263u + seed * 69069u;
  n = n ^ (n >> 13u);
  n = n * 1274126177u;
  n = n ^ (n >> 16u);
  return float(n) * (1.0 / 4294967296.0);
}

float vnoise(float x, float y, uint seed) {
  float fx = floor(x);
  float fy = floor(y);
  int xi = int(fx);
  int yi = int(fy);
  float xf = x - fx;
  float yf = y - fy;
  float a = hash2(xi, yi, seed);
  float b = hash2(xi + 1, yi, seed);
  float c = hash2(xi, yi + 1, seed);
  float d = hash2(xi + 1, yi + 1, seed);
  float u = xf * xf * (3.0 - 2.0 * xf);
  float v = yf * yf * (3.0 - 2.0 * yf);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

float fbm(float x, float y, uint seed, int oct) {
  float v = 0.0;
  float amp = 0.5;
  float f = 1.0;
  float n = 0.0;
  for (int i = 0; i < 5; i++) {
    if (i >= oct) break;
    v += amp * vnoise(x * f, y * f, seed + uint(i * 131));
    n += amp;
    amp *= 0.5;
    f *= 2.0;
  }
  return v / n;
}

float fieldAt(
  vec2 uv, uint seed, int field, int oct, float warp,
  vec4 band[4], vec2 bandWs[4], int nb
) {
  float u = uv.x;
  float vy = uv.y;
  float nx = u * 3.0;
  float ny = vy * 2.0;
  float ph = u_phase;
  if (field == 0) { // stratus
    float w1 = fbm(nx * 1.2 + 40.0 + ph * 0.55, ny * 1.2 + ph * 0.3, seed, oct);
    float w2 = fbm(nx * 2.6 + 90.0 + ph * 0.85, ny * 2.6 + 30.0 + ph * 0.45, seed + 53u, max(2, oct - 1));
    return 1.0 - (vy + (w1 - 0.5) * warp + (w2 - 0.5) * 0.1);
  }
  if (field == 1) { // vortex
    int wo = min(oct, 3);
    float qx = fbm(nx * 1.1 + 11.0 + ph * 0.6, ny * 1.1 + 7.0 + ph * 0.35, seed + 17u, wo);
    float qy = fbm(nx * 1.1 + 59.0 + ph * 0.4, ny * 1.1 + 23.0 + ph * 0.6, seed + 71u, wo);
    float v = fbm(nx * 1.6 + (qx - 0.5) * warp * 7.0, ny * 1.6 + (qy - 0.5) * warp * 7.0, seed + 5u, oct);
    return 0.52 * v + 0.48 * (1.0 - vy);
  }
  // aurora
  float t = (1.0 - vy) * 0.15;
  int o3 = min(3, oct);
  for (int k = 0; k < 4; k++) {
    if (k >= nb) break;
    float y0 = band[k].x;
    float slope = band[k].y;
    float amp = band[k].z;
    float freq = band[k].w;
    float wid = bandWs[k].x;
    float str = bandWs[k].y;
    float fk = float(k);
    float yc = y0 + slope * (u - 0.5)
      + (fbm(u * freq * 3.0 + fk * 40.0 + ph * 0.7, 0.5 + ph * 0.3, seed + uint(k * 97), o3) - 0.5) * amp * 2.4;
    float wv = wid * (0.65 + 0.75 * fbm(u * 2.2 + fk * 11.0 + ph * 0.5, 1.5, seed + uint(k * 61), 2));
    float ray = 0.35 + 0.95 * fbm(u * 13.0 + fk * 5.0 + ph * 0.9, 2.5 + ph * 0.4, seed + uint(k * 29), o3);
    float dy = vy - yc;
    float w = dy < 0.0 ? wv * 2.6 : wv;
    float d = dy / w;
    t += (exp(-d * d) * ray + exp(-d * d * 0.14) * 0.2) * str;
  }
  return 1.0 - exp(-t * 1.35);
}

const float SK = 0.68;
const float SS = 1.8;

float toneCurve(float u) {
  u = clamp(u, 0.0, 1.0);
  if (u > SK) {
    float x = (u - SK) / (1.0 - SK);
    u = SK + (1.0 - SK) * ((1.0 - exp(-x * SS)) / (1.0 - exp(-SS)));
  }
  return u;
}

vec3 rampAt(vec3 st[5], float t) {
  t = clamp(t, 0.0, 0.9999);
  float g = t * 4.0;
  int i = int(g);
  return mix(st[i], st[i + 1], g - float(i));
}

vec3 applyEnv(vec3 c) {
  c *= u_envMul;
  float l = dot(c, vec3(0.299, 0.587, 0.114));
  c = vec3(l) + (c - vec3(l)) * u_envSat;
  c += (u_envTint - c) * u_envTa;
  return clamp(c, 0.0, 1.0);
}

vec3 shade(
  vec2 uv, uint seed, int field, int oct, float warp,
  float lo, float hi, float tMax, vec3 st[5],
  vec4 band[4], vec2 bandWs[4], int nb
) {
  float t = clamp(fieldAt(uv, seed, field, oct, warp, band, bandWs, nb), 0.0, 1.0);
  float u = ((t - lo) / (hi - lo)) * 0.82 + 0.02;
  u = t + (u - t) * 0.6;
  u = clamp(u, 0.0, 1.0);
  return applyEnv(rampAt(st, toneCurve(u) * tMax));
}

void main() {
  vec2 uv = vec2(gl_FragCoord.x / u_res.x, 1.0 - gl_FragCoord.y / u_res.y);
  vec3 col = shade(uv, u_seedB, u_fieldB, u_octB, u_warpB,
                   u_loB, u_hiB, u_tMaxB, u_stopsB, u_bandB, u_bandWsB, u_nbB);
  if (u_blend < 0.999) {
    vec3 from = shade(uv, u_seedA, u_fieldA, u_octA, u_warpA,
                      u_loA, u_hiA, u_tMaxA, u_stopsA, u_bandA, u_bandWsA, u_nbA);
    col = mix(from, col, u_blend);
  }
  fragColor = vec4(col, 1.0);
}
`

export interface BackdropGl {
  draw(state: DrawState): void
  resize(width: number, height: number): void
  dispose(): void
}

function compile(gl: WebGL2RenderingContext, kind: number, source: string): WebGLShader | null {
  const shader = gl.createShader(kind)
  if (!shader) return null
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.warn('biome shader failed to compile:', gl.getShaderInfoLog(shader))
    gl.deleteShader(shader)
    return null
  }
  return shader
}

/** 建不出 WebGL2 就返回 null,调用者退回 CPU 路径。 */
export function createBackdropGl(canvas: HTMLCanvasElement): BackdropGl | null {
  const context = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    // 背景可以慢一帧,不能抢游戏的电:明确告诉合成器这不是性能敏感层。
    powerPreference: 'low-power',
  })
  if (!context) return null
  // 固化成非空类型:下面的闭包在守卫之外执行,TS 的收窄跟不进去。
  const gl: WebGL2RenderingContext = context

  const vert = compile(gl, gl.VERTEX_SHADER, VERT)
  const frag = compile(gl, gl.FRAGMENT_SHADER, FRAG)
  if (!vert || !frag) return null
  const program = gl.createProgram()
  gl.attachShader(program, vert)
  gl.attachShader(program, frag)
  gl.linkProgram(program)
  gl.deleteShader(vert)
  gl.deleteShader(frag)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.warn('biome shader failed to link:', gl.getProgramInfoLog(program))
    gl.deleteProgram(program)
    return null
  }
  gl.useProgram(program)

  const at = (name: string) => gl.getUniformLocation(program, name)
  const res = at('u_res')
  const phase = at('u_phase')
  const blend = at('u_blend')
  const envMul = at('u_envMul')
  const envSat = at('u_envSat')
  const envTa = at('u_envTa')
  const envTint = at('u_envTint')

  const layerAt = (suffix: 'A' | 'B') => ({
    seed: at(`u_seed${suffix}`),
    field: at(`u_field${suffix}`),
    oct: at(`u_oct${suffix}`),
    warp: at(`u_warp${suffix}`),
    lo: at(`u_lo${suffix}`),
    hi: at(`u_hi${suffix}`),
    tMax: at(`u_tMax${suffix}`),
    stops: at(`u_stops${suffix}[0]`),
    band: at(`u_band${suffix}[0]`),
    bandWs: at(`u_bandWs${suffix}[0]`),
    nb: at(`u_nb${suffix}`),
  })
  const slotA = layerAt('A')
  const slotB = layerAt('B')

  const stopsBuf = new Float32Array(15)
  const bandBuf = new Float32Array(16)
  const bandWsBuf = new Float32Array(8)

  function setLayer(slot: ReturnType<typeof layerAt>, layer: GlLayer) {
    gl.uniform1ui(slot.seed, layer.r.seed)
    gl.uniform1i(slot.field, layer.field)
    gl.uniform1i(slot.oct, layer.r.oct)
    gl.uniform1f(slot.warp, layer.r.warp)
    gl.uniform1f(slot.lo, layer.lo)
    gl.uniform1f(slot.hi, Math.max(layer.hi, layer.lo + 1e-4))
    gl.uniform1f(slot.tMax, layer.r.tMax)
    for (let i = 0; i < 5; i++) {
      const c = layer.stops[i]!
      stopsBuf[i * 3] = c[0] / 255
      stopsBuf[i * 3 + 1] = c[1] / 255
      stopsBuf[i * 3 + 2] = c[2] / 255
    }
    gl.uniform3fv(slot.stops, stopsBuf)
    bandBuf.fill(0)
    bandWsBuf.fill(0)
    for (let i = 0; i < layer.bands.length && i < 4; i++) {
      const b = layer.bands[i]!
      bandBuf[i * 4] = b.y0
      bandBuf[i * 4 + 1] = b.slope
      bandBuf[i * 4 + 2] = b.amp
      bandBuf[i * 4 + 3] = b.freq
      bandWsBuf[i * 2] = b.wid
      bandWsBuf[i * 2 + 1] = b.str
    }
    gl.uniform4fv(slot.band, bandBuf)
    gl.uniform2fv(slot.bandWs, bandWsBuf)
    gl.uniform1i(slot.nb, Math.min(4, layer.bands.length))
  }

  return {
    draw(state: DrawState) {
      gl.uniform2f(res, canvas.width, canvas.height)
      gl.uniform1f(phase, state.phase)
      gl.uniform1f(blend, state.blend)
      gl.uniform1f(envMul, state.env.mul)
      gl.uniform1f(envSat, state.env.sat)
      gl.uniform1f(envTa, state.env.ta)
      gl.uniform3f(
        envTint,
        state.env.tint[0] / 255,
        state.env.tint[1] / 255,
        state.env.tint[2] / 255,
      )
      // 稳态时 from 在着色器里被整支跳过,不用在这里省。
      setLayer(slotA, state.from)
      setLayer(slotB, state.to)
      gl.drawArrays(gl.TRIANGLES, 0, 3)
    },
    resize(width: number, height: number) {
      canvas.width = width
      canvas.height = height
      gl.viewport(0, 0, width, height)
    },
    dispose() {
      gl.deleteProgram(program)
    },
  }
}
