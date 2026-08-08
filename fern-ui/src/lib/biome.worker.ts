/**
 * 群系绘制的 Worker 端:收到请求就在 OffscreenCanvas 上画一张,把位图转移
 * 回去。取消是尽力而为——同步的 paint 打断不了,只保证画完的结果不会送出去。
 */

import { paintBitmap, type BiomeOptions } from 'fern-kit/biome'

interface RenderMessage {
  type: 'render'
  id: number
  width: number
  height: number
  phase?: number
  quality?: number
  options: BiomeOptions
}

interface CancelMessage {
  type: 'cancel'
  id: number
}

type WorkerMessage = RenderMessage | CancelMessage

const cancelled = new Set<number>()

// tsconfig 的 lib 是 DOM,`self` 被当成 Window;这里只用到 Worker 语义的
// 两个成员,按结构收窄比整套换 webworker lib 干净。
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<WorkerMessage>) => void) | null
  postMessage(message: unknown, transfer: Transferable[]): void
}

scope.onmessage = (event: MessageEvent<WorkerMessage>) => {
  const message = event.data
  if (message.type === 'cancel') {
    cancelled.add(message.id)
    return
  }

  if (cancelled.delete(message.id)) return

  const bitmap = paintBitmap(
    message.width,
    message.height,
    message.options,
    message.phase ?? 0,
    message.quality ?? 0.6,
  )

  if (cancelled.delete(message.id)) {
    bitmap.close()
    return
  }

  scope.postMessage({ type: 'done', id: message.id, bitmap }, [bitmap])
}

export {}
