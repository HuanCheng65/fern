// 由 `cargo test` 生成，不要手改（见 fern-core/src/lib.rs 的 message_ids）。
//
// 后端只发 id 和参数，不发句子。这里的每一条都必须在文案表里有标题与说明——
// 少一条是编译错误，不是运行时才发现的空白。
export const BACKEND_MESSAGES = [
  'crash.broken-config',
  'crash.corrupt-archive',
  'crash.duplicate-mod',
  'crash.fabric-incompatible-mods',
  'crash.fabric-missing-dependency',
  'crash.forge-duplicate-mods',
  'crash.forge-mandatory-dependencies',
  'crash.forge-missing-dependency',
  'crash.graphics-driver-crash',
  'crash.graphics-unavailable',
  'crash.java-too-old',
  'crash.mixin-failure',
  'crash.mixin-failure-named',
  'crash.out-of-memory',
  'crash.port-in-use',
  'crash.unrecognized-jvm-option',
  'preflight.disabled-dependency',
  'preflight.duplicate',
  'preflight.missing-dependency',
  'preflight.no-loader',
  'preflight.wrong-game-version',
  'preflight.wrong-loader',
] as const

export type BackendMessage = (typeof BACKEND_MESSAGES)[number]
