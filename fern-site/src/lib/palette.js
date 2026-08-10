import { BIOMES, paletteOf, resolve, stopsOf } from 'fern-kit/ui/biome';

/**
 * 产品在这张背景下会得到的那份色板。
 *
 * 启动器的色板是背景交出来的：`Backdrop` 每隔几秒把 --c0..c4 写在挂着 .fern-dark
 * 的那个元素上。官网没有那一层，所以这里当场算一份——用的是同一个 `paletteOf`，
 * 同一份数值，连压在强调色上的文字色都照同一条亮度阈值挑。
 *
 * 这不是「配一套接近的颜色」。整个设计那一章要证明的就是**颜色是推导出来的**，
 * 那这一份就必须真的是推导出来的。
 */
function stopsFor(scene) {
  const r = resolve({ name: scene.name, hours: scene.hours ?? 0, hour: scene.hour });
  return paletteOf(stopsOf(r.bk), r.env, r.tMax);
}

function varsOf(palette) {
  const rgb = (c) => `rgb(${c.map(Math.round).join(',')})`;
  const [cr, cg, cb] = palette[4];
  const luminance = (0.2126 * cr + 0.7152 * cg + 0.0722 * cb) / 255;
  return [
    ...palette.map((c, i) => `--c${i}:${rgb(c)}`),
    `--on-accent:${luminance > 0.55 ? '#0a0f12' : '#f2f5f5'}`,
    `--accent-glow:rgba(${Math.round(cr)},${Math.round(cg)},${Math.round(cb)},0.3)`
  ].join(';');
}

export function paletteVars(scene) {
  return varsOf(stopsFor(scene));
}

/**
 * 两个实例之间的色板，t 从 0 走到 1。
 *
 * 逐档在 RGB 上插值。生成器只肯按名字交出一份色板，中间不存在「半个实例」可以问，
 * 所以过渡这一段只能自己补。补的是**两份真实推导结果之间**的路，两头仍然分毫不差。
 */
export function mixVars(a, b, t) {
  if (t <= 0) return paletteVars(a);
  if (t >= 1) return paletteVars(b);
  const pa = stopsFor(a);
  const pb = stopsFor(b);
  return varsOf(pa.map((c, i) => c.map((v, k) => v + (pb[i][k] - v) * t)));
}

/** 强调色本身，用来给页面铺一层同色的环境光。 */
export function accentOf(scene) {
  const [cr, cg, cb] = stopsFor(scene)[4].map(Math.round);
  return `${cr}, ${cg}, ${cb}`;
}

/** 过渡当中的强调色。页面身后那层空气也要跟着换。 */
export function mixAccent(a, b, t) {
  if (t <= 0) return accentOf(a);
  if (t >= 1) return accentOf(b);
  const ca = stopsFor(a)[4];
  const cb = stopsFor(b)[4];
  return ca.map((v, k) => Math.round(v + (cb[k] - v) * t)).join(', ');
}

/** 这张背景是哪个群系。群系由名字的哈希决定，不由名字的意思决定。 */
export function biomeName(scene) {
  return BIOMES[resolve({ name: scene.name, hours: scene.hours ?? 0, hour: scene.hour }).bk].name;
}

/**
 * 这个钟点该说哪句问候。规则和产品里 Launch.svelte 的那一行同一条。
 *
 * 封面的环境种子在按同一个钟点调色温，所以问候语和画面上的光是同一个信号的两个面。
 */
export function salutationAt(hour) {
  return hour >= 18 || hour < 5 ? '晚上好' : hour >= 12 ? '下午好' : '早上好';
}
