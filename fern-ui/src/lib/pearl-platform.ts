/** Browser builds use the demo backend; the desktop build exposes Tauri internals. */
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
