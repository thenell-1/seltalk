# 择言 SelTalk

> Windows 轻量化 AI 聊天辅助工具 · 被动触发 · 悬浮选词 · 逐字输入

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2F11-blue.svg)](https://github.com/thenell-1/seltalk)
[![Tauri](https://img.shields.io/badge/Tauri-2-orange.svg)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-2021-red.svg)](https://www.rust-lang.org)
[![Vue](https://img.shields.io/badge/Vue-3-brightgreen.svg)](https://vuejs.org)

**SelTalk** 是一款 Windows 桌面 AI 聊天助手。它采用「**被动触发**」模式——在任意应用中复制一段文本，按下热键，SelTalk 读取剪贴板内容、调用大语言模型（LLM）生成多条候选回复，在轻量悬浮窗中供你挑选，选中后以**逐字模拟输入**的方式写入当前光标位置。整个流程无需切换窗口、无需手动粘贴。

---

## ✨ 核心特性

| 特性 | 说明 |
|------|------|
| 🔥 **被动热键触发** | 不抢占焦点，仅在按下热键时工作，与现有工作流零干扰 |
| 📋 **剪贴板安全** | 读取后立即原样还原，不破坏你已复制的内容 |
| 🛡️ **黑名单脱敏** | 手机号/身份证/邮箱等敏感信息在送入 LLM 前替换为 `***` |
| 📚 **词库注入** | 可将常用术语通过 `{{words}}` 变量注入提示词 |
| 📊 **词频统计** | 自动统计选用的候选词汇，可视化高频词云 |
| 🔒 **任务锁 + 看门狗** | 防止重复触发；卡死 60 秒自动恢复 |
| 🖥️ **系统托盘** | 可暂停/恢复热键，避免与其他软件冲突 |
| ⚡ **LLM 流式输出** | 首字延迟从总生成时间降到首 token 时间 |
| 🚀 **开机自启** | 可选，系统启动时自动运行 |

---

## 🎬 工作流程

```
① 选中文字 → Ctrl+C 复制
        ↓
② 按下热键（默认 Ctrl+Shift+Space，可自定义）
        ↓
③ SelTalk 读取剪贴板 → 清洗 → 黑名单脱敏 → 调用 LLM
        ↓
④ 悬浮窗弹出，显示原文 + 多条候选回复
        ↓
⑤ ↑↓ 或滚轮选择 → Tab / 双击 / 确认按钮确认
        ↓
⑥ SelTalk 在原光标位置逐字输入选中的回复
```

---

## 🏗️ 技术栈

- **后端**：Rust 2021 + [Tauri 2](https://tauri.app)
- **前端**：Vue 3 + TypeScript + Vite
- **UI 库**：[Naive UI](https://www.naiveui.com) + [ECharts](https://echarts.apache.org)（词云）
- **数据库**：SQLite ([rusqlite](https://github.com/rusqlite/rusqlite))
- **LLM 接入**：OpenAI 兼容 API（SSE 流式 + 非流式回退）
- **日志**：[tracing](https://github.com/tokio-rs/tracing) + 滚动文件日志

---

## 📦 项目结构

```
seltalk/
├── src-tauri/                # Rust 后端
│   ├── src/
│   │   ├── clipboard/        # 剪贴板读取
│   │   ├── commands.rs       # Tauri 命令桥接（前端 ↔ Rust）
│   │   ├── config.rs         # 默认值与设置键名常量
│   │   ├── db/               # SQLite 数据访问层
│   │   ├── hotkey/           # 全局热键注册 + 系统保留键黑名单
│   │   ├── input/            # 逐字模拟输入（Windows SendInput）
│   │   ├── llm/              # LLM 客户端（SSE 流式）+ Prompt 模板渲染
│   │   ├── orchestrator/     # 主链路编排：trigger → 生成 → 显示
│   │   ├── state/            # 全局状态 + 任务锁 + 配置缓存
│   │   ├── text/             # 文本清洗/过滤/分词/词云
│   │   ├── tray/             # 系统托盘
│   │   └── window/           # 悬浮窗管理
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                      # Vue 前端
│   ├── float/                # 悬浮窗（候选选择界面）
│   ├── manager/              # 管理面板（设置/词库/模板/词云）
│   │   ├── views/             # 设置/LLM配置/模板/词库/词云视图
│   │   └── router/
│   ├── lib/api.ts            # Tauri 命令封装
│   └── styles/
├── docs/                     # 文档
│   ├── 使用教程.md
│   ├── 效率优化方案.md
│   └── 阶段二验收清单.md
├── float.html                # 悬浮窗入口
├── manager.html              # 管理面板入口
├── package.json
└── vite.config.ts
```

---

## 🚀 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) ≥ 18
- [pnpm](https://pnpm.io/) ≥ 8
- [Rust](https://www.rust-lang.org/tools/install) ≥ 1.77.2
- Windows 10 / 11（仅支持 Windows，依赖 `windows` crate 的 SendInput）

### 安装与运行

```bash
# 1. 克隆仓库
git clone https://github.com/thenell-1/seltalk.git
cd seltalk

# 2. 安装前端依赖
pnpm install

# 3. 开发模式运行（同时启动 Vite + Tauri）
pnpm tauri dev

# 4. 构建生产安装包（生成 NSIS .exe）
pnpm tauri build
```

构建产物位于 `src-tauri/target/release/bundle/nsis/`。

### 首次配置

1. 启动应用后右键托盘图标 → 「显示管理面板」
2. 进入 **LLM 配置** → 填写接口地址、API Key、模型名称 → 「测试连接」
3. 进入 **设置** → 自定义热键（默认 `Ctrl+Shift+Space`）、候选数、打字速度等
4. （可选）在 **Prompt 模板** 中编辑默认模板，使用 `{{origin}}`、`{{words}}` 变量
5. （可选）在 **词库管理** 中导入常用术语，通过 `{{words}}` 注入到提示词

---

## ⌨️ 操作说明

| 操作 | 快捷键 / 鼠标 |
|------|---------------|
| 触发 AI 生成候选 | 自定义热键（默认 `Ctrl+Shift+Space`） |
| 切换候选 | `↑` `↓` 或鼠标滚轮 |
| 确认选词 | `Tab` / 双击候选 / 点击确认按钮 |
| 取消本次会话 | `Esc` / 点击窗外 / 关闭按钮 |
| 暂停/恢复热键 | 右键托盘图标 → 「暂停热键」 |

> ⚠️ **重要**：热键不可设为系统保留键（如 `Ctrl+C`、`Alt+F4`、`Alt+Tab`），SelTalk 会自动拦截这类配置以保护系统功能。

---

## ⚙️ 配置项速查

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `hotkey` | `Ctrl+Shift+Space` | 全局触发热键 |
| `candidate_count` | `3` | 候选条数 |
| `type_min_ms` / `type_max_ms` | `30` / `120` | 逐字输入延迟（毫秒，随机抖动） |
| `llm_base_url` | — | LLM API 地址（OpenAI 兼容） |
| `llm_model` | — | 模型名称 |
| `llm_temperature` | `0.6` | 生成温度 |
| `llm_max_tokens` | `1024` | 单次最大 token 数 |
| `llm_stream_enabled` | `true` | 是否启用 SSE 流式输出 |
| `float_style_preset` | `standard` | 悬浮窗样式（compact/standard/loose） |
| `autostart` | `false` | 开机自启 |

---

## 🧪 测试

```bash
# 前端单元测试
pnpm test

# Rust 单元测试 + Clippy 检查
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

---

## 📚 文档

- 📖 [使用教程](docs/使用教程.md) — 完整功能操作指南与故障排除
- ⚡ [效率优化方案](docs/效率优化方案.md) — 性能调优与监控指标设计
- ✅ [阶段二验收清单](docs/阶段二验收清单.md) — 功能验收清单

---

## 🛠️ 故障排除

| 现象 | 可能原因与解决 |
|------|----------------|
| 热键无响应 | 1) 检查托盘是否「暂停热键」；2) 热键被其他软件占用，更换热键；3) 检查是否设为系统保留键 |
| `Ctrl+C` 复制失效 | 历史版本热键未做保留键校验，请升级到最新版并在设置中修正热键 |
| 悬浮窗弹出慢 | 1) 检查 LLM 服务端响应延迟（日志中查 `LLM 流式首字节`）；2) 确认启用了 `llm_stream_enabled` |
| 候选为空 | 1) LLM 配置未填或测试失败；2) 剪贴板内容被黑名单全部过滤 |
| 逐字输入异常 | 1) 目标应用以管理员权限运行，SelTalk 未提权；2) 调整 `type_min_ms/max_ms` 增大延迟 |
| LLM 响应超 30s | 多为服务端瓶颈，建议更换模型或服务商（参考 [效率优化方案](docs/效率优化方案.md)） |

日志位置：`%APPDATA%\com.seltalk.app\logs\seltalk.log`

---

## 🤝 贡献

欢迎提 Issue 与 PR。请确保：

1. 新增功能附带单元测试（Rust `cargo test` + 前端 `pnpm test` 全绿）
2. 通过 `cargo clippy --all-targets -- -D warnings` 与 `vue-tsc --noEmit`
3. 遵循现有代码风格（中文注释、模块化拆分、单函数 ≤ 50 行）

---

## 📄 许可证

[MIT License](LICENSE)
