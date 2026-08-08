import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
    // dist/ 会被整个嵌进可执行文件，所以 sourcemap 是要发出去的东西，不是本地
    // 产物：压缩后它比它解释的那份 JS 还大一倍半，而且等于把源码原样附上。
    // `tauri build --debug` 设这个变量，调试包照旧带上。
    sourcemap: process.env.TAURI_ENV_DEBUG === "true",
  },
});
