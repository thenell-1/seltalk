// NOTE Tauri v2 应用入口（防 windows.h 冲突）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    creative_input_method_lib::run()
}
