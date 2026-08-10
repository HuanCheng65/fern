/**
 * 下载什么、从哪儿下载。
 *
 * 版本从**自更新用的那份清单**读：`{端点}/{通道}/manifest.json`，和客户端读的是同
 * 一个文件（见 fern-core/src/update/mod.rs）。这样官网上的版本号不可能和客户端认为
 * 的版本号对不上——它们本来就是一个来源。
 *
 * 但**清单里的地址不能直接给人**。macOS 那一条指向 `.app.tar.gz`，那是更新器要的
 * 格式；人要的是 `.dmg`。两套文件同在 `release/{版本}/` 下，发布脚本里写得很清楚：
 * 「给人下载的那两个，不进清单」。所以这里只从清单取版本号，文件名按发布脚本的命名
 * 规则自己拼——规则变了，这里必须跟着变。
 *
 * 端点写死成和 tauri.conf.json 里 updater.endpoints 同一个域名。发布流水线有一步
 * 专门校验「客户端构建时用的端点必须就是这次发布的目标」，两边不会各走各的。
 */

/** 和 fern-ui/src-tauri/tauri.conf.json 的 updater.endpoints 同一个域名。 */
export const ENDPOINT = 'https://dl.fern.huanchengfly.top';

export const RELEASES = 'https://github.com/HuanCheng65/fern/releases';

/** 通道。顺序即优先级：正式版在前。 */
export const CHANNELS = ['stable', 'beta'];

export const CHANNEL_NAME = { stable: '正式版', beta: '测试版' };

const OS_NAME = { windows: 'Windows', macos: 'macOS', linux: 'Linux' };

export const osName = (os) => OS_NAME[os] ?? '';

/**
 * 访客在哪个系统上。认不出就返回 null——那时页面给出全部平台，不猜。
 *
 * iPadOS 在 UA 里自称 Macintosh，但它下载不了桌面版；用最大触点数把它排除掉。
 */
export function detectOs() {
  if (typeof navigator === 'undefined') return null;
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Android/i.test(ua)) return null;
  if (/Mac|iPhone|iPad|iPod/i.test(ua)) {
    if (/iPhone|iPod/i.test(ua)) return null;
    if (navigator.maxTouchPoints > 1 && /Mac/i.test(ua)) return null;
    return 'macos';
  }
  if (/Linux|X11/i.test(ua)) return 'linux';
  return null;
}

/**
 * 某个版本下，每个平台可下载的东西。
 *
 * 文件名规则来自 .github/workflows/release.yml 的「Give everything its published
 * name」那一步。`.exe` 和 `.AppImage` 的名字不带版本号（它们同时是更新器的目标，
 * 清单按固定名字引用），`.dmg` 和 `.deb` 带版本号。
 */
export function filesFor(version) {
  const at = (name) => `${ENDPOINT}/release/${version}/${name}`;
  return {
    windows: [
      {
        id: 'exe',
        label: '可执行文件',
        ext: '.exe',
        note: 'x86_64 · 免安装',
        url: at('Fern-windows-x86_64.exe')
      }
    ],
    macos: [
      {
        id: 'dmg',
        label: '磁盘映像',
        ext: '.dmg',
        note: '通用二进制 · Apple 芯片与 Intel',
        url: at(`Fern-${version}-universal.dmg`)
      }
    ],
    linux: [
      {
        id: 'appimage',
        label: 'AppImage',
        /* 名字本身就是格式，再挂一个 `.AppImage` 是同一个词说两遍。 */
        ext: '',
        note: 'x86_64',
        url: at('Fern-linux-x86_64.AppImage')
      },
      {
        id: 'deb',
        label: 'Debian 软件包',
        ext: '.deb',
        note: 'x86_64',
        url: at(`Fern-${version}-amd64.deb`)
      }
    ]
  };
}

/**
 * 取一个通道的状态。
 *
 * 三种结果，对应三句不同的话：
 *   ready   ——  这个通道有版本
 *   absent  ——  端点答了 404：这个通道还没有发布过任何版本
 *   offline ——  取不到（网络、跨域、清单损坏）。**不能当成「没有版本」**，
 *               那会把一次网络故障说成「本项目尚未发布」。
 */
export async function readChannel(channel, { timeout = 6000 } = {}) {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), timeout);
  try {
    const res = await fetch(`${ENDPOINT}/${channel}/manifest.json`, {
      signal: abort.signal,
      cache: 'no-cache'
    });
    if (res.status === 404) return { channel, state: 'absent' };
    if (!res.ok) return { channel, state: 'offline' };
    const m = await res.json();
    if (typeof m?.version !== 'string') return { channel, state: 'offline' };
    return {
      channel,
      state: 'ready',
      version: m.version,
      notes: typeof m.notes === 'string' ? m.notes : '',
      date: typeof m.pubDate === 'string' ? m.pubDate : ''
    };
  } catch {
    return { channel, state: 'offline' };
  } finally {
    clearTimeout(timer);
  }
}

/**
 * 一页里问同一个通道，只问一次。
 *
 * 首页顶上、页尾、下载页顶上是同三颗按钮，下载页底下还要列全部版本——各发各的
 * 请求，是同一个答案取好几遍。而且通道是在发布途中被改写的：几秒之内的两次请求
 * 真的可能给出两个版本号，那时一页之内会自相矛盾。
 */
const asked = new Map();

export function channelOnce(channel) {
  if (!asked.has(channel)) asked.set(channel, readChannel(channel));
  return asked.get(channel);
}

/** 「下载」按钮该给哪一版：有正式版给正式版，没有才退到测试版。取不到给 null。 */
export async function currentRelease() {
  for (const channel of CHANNELS) {
    const found = await channelOnce(channel);
    if (found.state === 'ready') return found;
  }
  return null;
}

/** `2026-08-10T12:00:00Z` → `2026 年 8 月 10 日`。认不出就原样返回。 */
export function readableDate(value) {
  if (!value) return '';
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  return `${d.getFullYear()} 年 ${d.getMonth() + 1} 月 ${d.getDate()} 日`;
}
