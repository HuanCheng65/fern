/**
 * 守住 kit 的那条线：`parts/` 可以用 `ui/`，反过来不行。
 *
 * 这条规则写在 README 里会烂——它只在有人违反的那一刻才需要被想起来，而那一刻
 * 没人在读 README。所以做成一个会失败的检查，挂在 `pnpm check` 前面。
 *
 * 判据是**依赖方向**，不是词表。想过用「ui/ 里不许出现『实例』」这种关键词扫描，
 * 但那既会误伤注释，也拦不住 `import type { Hit }` 这种真正的越界。
 */

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const src = join(dirname(fileURLToPath(import.meta.url)), 'src')

function filesIn(dir) {
  return readdirSync(join(src, dir), { withFileTypes: true })
    .flatMap((entry) =>
      entry.isDirectory()
        ? filesIn(join(dir, entry.name))
        : /\.(svelte|ts)$/.test(entry.name)
          ? [join(dir, entry.name)]
          : [],
    )
}

const wrongWay = []
const dangling = []

for (const file of filesIn('.')) {
  const text = readFileSync(join(src, file), 'utf8')
  for (const match of text.matchAll(/from\s+'([^']+)'/g)) {
    const target = match[1]

    if (file.startsWith('ui/') && (target.includes('parts/') || target.startsWith('fern-kit/parts'))) {
      wrongWay.push(`  ${file} → ${target}`)
    }

    /*
     * 相对路径解析不到就报。看着多余（构建当然会报），但 svelte-check 漏过一次：
     * 组件在层之间搬家之后 `./host.svelte` 变成了死路，check 全绿，直到构建才炸。
     * 搬家正是这条最容易断的时候，所以在最快的那道关口就拦住。
     */
    if (target.startsWith('.')) {
      const base = resolve(dirname(join(src, file)), target)
      const found = ['', '.ts', '.js', '.svelte', '.svelte.ts'].some((ext) => existsSync(base + ext))
      if (!found) dangling.push(`  ${file} → ${target}`)
    }
  }
}

if (wrongWay.length > 0) {
  console.error('ui/ 引用了 parts/，方向反了：\n' + wrongWay.join('\n'))
  console.error('\nui/ 只认形状，不认 Fern 的名词。要么把用到的东西降到 ui/，要么这个组件本来就属于 parts/。')
}
if (dangling.length > 0) {
  console.error('相对路径解析不到：\n' + dangling.join('\n'))
}
if (wrongWay.length > 0 || dangling.length > 0) process.exit(1)
