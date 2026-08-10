// 群系色场，换成品牌色。
//
// biome.ts 的每个群系自带一套色标（丛林、深海、下界……），站上要的是同一支笔
// 画出来的形状，但落在墨松—蕨绿—嫩芽这条线上。所以先让它照常画，再按亮度
// 把颜色整体映射过来：形状、层次、时辰的影响全部保留，只换了色。

import { paint } from 'fern-kit/ui/biome';

/** 墨松 → 蕨绿 → 嫩芽，五档。第一档比墨松再沉一点，暗部才压得住。 */
const RAMP = [
  [10, 19, 14],
  [14, 32, 24],
  [28, 61, 42],
  [53, 113, 74],
  [191, 228, 178]
];

function rampAt(t) {
  const x = Math.min(0.9999, Math.max(0, t)) * (RAMP.length - 1);
  const i = Math.floor(x);
  const f = x - i;
  const a = RAMP[i];
  const b = RAMP[i + 1];
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
}

/**
 * 画一块品牌色的场，返回画布。
 * @param {number} w 像素宽（可以按半分辨率给，场本来就是大色块）
 * @param {number} h 像素高
 * @param {{name: string, hours?: number, hour?: number}} o 三颗种子
 */
export function brandField(w, h, o) {
  const cv = document.createElement('canvas');
  cv.width = Math.max(2, Math.round(w));
  cv.height = Math.max(2, Math.round(h));
  // 丛林的动态范围最宽，映射之后层次留得最多
  paint(cv, { ...o, biomeKey: 'jungle' }, 0, 0.6);

  const ctx = cv.getContext('2d');
  const img = ctx.getImageData(0, 0, cv.width, cv.height);
  const d = img.data;

  // 先按自身的分位数把动态范围拉开。
  //
  // 群系场的输出常常整幅压在暗部（丛林本来就暗，再叠上夜里的环境色更暗），
  // 直接查表就只用得到色标最下面一两档，出来是一团黑。所以先量一遍这一幅
  // 到底落在哪一段，把它拉满 0–1 再查——多暗由色标说了算，不由源场碰巧多暗
  // 说了算。掐掉两头各百分之一点五，免得个别极值把整幅压回去。
  const hist = new Uint32Array(256);
  const n = d.length / 4;
  for (let i = 0; i < d.length; i += 4) {
    hist[(d[i] * 54 + d[i + 1] * 183 + d[i + 2] * 19) >> 8]++;
  }
  const cut = n * 0.015;
  let lo = 0;
  let hi = 255;
  for (let i = 0, acc = 0; i < 256; i++) {
    acc += hist[i];
    if (acc > cut) {
      lo = i;
      break;
    }
  }
  for (let i = 255, acc = 0; i >= 0; i--) {
    acc += hist[i];
    if (acc > cut) {
      hi = i;
      break;
    }
  }
  const span = Math.max(1, hi - lo);

  const lut = new Uint8Array(768);
  for (let i = 0; i < 256; i++) {
    // 伽马压一点，让亮的那一头收着，暗部的层次多留一些
    const t = Math.pow(Math.min(1, Math.max(0, (i - lo) / span)), 1.25);
    const c = rampAt(t);
    lut[i * 3] = c[0];
    lut[i * 3 + 1] = c[1];
    lut[i * 3 + 2] = c[2];
  }
  for (let i = 0; i < d.length; i += 4) {
    const l = (d[i] * 54 + d[i + 1] * 183 + d[i + 2] * 19) >> 8;
    d[i] = lut[l * 3];
    d[i + 1] = lut[l * 3 + 1];
    d[i + 2] = lut[l * 3 + 2];
  }
  ctx.putImageData(img, 0, 0);
  return cv;
}
