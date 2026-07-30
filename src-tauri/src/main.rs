// TODO 人工审查点：1. windows_subsystem 在 release 下隐藏控制台
// NOTE Tauri 桌面入口：调用 lib::run()，release 模式隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    seltalk_lib::run();
}
