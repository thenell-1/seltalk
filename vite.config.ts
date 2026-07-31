import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { resolve } from "node:path";

// TODO 人工审查点：1.双入口路径 2.dev server 端口与 tauri.conf.json 一致 3.build 输出到 dist
// NOTE Tauri 双窗口：float.html 与 manager.html 两个独立入口，对应 tauri.conf.json 的两个窗口 url
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      // P4.4：ts-rs 生成的 .ts 类型文件 alias（指向 src-tauri/bindings/）
      "@bindings": resolve(__dirname, "src-tauri/bindings"),
    },
  },
  // Tauri 期望 dev server 固定端口 1420
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // 忽略 Rust 目录变更，避免触发前端热重载
      // 注：bindings/*.ts 是自动生成的，不希望触发 HMR（cargo test 时手动刷新即可）
      ignored: ["**/src-tauri/**"],
    },
    fs: {
      // 允许前端 import 从 src-tauri/bindings/ 读取生成的类型文件
      allow: ["..", resolve(__dirname)],
    },
  },
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        float: resolve(__dirname, "float.html"),
        manager: resolve(__dirname, "manager.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["tests/frontend/**/*.{test,spec}.ts"],
  },
});
