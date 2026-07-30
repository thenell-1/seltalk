import { createApp } from "vue";
import { createPinia } from "pinia";
import { router } from "./router";
import App from "./App.vue";
import "@/styles/base.css";

// NOTE 管理面板入口：独立 Vite 入口，对应 manager.html / tauri.conf.json 的 manager 窗口
const app = createApp(App);
app.use(createPinia());
app.use(router);
app.mount("#app");
