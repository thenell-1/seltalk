import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "@/styles/base.css";
import "@/styles/float.css";

// NOTE 悬浮窗入口：独立 Vite 入口，对应 float.html / tauri.conf.json 的 float 窗口
const app = createApp(App);
app.use(createPinia());
app.mount("#app");
