/**
 * `Field` 递给控件的那几样属性。
 *
 * 单独放一个文件是因为 Svelte 的实例脚本导不出类型，而 `Input` / `Select`
 * 这些门面组件都要按这个形状接。
 */

export interface ControlProps {
  /** 由 Field 生成，同时绑在 `<label for>` 上。 */
  id: string
  /** 指向说明和错误那两行，控件自己不用管它们长在哪。 */
  'aria-describedby': string | undefined
  'aria-invalid': true | undefined
}
