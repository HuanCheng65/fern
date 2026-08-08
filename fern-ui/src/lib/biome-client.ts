/**
 * 群系绘制的 Worker 客户端。
 *
 * 头像这类小图不值得上 GPU,但也没理由占主线程:一个常驻 Worker 里跑同一套
 * paint,画完把像素当 ImageBitmap 转移回来,零拷贝。Worker 是懒建的单例——
 * 第一张图才起,之后所有请求共用;起不来(或环境不支持)由调用者退回主线程
 * 同步画,所以这里只管「能用 Worker 的那条路」。
 */

import type { BiomeOptions } from 'fern-kit/biome'

interface DoneMessage {
  type: 'done'
  id: number
  bitmap: ImageBitmap
}

interface Pending {
  resolve: (bitmap: ImageBitmap) => void
  reject: (error: unknown) => void
}

let worker: Worker | null = null
let nextId = 1
const pending = new Map<number, Pending>()

export const supportsBiomeWorker =
  typeof Worker !== 'undefined' &&
  typeof OffscreenCanvas !== 'undefined' &&
  typeof ImageBitmap !== 'undefined'

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL('./biome.worker.ts', import.meta.url), { type: 'module' })
    worker.onmessage = (event: MessageEvent<DoneMessage>) => {
      if (event.data.type !== 'done') return
      const request = pending.get(event.data.id)
      if (!request) {
        // 已取消的请求还是画完了——位图不还给任何人,关掉别泄漏。
        event.data.bitmap.close()
        return
      }
      pending.delete(event.data.id)
      request.resolve(event.data.bitmap)
    }
    worker.onerror = (event) => {
      // Worker 挂了就整个放弃:在等的全部拒绝,让调用者走同步兜底;
      // 下一个请求会重新建一个试试。
      for (const request of pending.values()) request.reject(event.error ?? event.message)
      pending.clear()
      worker?.terminate()
      worker = null
    }
  }
  return worker
}

/** 在 Worker 里画一张,结果以 ImageBitmap 转移回来。 */
export function renderBiome(
  width: number,
  height: number,
  options: BiomeOptions,
  phase = 0,
  quality = 0.6,
): { promise: Promise<ImageBitmap>; cancel: () => void } {
  const id = nextId++
  const promise = new Promise<ImageBitmap>((resolve, reject) => {
    pending.set(id, { resolve, reject })
    getWorker().postMessage({ type: 'render', id, width, height, options, phase, quality })
  })
  return {
    promise,
    cancel: () => {
      if (!pending.delete(id)) return
      worker?.postMessage({ type: 'cancel', id })
    },
  }
}
