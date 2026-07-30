import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { resolve } from "path";

// NOTE 双入口配置：管理面板（panel）+ 悬浮窗（overlay）
// 悬浮窗独立轻量化，不加载管理面板代码
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
    },
  },
  // 防止 Tauri 进程重启时端口冲突
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // 不监听 Rust 后端变化
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Tauri 使用 Webview，目标为现代浏览器
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        panel: resolve(__dirname, "panel.html"),
        overlay: resolve(__dirname, "overlay.html"),
      },
      output: {
        // 分目录输出，便于 Tauri 按窗口加载
        entryFileNames: (chunkInfo) => {
          return `${chunkInfo.name}/[name].js`;
        },
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash].[ext]",
      },
    },
  },
});
