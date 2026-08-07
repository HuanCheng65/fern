/**
 * 实例的默认名字。
 *
 * 新建实例不该从一个空输入框开始。绝大多数人对「叫什么」没有意见，逼他先想
 * 一个名字，只是在真正的问题（玩哪个版本）前面加了一道无谓的门槛。
 *
 * 名字就是版本号，装了加载器就跟在后面：`1.21.1`、`1.21.1 Fabric`。这是这一
 * 类工具的通行做法，也是最有用的默认值——列表里一眼看得出是什么。
 *
 * 这是默认值不是命名规则，随时可以改成别的。
 */

/** 同名不是错误（id 是另算的），但默认值撞上已有的会让人以为在改旧的那个。 */
export function suggestName(
  gameVersion: string,
  loaderLabel: string,
  taken: readonly string[] = [],
): string {
  const base =
    !gameVersion ? '' : loaderLabel && loaderLabel !== '原版' ? `${gameVersion} ${loaderLabel}` : gameVersion
  if (!base) return ''
  const used = new Set(taken)
  if (!used.has(base)) return base
  for (let index = 2; index < 100; index += 1) {
    const candidate = `${base} (${index})`
    if (!used.has(candidate)) return candidate
  }
  return base
}
