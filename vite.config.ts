import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri 前端集成配置：
// - 固定端口 5173 供 tauri devUrl 使用
// - clearScreen:false 保留 Rust 侧日志输出
// - 静态资源内联阈值调小，避免运行时资源寻址问题
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
