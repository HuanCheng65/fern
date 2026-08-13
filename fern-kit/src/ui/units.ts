/**
 * 数量变成一句话。
 *
 * 在 `ui/` 而不是 `parts/`：这里没有 Fern 的名词，只有「一个数怎么念」。内存
 * 这件事出现在启动页的压力条、实例设置、内存尺、设置页四处，四处必须念得一样
 * ——之前是四份逐字一样的副本，改一处的时候没人知道还有另外三处。
 */

/** 字节变成 MB。零和负数不说话——`0 MB` 是句废话，调用方会把它 filter 掉。 */
export const megabytes = (bytes: number) =>
  bytes > 0 ? `${Math.round(bytes / (1024 * 1024))} MB` : ''

/**
 * MB 变成 GB。
 *
 * 整数不带小数点——`8 GB` 比 `8.0 GB` 更像一个决定。`0.05` 这个容差是为了
 * `8191 MB` 这种从物理内存算出来的数：差半个百分点不值得写成 `8.0`。
 */
export const gigabytes = (mb: number) => {
  const value = mb / 1024
  return Math.abs(value - Math.round(value)) < 0.05
    ? `${Math.round(value)} GB`
    : `${value.toFixed(1)} GB`
}
