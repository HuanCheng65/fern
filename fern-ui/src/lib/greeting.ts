/**
 * 按钟点问候。启动页和联机页共用这一份。
 *
 * 只分三段。不写「夜深了」「该休息了」这类——那是在劝人做什么，文案规范里明确
 * 不做这件事；两处各写一份时，联机页就这么写过。
 */
export function salutation(hour: number): string {
  if (hour >= 18 || hour < 5) return '晚上好'
  return hour >= 12 ? '下午好' : '早上好'
}
