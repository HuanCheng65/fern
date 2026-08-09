/**
 * 守住 kit 的那条线：`parts/` 可以用 `ui/`，反过来不行。
 *
 * 这条规则写在 README 里会烂——它只在有人违反的那一刻才需要被想起来，而那一刻
 * 没人在读 README。所以做成一个会失败的检查，挂在 `pnpm check` 前面。
 *
 * 判据是**依赖方向**，不是词表。想过用「ui/ 里不许出现『实例』」这种关键词扫描，
 * 但那既会误伤注释，也拦不住 `import type { Hit }` 这种真正的越界。
 */

import { readdirSync, readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const src = join(dirname(fileURLToPath(import.meta.url)), 'src')

function filesIn(dir) {
  return readdirSync(join(src, dir), { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(dir, entry.name))
}

const bad = []
for (const file of filesIn('ui')) {
  const text = readFileSync(join(src, file), 'utf8')
  for (const match of text.matchAll(/from\s+'([^']+)'/g)) {
    const target = match[1]
    if (target.includes('parts/') || target.startsWith('fern-kit/parts')) {
      bad.push(`  ${file} → ${target}`)
    }
  }
}

if (bad.length > 0) {
  console.error('ui/ 引用了 parts/，方向反了：\n' + bad.join('\n'))
  console.error('\nui/ 只认形状，不认 Fern 的名词。要么把用到的东西降到 ui/，要么这个组件本来就属于 parts/。')
  process.exit(1)
}
