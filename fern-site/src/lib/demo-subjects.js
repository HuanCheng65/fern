// 喂给真引擎的演示数据。引擎（fern-kit/parts/palette）是个注册表：
// 各特性自己调 provides()/commands()，所以站里注册一份假的，就能用真的匹配和排序。
// 引擎在浏览器里才加载（它要读 localStorage 记习惯），所以注册表由调用方传进来。

const now = Date.now();
const day = 86400000;

const INSTANCES = [
  { id: 'i1', title: '主世界', hint: '1.20.1 · Fabric', seen: now - day * 0.2 },
  { id: 'i2', title: '极限生存', hint: '1.16.5 · Forge', seen: now - day * 3 },
  { id: 'i3', title: '建筑档', hint: '1.21.4 · NeoForge', seen: now - day * 1 },
  { id: 'i4', title: '模组测试', hint: '1.20.4 · Quilt', seen: now - day * 12 },
  { id: 'i5', title: '光影档', hint: '1.21.1 · Fabric', seen: now - day * 6 },
  { id: 'i6', title: '红石实验', hint: '1.19.2 · Fabric', seen: now - day * 20 }
];

const WORLDS = [
  { id: 'w1', title: '我的世界', hint: '主世界 · 生存', seen: now - day * 0.5 },
  { id: 'w2', title: '空岛起源', hint: '极限生存 · 极限', seen: now - day * 9 },
  { id: 'w3', title: '海底神殿', hint: '建筑档 · 创造', seen: now - day * 2 }
];

const SERVERS = [
  { id: 's1', title: '朋友的服务器', hint: 'play.example.net', seen: now - day * 1.5 },
  { id: 's2', title: '生电服', hint: 'mc.example.org', seen: now - day * 30 }
];

const PLACES = [
  { id: 'p1', title: '外观', hint: '设置', terms: 'waiguan appearance theme 主题 颜色' },
  { id: 'p2', title: 'Java', hint: '设置', terms: 'java runtime 运行时' },
  { id: 'p3', title: '下载', hint: '设置', terms: 'xiazai download 镜像 源' },
  { id: 'p4', title: '账户', hint: '设置', terms: 'zhanghu account 登录' }
];

// 站上没有可执行的后端，所以「执行」就是把它说出来。由调用方给一个播报口。
let announce = () => {};

const asSubjects = (list, type, seedFrom = true) =>
  list.map((it) => ({
    type,
    id: it.id,
    title: it.title,
    hint: it.hint,
    terms: it.terms,
    seed: seedFrom ? it.title : undefined,
    seen: it.seen,
    run: () => announce(`前往 ${it.title}`)
  }));

const act = (title) => (subject) =>
  announce(subject ? `${title} · ${subject.title}` : title);

let registered = false;

/**
 * 只注册一次；重复调用无副作用。
 * @param {{ provides: Function, commands: Function }} registry fern-kit/parts/palette
 * @param {(text: string) => void} say 执行了什么，说一句
 */
export function registerDemo({ provides, commands }, say) {
  announce = say;
  if (registered) return;
  registered = true;

  provides(() => asSubjects(INSTANCES, 'instance'));
  provides(() => asSubjects(WORLDS, 'world'));
  provides(() => asSubjects(SERVERS, 'server'));
  provides(() => asSubjects(PLACES, 'place', false));

  commands(() => [
    { id: 'launch', title: '启动实例', accepts: 'instance', run: act('启动实例') },
    { id: 'open-dir', title: '打开游戏目录', accepts: 'instance', run: act('打开游戏目录') },
    { id: 'export', title: '导出整合包', accepts: 'instance', run: act('导出整合包') },
    { id: 'backup', title: '创建快照', accepts: 'instance', run: act('创建快照') }
    // 「新建实例」不在这里：引入 CommandPalette 时，fern-ui 的 lib/instances
    // 已经把它注册进同一张注册表了。再加一条就是列表里两行同名。
  ]);
}
