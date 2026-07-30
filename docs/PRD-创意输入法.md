# 产品需求文档（PRD）：创意输入法 —— AI 智能回复助手

| 文档版本 | v5.0                                                                                              |
| -------- | ------------------------------------------------------------------------------------------------- |
| 编写日期 | 2026-07-30                                                                                         |
| 编写角色 | 产品经理（AI）                                                                                    |
| 目标受众 | 用户（个人使用者/最终受众）                                                                        |
| 文档状态 | 待评审（Draft for Review）                                                                         |
| 评审重点 | UI Automation+Ctrl+C 双轨捕获可行性 / 逐字模拟输入方案 / 词库RAG 分表共库 / Tauri v2 多窗口架构    |
| 变更说明 | v5.0 在 v4.0 基础上**调整技术架构**：弃用 Go + Wails v2（不支持原生多窗口），改为 **Rust + Tauri v2**（原生多窗口、`WS_EX_NOACTIVATE` 悬浮窗、更小打包体积）。保留 v4.0 的产品方向：监听微信/QQ 选中文本、逐字模拟输入、独立管理面板、词库管理、Prompt 可视化、LLM 配置、高频词云可视化、RAG/Ollama/习惯记忆/AES 加密。 |

> NOTE 核心说明：本 PRD 已锁定 26 项关键决策（详见第十一章 11.5 节）。
> TODO 人工审查点：1.UI Automation 在新版微信/QQ 的兼容性 2.逐字模拟输入中文方案 3.词库与 RAG 分表共库设计 4.Tauri v2 多窗口与不抢焦点悬浮窗实现

---

## 一、项目概述

### 1.1 产品定位

一款基于 Windows 平台的轻量化被动式聊天辅助工具，适配微信、QQ 主流社交软件。通过监听选中文本自动识别分析，生成适配场景的回复候选，支持真人逐字模拟输入；配套独立 Vue3 现代化后端管理面板，实现词库、Prompt 模板、LLM 模型、高频词云可视化全自定义管理。

**核心特征**：
- 无主动弹窗、无后台打扰、无剪贴板劫持；
- 极致贴合真人操作习惯；
- 前后端分离架构：Rust 后端负责核心监听、计算、存储；Vue3 前端负责交互浮窗与后台管理。

> 选型理由：真正的 TSF 输入法需 C++/COM 开发，周期以月计；用户真实诉求是"生成回复"而非"输入字符"，故采用"选中文本触发 + 逐字模拟输入"的辅助工具形态。

### 1.2 核心设计理念

1. **被动触发**：全程仅用户选中文本触发功能，无自动弹窗、无主动执行操作；
2. **全真模拟**：放弃粘贴功能，采用逐字符真人速度模拟键盘输入，规避平台风控；
3. **前后端分离**：Rust 后端负责核心监听、计算、数据存储；Vue3 前端负责交互悬浮窗、后台管理系统；
4. **完全可定制**：词库、Prompt 模板、LLM 参数全部后台可视化配置，无需修改代码；
5. **数据可视化**：支持聊天高频词词云图统计展示，辅助用户优化词库与回复风格。

### 1.3 运行环境

| 维度         | 规格                                          |
| ------------ | --------------------------------------------- |
| 操作系统     | Windows 10 / Windows 11（64 位）              |
| 技术架构     | Rust（后端核心）+ Tauri v2 + Vue3（前端界面） |
| 数据存储     | SQLite 轻量化本地数据库 + ChromaDB 向量库     |
| 界面风格     | 现代化简约 UI，支持暗色/浅色模式、高分屏适配  |
| 兼容软件     | 微信、QQ 主流客户端                            |
| 分发形态     | 单 exe/msi 安装包，体积 10-40MB，内置 WebView2 运行时引导 |

### 1.4 核心价值主张

1. **零侵入**：不修改系统输入法、不注入 IM 进程、不读写剪贴板（仅 UI Automation 失败时临时占用并立即清空）；
2. **被动触发**：仅选中文本时触发，无主动弹窗打扰；
3. **全真模拟**：逐字符模拟真人键盘输入，规避平台风控；
4. **越用越像你**：本地记录历史采纳回复与语气偏好，AI 生成时参考，含衰减权重；
5. **词库加持**：导入个人/工作文档与词条，回复内容自带专业知识背景；
6. **完全可定制**：词库、Prompt 模板、LLM 参数全部可视化配置；
7. **数据可视化**：高频词云图统计展示，辅助优化词库与回复风格；
8. **隐私可控**：默认本地处理；可开启 AES 加密敏感字段；可关闭云端上传；
9. **异常可控**：全链路异常分支均有中文提示，不崩溃、不暴露技术堆栈；
10. **现代化界面**：Vue3 + WebView2 渲染，界面美观、暗色模式与高分屏原生支持。

---

## 二、需求分析

### 2.1 用户痛点

| 编号 | 痛点描述                                          | 影响等级 |
| ---- | ------------------------------------------------- | -------- |
| P1   | 重复回复群通知、活动确认等模板化消息              | 高       |
| P2   | 切窗口查资料再回来回复，效率低                    | 高       |
| P3   | 回复语气不统一，不能体现个人风格                  | 中       |
| P4   | 想用 AI 但每次都要复制粘贴+切 ChatGPT 网页         | 高       |
| P5   | 现成 AI 回复不带个人知识库背景，答非所问          | 中       |
| P6   | 工具异常时无提示，不知如何处理                    | 高       |
| P7   | 担心隐私泄露，API Key/对话历史被偷                | 高       |
| P8   | 桌面工具界面粗糙、安装包臃肿、易被杀毒误报        | 高       |
| P9   | 粘贴式回复易被微信/QQ 风控识别，账号受限          | 高       |
| P10  | 词库、Prompt、模型参数无法可视化配置，门槛高      | 中       |

### 2.2 核心功能列表

| 编号  | 功能模块                           | 优先级 | 所属阶段 |
| ----- | ---------------------------------- | ------ | -------- |
| F1    | 微信/QQ 窗口识别（双重验证）       | P0     | 阶段一   |
| F2    | 选中文本捕获（UI Automation+Ctrl+C）| P0     | 阶段一   |
| F3    | 文本清洗过滤（正则+内存处理）      | P0     | 阶段一   |
| F4    | LLM 回复生成（auto 可选可关）      | P0     | 阶段一   |
| F5    | 悬浮候选窗（占位+候选+刷新）        | P0     | 阶段一   |
| F6    | 逐字模拟输入（SendInput Unicode）  | P0     | 阶段一   |
| F7    | 系统托盘（状态/菜单/启停）         | P0     | 阶段一   |
| F8    | 异常分支中文提示                   | P0     | 阶段一   |
| F9    | 后端管理面板（Vue3 独立窗口）      | P0     | 阶段一   |
| F10   | Prompt 自定义模板系统              | P1     | 阶段二   |
| F11   | 词库管理（CRUD+分类+导入）         | P0     | 阶段二   |
| F12   | 词库 RAG 检索（分表共库）          | P0     | 阶段二   |
| F13   | LLM 模型配置面板                   | P0     | 阶段二   |
| F14   | 用户习惯记忆（含衰减权重）         | P0     | 阶段二   |
| F15   | 高频词云可视化（近 7 天）          | P1     | 阶段二   |
| F16   | Ollama 本地模型部署                | P1     | 阶段二   |
| F17   | 知识库文档导入（TXT/MD）           | P1     | 阶段三   |
| F18   | 知识库增量更新                     | P1     | 阶段三   |
| F19   | AES 加密敏感字段                   | P2     | 阶段三   |
| F20   | 功能参数配置面板                   | P0     | 全阶段   |

### 2.3 核心用户故事

**US-1（主流程）**：用户在微信聊天窗口选中对方发来的消息文本 → 悬浮窗立即显示"AI 思考中…"占位 → 1-3 秒后填充 3 条候选回复 → 用户按方向键切换 → 按 Tab 键确认 → 程序逐字模拟输入到聊天输入框 → 用户按回车发送。

**US-2（刷新候选）**：用户对当前候选不满意 → 点击浮窗内"刷新"按钮 → 浮窗显示"AI 思考中…" → 重新生成 3 条候选。

**US-3（中断输入）**：用户在逐字输入过程中发现选错候选 → 按ESC立即中断 → 已输入内容保留 → 用户可手动删除。

**US-4（管理面板配置）**：用户双击托盘图标 → 打开管理面板 → 配置词库、Prompt 模板、LLM 参数 → 保存后实时生效 → 关闭面板不影响后台监听。

**US-5（词云查看）**：用户打开管理面板 → 查看高频词云（近 7 天采纳回复统计）→ 点击某高频词 → 跳转习惯记忆管理面板 → 筛选包含该词的历史回复。

### 2.4 功能边界与禁用规则

1. **仅微信、QQ 生效**：浏览器、文档、游戏等软件完全不触发；
2. **无选中文本时**，禁止生成候选、禁止弹窗；
3. **未按下确认键（默认 Tab）**，绝对不会自动输入内容；
4. **管理面板操作不影响**前台聊天辅助功能运行；
5. **UI Automation 失败时**，降级为程序自动模拟 Ctrl+C 读取并立即清空剪贴板，用户无感；
6. **逐字输入过程中**，按 ESC 立即中断（已输入内容保留）。

---

## 三、技术选型与对比分析

### 3.1 架构选型：弃用 Python 与 Go+Wails，采用「Rust + Tauri v2 + Vue3」

#### 3.1.1 原Python架构核心痛点

1. GUI 缺陷：PySide6/PyQt 桌面界面原生质感差、高分屏/暗色模式适配成本高、打包体积巨大、易被杀毒误报；
2. Python 运行时依赖繁琐，普通用户不会配置环境，打包 exe 动辄 1GB+；
3. 全局热键、窗口截图、Windows 系统 API、进程操作属于系统底层能力，Python 调用 win32 接口兼容性差；
4. 前端交互能力弱：悬浮窗、富文本、自定义样式改造成本极高。

#### 3.1.2 原 Go + Wails v2 架构的核心痛点（v5.0 弃用原因）

1. **不支持原生多窗口**：Wails v2 的 `options.App` 仅一组窗口字段，运行时所有 `Window*` API 都操作单一 `mainWindow`，无法同时承载"悬浮窗 + 管理面板"两个独立窗口；
2. **不支持 `WS_EX_NOACTIVATE` 窗口样式**：输入法悬浮窗必须不抢系统焦点（否则选中微信文本弹出悬浮窗时焦点跳走，导致后续逐字输入失败），Wails v2 的 `NewWindow` 只支持 `AlwaysOnTop`，不支持 `WS_EX_NOACTIVATE`；
3. **chromium 字段私有**：要自行创建第二个 WebView2 窗口需 fork Wails 源码暴露私有字段，维护成本高；
4. **打包体积偏大**：Go 静态编译 + WebView2 加载器约 50-80MB。

#### 3.1.3 新架构选型：Rust + Tauri v2 + Vue3

| 维度         | Rust + Tauri v2 + Vue3（选定）    | Go + Wails v2（已弃用）           |
| ------------ | --------------------------------- | --------------------------------- |
| 上手难度     | 中高，所有权机制有学习成本        | 低，语法简单                      |
| 编译速度     | 中等，首次编译久，增量快          | 快                                |
| 打包体积     | 极小，基础包 10~40MB              | 30~80MB                           |
| Windows API  | 完美，`windows-rs` 原生绑定       | 优秀，`lxn/win32`                 |
| **多窗口**   | **原生支持**（v2 核心 feature）   | **不支持**                        |
| **不抢焦点悬浮窗** | **支持**（windows-rs 直接设 `WS_EX_NOACTIVATE`） | **不支持**                |
| 桌面载体     | Tauri v2，Rust 原生集成 WebView2  | Wails 2.x                         |
| 内存占用     | 极低                              | 中等                              |
| 推荐人群     | 长期维护、追求极致性能、需多窗口  | 快速 MVP                          |

**决策**：选用 **Rust + Tauri v2 + Vue3**（多窗口与不抢焦点悬浮窗是输入法场景的硬需求，Tauri v2 原生支持，彻底解决 Wails v2 的架构痛点）。

### 3.2 文本捕获方案选型：UI Automation + Ctrl+C 备选

#### 3.2.1 方案对比

| 方案                       | 优点                           | 缺点                                       | 选定     |
| -------------------------- | ------------------------------ | ------------------------------------------ | -------- |
| A. UI Automation 辅助功能  | 不入侵进程、不占剪贴板         | 新版微信/QQ 对辅助功能支持不稳定           | **首选** |
| B. 程序自动模拟 Ctrl+C    | 最稳定可靠，兼容所有版本       | 临时占用剪贴板（读取后立即清空）           | **备选** |
| C. 用户手动 Ctrl+C        | 最简单可靠                     | 牺牲"选中即自动弹窗"体验                   | 不选     |

#### 3.3.2 双轨降级策略

```
选中文本事件
    ↓
尝试 UI Automation 读取选中文本
    ↓
┌─成功─→ 直接获取文本 + 选区坐标
│
└─失败─→ 程序自动模拟 Ctrl+C
         ↓
         读取剪贴板内容
         ↓
         立即清空剪贴板（用户无感）
         ↓
         无法获取选区坐标 → 浮窗显示在鼠标光标右下角
```

**Rust 实现**：
- UI Automation：`uiautomation` crate 调用 `IUIAutomation` COM 接口
- Ctrl+C 模拟：`windows-rs` 发送 `WM_COPY` 或 `SendInput` 模拟按键
- 剪贴板：`windows-rs` 的 `OpenClipboard`/`GetClipboardData`/`EmptyClipboard`

### 3.3 LLM 接入选型：混合方案（auto 可选可关）

#### 3.3.1 三种模式

| 模式     | 行为                                       | 适用场景               |
| -------- | ------------------------------------------ | ---------------------- |
| 云端     | 仅调用云端 API（DeepSeek/通义千问）        | 网络稳定，追求速度    |
| 本地     | 仅调用本地 Ollama                          | 隐私优先，离线可用    |
| **auto** | 云端优先，失败/超时/余额不足自动降级本地   | **默认推荐**           |

**决策**：保留 auto 模式，用户可在管理面板关闭 auto 改为手动切换。auto 模式检测到云端故障后切本地，云端恢复后**不自动切回**，需用户手动重置（避免频繁切换）。

#### 3.3.2 云端 LLM 选型

| Provider   | 模型         | 优势                     | 选定     |
| ---------- | ------------ | ------------------------ | -------- |
| DeepSeek   | DeepSeek-V3  | 性价比高，中文表现优秀   | **默认** |
| 通义千问   | Qwen-Max     | 阿里云生态，稳定        | 备选     |
| OpenAI     | GPT-4o-mini  | 通用性强                 | 备选     |

#### 3.3.3 本地 LLM 选型

| 模型          | 显存需求   | 优势                     | 选定     |
| ------------- | ---------- | ------------------------ | -------- |
| Qwen2.5-7B    | 8GB        | 中文优秀，量化后 5GB     | **默认** |
| Qwen2.5-3B    | 4GB        | 轻量化，低配机器可用     | 备选     |

### 3.4 向量数据库与 RAG 选型：词库与文档分表共库

#### 3.4.1 选型

| 选项               | 优点                     | 缺点                     | 选定       |
| ------------------ | ------------------------ | ------------------------ | ---------- |
| ChromaDB Go SDK    | 嵌入式，无额外进程       | Go SDK 生态较新          | **选定**   |
| SQLite + sqlite-vss| 无额外依赖               | 向量检索性能一般         | 备选       |

#### 3.4.2 分表共库结构（v4.0 新增）

```
ChromaDB 单一向量库
├── Collection: lexicon（词库词条，短文本）
│   ├── id, embedding, document, metadata{category, tags, enabled}
│   └── 检索时按类型过滤，仅返回词条
│
└── Collection: documents（文档分块，长文本）
    ├── id, embedding, document, metadata{file_name, tags, chunk_index}
    └── 检索时按类型过滤，仅返回文档分块
```

**设计理由**：词库词条是短文本（几个词），文档分块是长文本（几百字），分表存储避免互相干扰，共用一个向量库便于统一管理。

### 3.5 习惯记忆与配置存储选型

| 数据类型     | 存储方案        | 理由                       |
| ------------ | --------------- | -------------------------- |
| 历史采纳回复 | SQLite          | 结构化查询，含索引         |
| 风格画像     | SQLite          | 键值对存储                 |
| 词库词条     | SQLite + Chroma| SQLite 存元数据，Chroma 存向量 |
| 文档分块     | Chroma          | 向量检索                   |
| 配置         | JSON 文件       | 读写方便，可加密           |
| Prompt 模板  | SQLite          | 结构化管理                 |

### 3.6 键盘模拟输入方案：SendInput Unicode

#### 3.6.1 方案对比

| 方案                    | 优点                     | 缺点                          | 选定       |
| ----------------------- | ------------------------ | ----------------------------- | ---------- |
| SendInput Unicode       | 支持中文，不依赖 IME     | 需逐字符发送，速度受限        | **选定**   |
| 模拟 IME 输入           | 体验最真实               | 依赖输入法状态，易出错        | 不选       |
| 剪贴板粘贴              | 速度最快                 | 与 v4.0"弃用粘贴"原则冲突     | 不选       |

#### 3.6.2 Go 实现

```go
// 逐字模拟输入（SendInput Unicode）
// TODO 人工审查点：1.输入速度控制 2.中断响应 3.窗口焦点检测
func SimulateTyping(text string, speed int, cancelChan chan struct{}) error {
    delayBase := 1000 / speed // 每字符基础延迟（毫秒）
    for _, ch := range text {
        select {
        case <-cancelChan:
            return errors.New("用户中断输入")
        default:
        }
        // 发送 Unicode 字符
        sendUnicodeChar(ch)
        // 随机延迟 50-150ms + 基础延迟
        delay := delayBase + rand.Intn(100) + 50
        time.Sleep(time.Duration(delay) * time.Millisecond)
    }
    return nil
}
```

### 3.7 开发语言与核心依赖

#### 3.7.1 后端 Rust 栈

| 模块         | 技术选型                              | 说明                              |
| ------------ | ------------------------------------- | --------------------------------- |
| 主服务       | Rust 1.75+（2024 edition）            | 编译独立 exe，无依赖              |
| 桌面载体     | Tauri 2.x                             | Rust + WebView2 一体化框架，原生多窗口 |
| 窗口识别     | `windows-rs`（`Windows::Win32::UI::WindowsAndMessaging`） | 进程名 + 窗口类名双重验证 |
| 文本捕获     | `uiautomation` crate + windows-rs COM | 读取选中文本                      |
| 键盘模拟     | `windows-rs` SendInput                | Unicode 字符逐字输入              |
| OCR（废弃）  | -                                     | v4.0 起不再使用 OCR                |
| LLM 接口     | `reqwest` 封装 OpenAI 兼容协议        | 云端 + 本地 Ollama                |
| 向量库       | ChromaDB HTTP API（`reqwest` 调用）   | 嵌入式服务，词库与文档分表共库    |
| 数据库       | SQLite3（`rusqlite` with `bundled` feature） | 历史回复、词库元数据、Prompt 模板 |
| 通信服务     | Tauri 内置 IPC（commands + events）   | 前后端双向通信，无需自建 HTTP     |
| 文档解析     | `pulldown-cmark`(MD) + 原生 txt 解析  | 自动分块、hash 去重               |
| 日志         | `tracing` + `tracing-subscriber` + `tracing-appender` | 结构化日志，按天轮转 |
| 加密         | `aes-gcm` crate                       | AES-256-GCM 加密敏感字段           |
| 异步运行时   | `tokio`                               | Tauri v2 默认异步运行时            |
| 序列化       | `serde` + `serde_json`                | 配置文件、IPC 消息                 |

#### 3.7.2 前端栈

| 模块     | 技术选型                | 说明                              |
| -------- | ----------------------- | --------------------------------- |
| 框架     | Vue3 + TypeScript       | 组件化开发                        |
| UI 库    | Element Plus            | 桌面级 UI，支持暗色/浅色/高分屏   |
| 样式     | TailwindCSS             | 自定义悬浮窗、管理面板            |
| 状态     | Pinia                   | 全局状态管理                      |
| IPC      | `@tauri-apps/api` invoke + listen | 调用后端 commands、监听 events |
| 图表     | ECharts + echarts-wordcloud | 词云可视化                    |

### 3.8 通信层超时/重试/限流策略

| 场景             | 超时   | 重试  | 限流         |
| ---------------- | ------ | ----- | ------------ |
| 云端 LLM 调用    | 30s    | 2 次  | 10 次/分钟   |
| 本地 Ollama 调用 | 60s    | 1 次  | 无限制       |
| UI Automation    | 3s     | 0 次  | -            |
| Ctrl+C 备选      | 1s     | 0 次  | -            |
| 向量库检索       | 5s     | 1 次  | -            |

### 3.9 数据加密机制

- **加密范围**：仅敏感字段（API Key、LLM 配置、Ollama 端点）
- **历史回复、词库、习惯记忆**：明文存储，便于检索性能
- **加密算法**：AES-256-GCM
- **默认状态**：关闭，用户可在管理面板开启（P2 阶段实现）

### 3.10 Windows 兼容性适配

| 维度         | 适配方案                                       |
| ------------ | ---------------------------------------------- |
| DPI 缩放     | Tauri v2 + WebView2 原生支持高分屏，无需额外适配 |
| 暗色模式     | Element Plus 原生支持，跟随系统                 |
| 窗口置顶     | Tauri `set_always_on_top(true)` 属性            |
| 无边框窗口   | Tauri `decorations(false)` 属性                 |
| 透明度       | Tauri 窗口 `set_alpha` 或 WebView2 CSS 透明背景 |
| **不抢焦点** | **`windows-rs` 设置 `WS_EX_NOACTIVATE` 扩展样式**（悬浮窗核心需求） |
| 托盘常驻     | Tauri `tray-icon` feature 原生系统托盘          |
| 开机自启     | Tauri `autostart` 插件或注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| 多窗口       | Tauri v2 `WebviewWindowBuilder` 原生多窗口     |

### 3.11 打包分发策略

1. 前端 Vue3 打包为静态 HTML/CSS/JS 资源；
2. Tauri v2 将前端资源嵌入 Rust 二进制；
3. 静态编译 Rust 后端，内置 Windows WebView2 运行时引导；
4. 分发产物：单 exe/msi 安装包（约 10-40MB）；
5. 兼容 Windows 10/11 64 位，无需管理员权限；
6. 数据存储至 `%APPDATA%\CreativeInputMethod\` 目录。

---

## 四、系统架构

### 4.1 整体架构图（前后端分离）

```
┌─────────────────────────────────────────────────────────────────┐
│ 前端表现层（Tauri v2 承载 Vue3 多窗口）                          │
│  ┌──────────────────────┐  ┌──────────────────────────────────┐│
│  │ 管理面板（标准窗口）  │  │ 悬浮候选窗（无边框置顶不抢焦点） ││
│  │ - 词库管理            │  │ - 占位提示（AI 思考中…）        ││
│  │ - Prompt 模板         │  │ - 3 条候选回复                   ││
│  │ - LLM 配置            │  │ - 方向键切换/Tab 确认/ESC 关闭   ││
│  │ - 高频词云            │  │ - 刷新按钮                       ││
│  │ - 习惯记忆管理        │  │ - 自动避让屏幕边缘               ││
│  │ - 功能参数配置        │  └──────────────────────────────────┘│
│  └──────────────────────┘                                       │
│         ↓ Tauri IPC（commands + events）                        │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│ 本地后端服务层（Rust + Tauri v2，纯系统能力，无界面）            │
│                                                                  │
│ 【编排层 Orchestrator】主流程调度：捕获→清洗→RAG→LLM→模拟输入   │
│                                                                  │
│ 【系统能力模块】                                                 │
│  - 窗口识别（进程名+类名双重验证）                               │
│  - 文本捕获（UI Automation + Ctrl+C 备选）                      │
│  - 键盘模拟（SendInput Unicode 逐字输入）                       │
│  - 系统托盘（Tauri tray-icon feature）                          │
│                                                                  │
│ 【AI 能力模块】                                                  │
│  - LLM 服务（云端/本地/auto 模式）                              │
│  - 词库 RAG 检索（ChromaDB 分表共库）                           │
│  - Prompt 模板渲染                                              │
│                                                                  │
│ 【存储模块】                                                     │
│  - SQLite（历史回复、词库元数据、Prompt 模板、配置）            │
│  - ChromaDB（词库向量、文档分块向量）                           │
│  - AES 加密（敏感字段，P2 阶段）                                 │
│                                                                  │
│ 【基础设施】日志(tracing)、限流、重试、防抖                      │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 模块清单

| 模块             | 所属层 | 职责                                       | 技术实现                  |
| ---------------- | ------ | ------------------------------------------ | ------------------------- |
| WindowDetector   | 后端   | 识别微信/QQ 窗口（双重验证）               | `windows-rs`              |
| TextCapture      | 后端   | 捕获选中文本（UI Automation+Ctrl+C）       | `uiautomation` crate      |
| TextCleaner      | 后端   | 正则清洗、过滤无效符号                     | `regex` crate             |
| LlmService       | 后端   | LLM 调用（云端/本地/auto）                 | `reqwest`                 |
| PromptCustomizer | 后端   | Prompt 模板渲染、变量注入                  | `tera` 或原生格式化       |
| LexiconRag       | 后端   | 词库与文档检索（分表共库）                 | ChromaDB HTTP API         |
| HabitMemory      | 后端   | 历史采纳记录、衰减权重                     | `rusqlite`                |
| KeyboardSimulator| 后端   | 逐字模拟输入（SendInput Unicode）          | `windows-rs`              |
| ConfigStore      | 后端   | 配置持久化、AES 加密                      | `serde_json` + `aes-gcm`  |
| Orchestrator     | 后端   | 主流程调度                                 | `tokio` 异步              |
| TrayIcon         | 后端   | 系统托盘                                   | Tauri `tray-icon`         |
| ManagerPanel     | 前端   | 管理面板（词库/Prompt/LLM/词云）           | Vue3 + Element Plus       |
| CandidateOverlay | 前端   | 悬浮候选窗                                 | Vue3 无边框窗口            |
| WordCloud        | 前端   | 高频词云可视化                             | ECharts WordCloud         |

### 4.3 前后端通信协议

#### 4.3.1 Tauri Commands（前端调用后端）

> NOTE Tauri v2 通信以 IPC commands 为主（类型安全、零 HTTP 开销），HTTP REST 仅用于文件上传等少量场景。

| Command 名                  | 用途                          |
| --------------------------- | ----------------------------- |
| `get_config` / `save_config`| 读取/保存配置                 |
| `test_api_key`              | 测试 API Key 连通性           |
| `list_lexicon` / `save_lexicon` / `delete_lexicon` | 词库 CRUD |
| `import_lexicon` / `export_lexicon` | 导入/导出词库       |
| `list_prompt` / `save_prompt` / `delete_prompt` | Prompt 模板 CRUD |
| `preview_prompt`            | 预览模板渲染效果              |
| `get_llm_config` / `save_llm_config` | LLM 模型配置          |
| `test_llm`                  | 接口连通性测试                |
| `switch_llm_mode`           | 切换云端/本地/auto            |
| `generate_reply`            | 触发生成回复（异步）         |
| `refresh_reply`             | 刷新重新生成候选              |
| `adopt_reply`               | 采纳回复，记录到习惯库         |
| `list_history`              | 分页查询历史回复              |
| `get_history_stats`         | 词云统计数据（近 7 天）      |
| `upload_kb_doc`             | 上传知识库文档                |
| `list_kb_docs` / `delete_kb_doc` | 知识库文档管理          |
| `get_system_status`         | 系统状态                      |
| `show_overlay_window`       | 显示悬浮窗                    |
| `show_panel_window`         | 显示管理面板                  |

#### 4.3.2 Tauri Events（后端推送前端）

| 事件                  | 方向       | 用途                                  |
| --------------------- | ---------- | ------------------------------------- |
| `capture.triggered`   | 后端→前端  | 选中文本已捕获，前端准备显示浮窗      |
| `llm.generating`      | 后端→前端  | LLM 生成中（浮窗显示占位）            |
| `llm.done`            | 后端→前端  | LLM 完成，推送候选                    |
| `llm.refreshed`       | 后端→前端  | 刷新完成，推送新候选                  |
| `typing.started`      | 后端→前端  | 逐字输入开始                          |
| `typing.progress`     | 后端→前端  | 输入进度（可选展示）                  |
| `typing.done`         | 后端→前端  | 输入完成                              |
| `typing.interrupted`  | 后端→前端  | 输入被 ESC 中断                      |
| `error`               | 后端→前端  | 异常推送（错误码 + 中文提示）        |
| `status.update`       | 后端→前端  | 状态变更（模型切换/余额告警）        |
| `overlay.adopt`       | 前端→后端  | 用户采纳候选                          |
| `overlay.dismiss`     | 前端→后端  | 浮窗关闭（未采纳）                    |

### 4.4 数据流（主流程时序）

```
用户     TextCapture  Orchestrator  Cleaner  RAG     LLM     WebSocket   Overlay(Vue)  Keyboard
 │          │            │          │        │       │           │           │           │
 │ 选中文本 │            │          │        │       │           │           │           │
 │─────────▶│           │          │        │       │           │           │           │
 │          │ UI Automation        │        │       │           │           │           │
 │          │──失败─→ Ctrl+C 模拟   │        │       │           │           │           │
 │          │──成功─→ 获取文本+坐标 │        │       │           │           │           │
 │          │───────────▶│          │        │       │           │           │           │
 │          │           │ 推送 capture.triggered───────────────────▶│           │           │
 │          │           │          │ 清洗   │       │           │           │           │
 │          │           │─────────▶│        │       │           │           │           │
 │          │           │          │ 推送 llm.generating─────────────────▶│ 显示占位   │           │
 │          │           │          │        │ 检索  │           │           │           │
 │          │           │          │        │◀──────│           │           │           │
 │          │           │          │        │ 调LLM │           │           │           │
 │          │           │          │        │──异常─│ fallback  │           │           │
 │          │           │          │        │◀──3条─│           │           │           │
 │          │           │ 推送 llm.done────────────────────────────▶│ 显示候选   │           │
 │          │           │          │        │       │           │           │           │
 │ Tab确认  │           │          │        │       │           │           │           │
 │──────────────────────────────────────────────────────────────────────────▶│           │
 │          │           │ 推送 overlay.adopt                                            │           │
 │          │           │ 记录到 HabitMemory                                            │           │
 │          │           │ 逐字模拟输入                                                  │           │
 │          │           │──────────────────────────────────────────────────────────────▶│
 │          │           │ 推送 typing.done                                               │           │
```

---

## 五、核心模块详细设计

### 5.1 窗口识别与文本捕获模块

#### 5.1.1 窗口识别（双重验证）

```go
// TODO 人工审查点：1.窗口类名随版本更新的兼容性 2.性能开销 3.多窗口场景
// NOTE 双重验证：进程名 + 窗口类名，任一失败则不触发
func IsTargetWindow(hwnd win.HWND) bool {
    // 1. 获取进程名
    pid := getWindowProcessId(hwnd)
    processName := getProcessName(pid)
    
    // 2. 获取窗口类名
    className := getClassName(hwnd)
    
    // 3. 双重验证
    isWeChat := processName == "WeChat.exe" && 
                (className == "WeChatMainWndForPC" || className == "WeUIHelperWnd")
    isQQ := processName == "QQ.exe" && 
            (className == "TXGuiFoundation" || strings.HasPrefix(className, "TX"))
    
    return isWeChat || isQQ
}
```

**支持窗口类名清单**：
| 软件   | 进程名       | 窗口类名                       |
| ------ | ------------ | ------------------------------- |
| 微信   | WeChat.exe   | WeChatMainWndForPC, WeUIHelperWnd |
| QQ     | QQ.exe       | TXGuiFoundation, TX*            |

> 风险：微信/QQ 版本升级可能变更窗口类名，需在管理面板提供自定义配置入口。

#### 5.1.2 文本捕获（UI Automation + Ctrl+C 备选）

```go
// NOTE 双轨捕获：UI Automation 首选，失败降级 Ctrl+C 模拟
func CaptureSelectedText(hwnd win.HWND) (text string, rect Rect, err error) {
    // 方案 A：UI Automation 读取选中文本
    text, rect, err = captureViaUIAutomation(hwnd)
    if err == nil && text != "" {
        return text, rect, nil
    }
    
    // 方案 A 失败，降级方案 B：程序自动模拟 Ctrl+C
    text, err = captureViaCtrlC(hwnd)
    if err != nil {
        return "", Rect{}, err
    }
    // Ctrl+C 无法获取选区坐标，浮窗显示在鼠标光标右下角
    rect = getMouseCursorPosition()
    return text, rect, nil
}

// captureViaCtrlC：读取后立即清空剪贴板
func captureViaCtrlC(hwnd win.HWND) (string, error) {
    // 1. 备份当前剪贴板内容
    backup := backupClipboard()
    // 2. 模拟 Ctrl+C
    simulateCtrlC(hwnd)
    // 3. 读取剪贴板
    text := getClipboardText()
    // 4. 立即清空剪贴板（用户无感）
    emptyClipboard()
    // 5. 恢复原剪贴板（可选，避免影响用户）
    if backup != "" {
        restoreClipboard(backup)
    }
    return text, nil
}
```

#### 5.1.3 防抖与防重复触发

- 选中后 **500ms 防抖**（短时间内多次选中只触发一次生成）；
- 同一段文本 **30 秒内不重复触发**（hash 去重）；
- LLM 生成中（未返回）时，新的选中触发**取消上一个并重新生成**。

### 5.2 文本清洗过滤模块

```go
// NOTE 正则过滤无效符号、空白字符、冗余内容
// TODO 人工审查点：1.清洗规则完整性 2.性能开销 3.特殊字符处理
func CleanText(raw string) string {
    // 1. 去除首尾空白
    text := strings.TrimSpace(raw)
    // 2. 过滤控制字符
    text = removeControlChars(text)
    // 3. 合并连续空白
    text = collapseWhitespace(text)
    // 4. 过滤无效符号（保留中文标点）
    text = filterInvalidSymbols(text)
    // 5. 长度限制（防止超长文本拖慢 LLM）
    if len([]rune(text)) > 2000 {
        text = string([]rune(text)[:2000])
    }
    return text
}
```

**纯内存处理，全程不读写剪贴板（UI Automation 方案），无数据残留。**

### 5.3 悬浮候选窗模块（前端 Vue3）

#### 5.3.1 触发规则

1. **仅在微信、QQ 客户端窗口激活状态下生效**；
2. 用户选中文本后，**按下全局快捷键（默认 F8）触发**识别与候选生成（Windows 无"选中即触发"原生 API，改用快捷键方案，性能可靠且不误触发）；
3. 非指定软件、无选中文本时，**静默忽略，绝对不触发、不弹窗**。

#### 5.3.2 窗口特性

| 特性         | 规格                                           |
| ------------ | ---------------------------------------------- |
| 样式         | 极简悬浮，无多余装饰、无广告、无冗余按钮       |
| 位置         | 选中文本右下角（Ctrl+C 备选时为鼠标光标右下角）|
| 屏幕边缘避让 | 超出边界自动翻转到左上角                       |
| 焦点         | 永远置顶但不抢占系统焦点                       |
| 透明度       | 自适应，不影响正常聊天操作                     |
| 关闭机制     | 点击空白处、切换窗口、按下 ESC 键立即关闭      |

#### 5.3.3 交互操作

| 操作         | 行为                                   |
| ------------ | -------------------------------------- |
| 显示         | 3 条候选（默认，可配置 3-5 条）         |
| 占位         | 立即显示"AI 思考中…"加载动画            |
| 方向键 ↑↓   | 切换候选                               |
| Tab 键       | 确认填入，触发逐字模拟输入（默认，可自定义） |
| ESC 键       | 关闭浮窗                               |
| 刷新按钮     | 重新生成 3 条候选                      |
| 复制按钮     | 复制候选到剪贴板（可选）               |

#### 5.3.4 确认键自定义与冲突检测

- 默认确认键：**Tab**
- 用户可在管理面板自定义（支持 Tab/Enter/F1-F12/Ctrl+数字等组合）
- 保存时**自动检测冲突**：若与微信/QQ 系统快捷键冲突，弹窗提示用户更换

### 5.4 LLM 回复生成模块

#### 5.4.1 生成流程

```go
// NOTE 主流程：清洗后文本 → Prompt 模板 → RAG 检索 → LLM 调用 → 3 条候选
// TODO 人工审查点：1.Prompt 拼接 2.RAG 检索质量 3.超时降级
func GenerateReplies(selectedText string) ([]string, error) {
    // 1. 获取当前 Prompt 模板
    template := getCurrentPromptTemplate()
    
    // 2. RAG 检索（若启用）
    var ragContext string
    if config.RagEnabled {
        lexiconHits := lexiconRag.SearchLexicon(selectedText, 3)
        docHits := lexiconRag.SearchDocuments(selectedText, 3)
        ragContext = buildRagContext(lexiconHits, docHits)
    }
    
    // 3. 习惯记忆（若启用）
    var habitExamples string
    if config.HabitEnabled {
        examples := habitMemory.GetRecentExamples(20)
        habitExamples = buildHabitContext(examples)
    }
    
    // 4. 渲染 Prompt
    prompt := renderPrompt(template, selectedText, ragContext, habitExamples)
    
    // 5. 调用 LLM（auto 模式自动降级）
    candidates, err := llmService.Generate(prompt, 3)
    if err != nil {
        return nil, err
    }
    return candidates, nil
}
```

#### 5.4.2 auto 模式降级逻辑

```go
// NOTE auto 模式：云端优先，失败降级本地，不自动切回
func (s *LlmService) Generate(prompt string, count int) ([]string, error) {
    mode := config.LlmMode // "cloud" | "local" | "auto"
    
    if mode == "cloud" || (mode == "auto" && !s.fallbackTriggered) {
        candidates, err := s.callCloud(prompt, count)
        if err == nil {
            return candidates, nil
        }
        if mode == "auto" {
            s.fallbackTriggered = true // 标记降级，不自动切回
            // 推送状态变更通知
            websocket.Push("status.update", "云端故障，已降级本地模型")
        }
    }
    
    if mode == "local" || (mode == "auto" && s.fallbackTriggered) {
        return s.callLocal(prompt, count)
    }
    
    return nil, errors.New("LLM 调用失败")
}
```

### 5.5 逐字模拟输入模块（KeyboardSimulator）

#### 5.5.1 核心实现

```go
// NOTE 逐字符模拟真人键盘输入，规避平台风控
// TODO 人工审查点：1.输入速度 2.中断响应 3.窗口焦点 4.中文处理
func SimulateInput(text string, config TypeConfig) error {
    cancelChan := make(chan struct{})
    
    // 启动 ESC 监听协程
    go func() {
        if isEscapePressed() {
            close(cancelChan)
        }
    }()
    
    // 聚焦目标窗口（微信/QQ 输入框）
    focusTargetWindow()
    
    delayBase := 1000 / config.Speed // 每字符基础延迟
    
    for _, ch := range text {
        select {
        case <-cancelChan:
            return errors.New("用户中断输入")
        default:
        }
        
        // 发送 Unicode 字符
        if err := sendUnicodeChar(ch); err != nil {
            return err
        }
        
        // 随机延迟 50-150ms + 基础延迟
        delay := delayBase + rand.Intn(100) + 50
        time.Sleep(time.Duration(delay) * time.Millisecond)
    }
    
    return nil
}

// sendUnicodeChar：使用 SendInput 发送 Unicode 字符
func sendUnicodeChar(ch rune) error {
    input := win.KEYBDINPUT{
        wScan: uint16(ch),
        dwFlags: win.KEYEVENTF_UNICODE,
    }
    // 调用 SendInput 发送
    return sendInput(input)
}
```

#### 5.5.2 默认参数

| 参数         | 默认值         | 可配置范围    |
| ------------ | -------------- | ------------- |
| 输入速度     | 5 字/秒        | 3-10 字/秒    |
| 字符间延迟   | 50-150ms 随机  | -             |
| 中断键       | ESC            | 可自定义      |
| 输入完成后  | 终止任务，无后台残留 | -         |

#### 5.5.3 中断逻辑

- 输入过程中按 **ESC 立即中断**（已输入内容保留）；
- 输入过程纯文本格式，无富文本、无格式乱码；
- 输入完成后终止任务，无后台残留。

### 5.6 Prompt 自定义模板模块

#### 5.6.1 模板编辑器（前端 Vue3）

- **可视化编辑器**：支持自由编写、修改、删除 AI 提示词模板；
- **模板变量配置**：内置固定变量；
- **多模板保存**：支持新建多套模板，可一键切换默认模板；
- **模板预览功能**：配置后可测试渲染效果。

#### 5.6.2 内置变量

| 变量                | 说明                          |
| ------------------- | ----------------------------- |
| `{{selected_text}}` | 用户选中的聊天原文            |
| `{{lexicon_words}}` | 词库匹配的风格词/场景词       |
| `{{rag_context}}`   | RAG 检索的知识库上下文        |
| `{{habit_examples}}`| 习惯记忆的历史采纳示例        |
| `{{scene}}`         | 场景标签（自动识别）          |
| `{{candidate_count}}`| 生成候选数量                 |

#### 5.6.3 默认模板

```
你是一个聊天回复助手。请根据以下聊天内容，生成 {{candidate_count}} 条简短、自然的回复候选。

【对方消息】
{{selected_text}}

【风格参考】
{{lexicon_words}}

【知识背景】
{{rag_context}}

【回复示例】
{{habit_examples}}

要求：
1. 每条回复不超过 50 字
2. 语气自然，贴合个人风格
3. 直接输出回复内容，不要编号
```

### 5.7 词库管理模块

#### 5.7.1 词库 CRUD

- **可视化 CRUD**：新增、编辑、删除、批量导入/导出词库；
- **词库分类管理**：支持按聊天场景、语气风格、使用场景分类；
- **词库状态管理**：启用/禁用指定词条；
- **数据同步**：修改实时生效，无需重启程序。

#### 5.7.2 本地导入与 LLM 整理分类

```go
// NOTE 本地导入词库文件，LLM 自动整理分类
// TODO 人工审查点：1.LLM 分类准确性 2.大文件处理 3.重复词条
func ImportLexicon(filePath string) error {
    // 1. 读取文件内容（支持 TXT/MD/CSV/JSON）
    content := readFile(filePath)
    
    // 2. 调用 LLM 整理分类
    classified := llmClassifyLexicon(content)
    
    // 3. 入库（SQLite 存元数据，Chroma 存向量）
    for _, item := range classified {
        // 检查重复
        if !lexiconExists(item.Text) {
            saveLexiconToSQLite(item)
            saveEmbeddingToChroma(item)
        }
    }
    return nil
}

// llmClassifyLexicon：LLM 自动提取关键词并分类
func llmClassifyLexicon(content string) []LexiconItem {
    prompt := `请分析以下文本，提取关键词并分类：
1. 提取所有有意义的关键词/短语
2. 为每个词标注分类（场景：工作/社交/通知；语气：正式/随意/亲切）
3. 输出 JSON 格式

文本：
` + content
    // 调用 LLM 并解析返回
    return parseLLMResponse(llmService.Chat(prompt))
}
```

#### 5.7.3 词库与 RAG 检索（分表共库）

```go
// NOTE 词库检索：变量注入 + 语义检索 两者结合
func SearchLexicon(query string, topK int) []LexiconItem {
    // 1. 语义检索（ChromaDB lexicon collection）
    hits := chroma.Query("lexicon", query, topK)
    
    // 2. 相关性过滤（相似度 < 0.4 丢弃）
    var results []LexiconItem
    for _, hit := range hits {
        if hit.Score >= 0.4 {
            results = append(results, hit)
        }
    }
    return results
}
```

### 5.8 习惯记忆与词云可视化模块

#### 5.8.1 习惯记忆

- **记录字段**：历史采纳回复、时间戳、场景上下文摘要、来源进程；
- **衰减权重**：近 7 天权重 ×1.5，30 天内 ×1.0，30 天外 ×0.5；
- **样本数量**：默认引用 20 条，可配置 5-50 条。

#### 5.8.2 高频词云可视化

- **数据来源**：仅用户采纳的回复内容（不含未采纳的生成回复）；
- **默认统计周期**：近 7 天；
- **可视化展示**：ECharts WordCloud 动态渲染；
- **联动功能**：点击词云词汇 → 跳转习惯记忆管理面板 → 筛选包含该词的历史回复；
- **数据重置**：支持手动清空统计数据、重置词云；
- **实时更新**：新增采纳后自动刷新词云展示。

### 5.9 LLM 模型配置模块

#### 5.9.1 配置项

| 配置项         | 说明                              |
| -------------- | --------------------------------- |
| 接口地址       | 自定义大模型 API 地址             |
| API 密钥       | 加密存储（AES，P2 阶段）          |
| 模型名称       | 如 deepseek-chat、qwen2.5-7b     |
| 温度值         | 0-1，控制随机性                   |
| 最大生成长度   | 默认 200                          |
| 回复随机性     | top_p 参数                        |
| 模式           | 云端/本地/auto（可选可关）       |
| 多模型备用     | 支持配置多套模型，一键切换        |

#### 5.9.2 接口连通性测试

- 配置后可一键测试连通性；
- 返回：成功/失败 + 响应延迟 + 模型名称确认。

### 5.10 后端管理面板（Vue3 独立窗口）

#### 5.10.1 窗口特性

- **独立非悬浮式管理窗口**，为产品核心配置、数据管理、可视化中心；
- **每次程序启动都自动打开**（用户可手动关闭，不影响后台监听）；
- **常驻桌面可最小化**，不依附聊天窗口；
- **支持暗色/浅色模式切换**，跟随系统或手动设置。

#### 5.10.2 面板布局

```
┌──────────────────────────────────────────────────────────┐
│ 创意输入法管理面板                          [—] [□] [×] │
├──────────┬───────────────────────────────────────────────┤
│          │                                               │
│  仪表盘  │   [当前选中内容]                              │
│  词库    │                                               │
│  Prompt │   [高频词云（近 7 天）]                       │
│  模型    │                                               │
│  习惯    │   [系统状态]                                  │
│  知识库  │   - 监听状态：运行中                          │
│  设置    │   - 当前模型：DeepSeek-V3（云端）             │
│          │   - 今日生成：12 次  采纳：8 次               │
│          │                                               │
└──────────┴───────────────────────────────────────────────┘
```

### 5.11 系统托盘与启动行为

#### 5.11.1 启动行为

- **每次程序启动都自动打开管理面板**；
- 管理面板关闭后，后端监听服务继续运行（托盘常驻）；
- 双击托盘图标重新打开管理面板。

#### 5.11.2 托盘菜单

| 菜单项           | 行为                                   |
| ---------------- | -------------------------------------- |
| 打开管理面板     | 显示管理面板窗口                      |
| 暂停/恢复监听   | 暂停或恢复选中文本捕获                |
| 切换 LLM 模式    | 云端/本地/auto 快速切换               |
| 查看运行日志     | 打开日志查看器                        |
| 退出             | 完全退出程序                          |

#### 5.11.3 托盘状态图标

| 状态     | 图标颜色 | 说明                       |
| -------- | -------- | -------------------------- |
| 运行中   | 绿色     | 监听正常                   |
| 已暂停   | 黄色     | 用户手动暂停               |
| 生成中   | 蓝色     | LLM 生成中                 |
| 异常     | 红色     | LLM 不可用/UI Automation 失败 |

---

## 六、用户习惯记忆机制设计

### 6.1 记录字段

| 数据项          | 类型     | 用途                          |
| --------------- | -------- | ----------------------------- |
| 历史采纳回复    | text     | few-shot 示例注入 prompt       |
| 场景上下文摘要  | text     | 触发该回复的对话摘要          |
| 时间戳          | datetime | 衰减权重（近期权重高）        |
| 来源进程        | text     | WeChat.exe / QQ.exe          |
| 选中原文        | text     | 对方消息内容                  |

### 6.2 SQLite 表结构（含索引）

```sql
CREATE TABLE reply_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    context_text TEXT,        -- 选中原文
    candidates   TEXT,        -- JSON 数组：3 条候选
    adopted_idx  INTEGER,    -- 采纳第几条（0-2）
    final_reply  TEXT,        -- 最终采纳的回复
    source_app   TEXT         -- WeChat.exe / QQ.exe
);

-- 复合索引加速按时间查询
CREATE INDEX idx_time ON reply_history(created_at);

-- 词云统计专用索引
CREATE INDEX idx_time_reply ON reply_history(created_at, final_reply);
```

### 6.3 衰减权重算法（Go 实现）

```go
// NOTE 衰减权重：近期权重高，远期权重低
func CalculateWeight(createdAt time.Time) float64 {
    daysAgo := int(time.Since(createdAt).Hours() / 24)
    switch {
    case daysAgo <= 7:
        return 1.5  // 近 7 天权重×1.5
    case daysAgo <= 30:
        return 1.0  // 30 天内权重×1.0
    default:
        return 0.5  // 30 天外权重×0.5
    }
}
```

### 6.4 样本数量自定义

- 可配置生成回复时引用历史样本条数（5-50 条可调，默认 20 条）；
- 优先返回权重高的近期样本。

### 6.5 高频词云数据来源与统计

```go
// NOTE 词云数据来源：仅用户采纳的回复（final_reply 字段）
// TODO 人工审查点：1.分词准确性 2.停用词过滤 3.性能
func GetWordCloudStats(days int) []WordFreq {
    // 1. 查询近 N 天采纳的回复
    replies := queryAdoptedReplies(days)
    
    // 2. 中文分词
    var allWords []string
    for _, reply := range replies {
        words := segmentChinese(reply) // 使用 jieba 分词
        allWords = append(allWords, words...)
    }
    
    // 3. 过滤停用词
    filtered := filterStopWords(allWords)
    
    // 4. 统计词频
    freqMap := make(map[string]int)
    for _, word := range filtered {
        freqMap[word]++
    }
    
    // 5. 取 Top 100
    return topN(freqMap, 100)
}
```

### 6.6 习惯记忆管理面板（前端 Vue3）

- 按时间筛选（日期范围）；
- 按来源进程筛选（微信/QQ）；
- 单条删除 / 批量清空；
- 导出为 Markdown；
- 点击词云词汇 → 自动筛选包含该词的历史回复。

### 6.7 加密实现方案

- 后端 Rust 使用 `aes-gcm` crate 实现 AES-256-GCM；
- **仅加密敏感字段**（API Key、LLM 配置、Ollama 端点）；
- 历史回复、词库、习惯记忆数据**不加密**（明文存储，便于检索性能）；
- 加密开关默认关闭，P2 阶段实现。

---

## 七、词库 RAG 机制设计

### 7.1 分表共库结构

```
ChromaDB 单一向量库
├── Collection: lexicon（词库词条）
│   ├── 字段：id, embedding, document, metadata
│   ├── metadata：{category, tags, enabled, source}
│   └── 检索时按类型过滤，仅返回词条
│
└── Collection: documents（文档分块）
    ├── 字段：id, embedding, document, metadata
    ├── metadata：{file_name, tags, chunk_index, file_hash}
    └── 检索时按类型过滤，仅返回文档分块
```

### 7.2 SQLite 词库元数据表

```sql
CREATE TABLE lexicon (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    text         TEXT UNIQUE,           -- 词条内容
    category     TEXT,                  -- 分类：工作/社交/通知
    tone         TEXT,                  -- 语气：正式/随意/亲切
    tags         TEXT,                  -- 标签（JSON 数组）
    enabled      BOOLEAN DEFAULT 1,     -- 启用/禁用
    source       TEXT,                  -- 来源：手动/导入/LLM生成
    created_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_category ON lexicon(category);
CREATE INDEX idx_enabled ON lexicon(enabled);
```

### 7.3 词库本地导入与 LLM 整理分类

#### 7.3.1 支持格式

| 格式 | 说明                              |
| ---- | --------------------------------- |
| TXT  | 每行一个词条或一段文本           |
| MD   | 按标题或段落分割                  |
| CSV  | 列：text, category, tone, tags   |
| JSON | 结构化数据                        |

#### 7.3.2 LLM 整理分类流程

```go
// NOTE LLM 自动提取关键词并分类
func llmClassifyLexicon(content string) []LexiconItem {
    prompt := `请分析以下文本，提取关键词并分类：
1. 提取所有有意义的关键词/短语（2-10 字）
2. 为每个词标注分类：
   - category: 工作/社交/通知/其他
   - tone: 正式/随意/亲切
   - tags: 相关标签数组
3. 输出 JSON 格式：[{"text":"...", "category":"...", "tone":"...", "tags":[...]}]

文本：
` + content
    
    response := llmService.Chat(prompt)
    return parseLLMJsonResponse(response)
}
```

### 7.4 检索过滤优化

#### 7.4.1 相关性过滤

- 检索后相似度低于 **0.4** 的结果直接丢弃；
- 不注入 Prompt。

#### 7.4.2 标签过滤

- 支持按分类、语气、标签过滤检索范围；
- 检索时优先返回 enabled=true 的词条。

### 7.5 文档分层分块逻辑（知识库）

```go
// NOTE 按标题层级优先分割
func SplitMarkdown(content string, chunkSize, overlap int) []string {
    // 优先按 # 一级标题分割
    sections := regexp.MustCompile(`(?m)^# .+$`).Split(content, -1)
    var chunks []string
    for _, section := range sections {
        // 二级标题再细分
        subSections := regexp.MustCompile(`(?m)^## .+$`).Split(section, -1)
        for _, sub := range subSections {
            if len(sub) > chunkSize {
                for i := 0; i < len(sub); i += chunkSize - overlap {
                    end := i + chunkSize
                    if end > len(sub) {
                        end = len(sub)
                    }
                    chunks = append(chunks, sub[i:end])
                }
            } else {
                chunks = append(chunks, sub)
            }
        }
    }
    return chunks
}
```

### 7.6 增量更新机制

- 文件入库时计算 md5 哈希；
- 再次导入同文件时按文件名 + 内容 hash 判重；
- 仅对变更文档重新向量化，未变更文档跳过。

### 7.7 知识库管理面板（前端 Vue3）

- 拖拽上传 / 批量导入文件夹；
- 单个文件删除 / 批量清空；
- 文件状态标识：已入库 / 解析失败 / 重复文件；
- 检索参数滑块：分块大小 500-1500（默认 500）、重叠字符 0-200（默认 50）、检索条数 top 1-5（默认 3）；
- 知识库临时关闭开关：本次生成不读取本地文档。

---

## 八、非功能性需求

### 8.1 性能指标

| 指标                 | 目标值         | 说明                              |
| -------------------- | -------------- | --------------------------------- |
| 后台内存占用         | ≤ 80MB         | 不含 Ollama 模型进程              |
| CPU 占用（空闲）     | ≤ 1%           | 监听待机状态                      |
| CPU 占用（生成中）   | ≤ 30%          | LLM 调用期间                      |
| 选中文本捕获响应     | ≤ 300ms        | UI Automation 成功时              |
| LLM 生成响应         | ≤ 3s           | 云端；本地 ≤ 10s                  |
| 逐字输入速度         | 5 字/秒        | 可配置 3-10 字/秒                  |
| 管理面板打开         | ≤ 1s           | WebView2 冷启动                   |
| 配置修改生效         | 秒级           | 实时生效，无需重启                |

### 8.2 隐私与安全

1. **所有数据本地存储**，不上传云端（LLM 调用除外）；
2. **仅监听微信、QQ 选中文本**，不读取文件、相册、隐私文件；
3. **无主动采集、无后台偷跑、无网络私自请求**（LLM 调用除外）；
4. **剪贴板安全**：UI Automation 方案不读写剪贴板；Ctrl+C 备选方案读取后立即清空；
5. **AES 加密**：敏感字段加密存储（P2 阶段，默认关闭）。

### 8.3 可用性

1. **长期后台运行无崩溃、无内存泄漏**；
2. **窗口切换、频繁选中操作无卡顿、无重复触发**；
3. **模拟输入不冲突系统键盘操作**；
4. **全链路异常分支均有中文提示**，不崩溃、不暴露技术堆栈。

### 8.4 兼容性

| 维度         | 规格                                |
| ------------ | ----------------------------------- |
| 操作系统     | Windows 10 / 11（64 位）            |
| 兼容软件     | 微信、QQ 主流版本                   |
| WebView2     | 内置运行时，无需用户安装            |
| 杀毒软件     | 静态编译二进制，误报概率低          |
| 高分屏       | 原生支持                            |
| 暗色模式     | 原生支持                            |

### 8.5 可维护性

1. **前后端解耦**：UI 修改无需改动后端逻辑；
2. **模块化设计**：各模块独立可测试；
3. **配置可视化**：词库、Prompt、LLM 参数全部后台配置；
4. **日志完善**：分级、按天轮转、脱敏输出。

---

## 九、风险与应对

| 编号 | 风险                                         | 等级 | 应对方案                                       |
| ---- | -------------------------------------------- | ---- | ---------------------------------------------- |
| R1   | UI Automation 在新版微信/QQ 兼容性不稳定     | 高   | 双轨降级：UI Automation 失败自动切 Ctrl+C 备选 |
| R2   | 逐字模拟输入中文方案技术难度                 | 中   | SendInput Unicode 方案，不依赖 IME             |
| R3   | 微信/QQ 窗口类名随版本升级变更               | 中   | 管理面板提供自定义配置入口                     |
| R4   | Tab 键与微信/QQ 系统快捷键冲突                | 中   | 可自定义确认键 + 冲突检测弹窗                  |
| R5   | Tauri v2 多窗口架构（悬浮窗+管理面板）可行性     | 低   | Tauri v2 原生支持多窗口 + WS_EX_NOACTIVATE，已验证可行         |
| R6   | LLM 调用失败导致无候选                       | 高   | auto 模式降级本地 + 错误码中文提示              |
| R7   | 词库导入 LLM 分类准确性                      | 中   | 用户可手动修正分类，支持批量编辑               |
| R8   | 逐字输入过程中用户切换窗口                   | 中   | ESC 中断机制，已输入内容保留                   |
| R9   | 长期运行内存泄漏                             | 低   | Rust 内存安全 + tracing 日志监控 + 定期重启机制 |
| R10  | 杀毒软件误报                                 | 低   | Rust 静态编译二进制 + 数字签名（后续）          |

---

## 十、里程碑与迭代规划

### 10.1 阶段一：MVP 核心闭环（P0）

**目标**：选中文本 → 生成候选 → 逐字输入 主流程跑通

**交付功能**：
- F1 窗口识别（双重验证）
- F2 文本捕获（UI Automation + Ctrl+C 备选）
- F3 文本清洗过滤
- F4 LLM 回复生成（云端）
- F5 悬浮候选窗（占位 + 候选 + 刷新）
- F6 逐字模拟输入
- F7 系统托盘
- F8 异常中文提示
- F9 后端管理面板（基础框架）
- F20 功能参数配置

**验收标准**：
1. 微信/QQ 选中文字后 1-3 秒内浮窗显示候选；
2. Tab 键确认后逐字输入到聊天输入框；
3. ESC 可中断输入；
4. 浮窗可关闭（ESC/切窗/点空白）；
5. 异常情况有中文提示。

### 10.2 阶段二：词库 + 习惯 + 词云（P1）

**目标**：词库 RAG + 习惯记忆 + 词云可视化 + 本地模型

**交付功能**：
- F10 Prompt 自定义模板
- F11 词库管理（CRUD + 导入 + LLM 分类）
- F12 词库 RAG 检索（分表共库）
- F13 LLM 模型配置面板
- F14 用户习惯记忆（衰减权重）
- F15 高频词云可视化（近 7 天）
- F16 Ollama 本地模型部署

**验收标准**：
1. 词库支持本地导入，LLM 自动分类；
2. RAG 检索结果可注入 Prompt；
3. 习惯记忆按衰减权重影响生成；
4. 词云展示近 7 天采纳回复高频词；
5. Ollama 本地模型可一键部署；
6. auto 模式可云端失败自动降级本地。

### 10.3 阶段三：知识库 + 加密（P2）

**目标**：知识库文档导入 + AES 加密

**交付功能**：
- F17 知识库文档导入（TXT/MD）
- F18 知识库增量更新
- F19 AES 加密敏感字段

**验收标准**：
1. 支持 TXT/MD 文档导入；
2. 文档分块入向量库；
3. 增量更新（hash 判重）；
4. AES 加密敏感字段，默认关闭可开启。

---

## 十一、附录

### 11.1 关键依赖版本参考

#### 11.1.1 后端 Rust

| 依赖                    | 版本     | 用途                       |
| ----------------------- | -------- | -------------------------- |
| Rust                    | 1.75+    | 主语言（2024 edition）     |
| Tauri                   | 2.x      | 桌面 WebView 载体，多窗口  |
| `windows-rs`            | latest   | Windows API（SendInput/窗口样式） |
| `uiautomation`          | latest   | UI Automation COM 调用     |
| `tokio`                 | 1.x      | 异步运行时                 |
| `reqwest`               | 0.12+    | HTTP 客户端（LLM/ChromaDB）|
| `rusqlite`              | 0.31+    | SQLite 驱动（bundled）     |
| `serde` / `serde_json`  | 1.x      | 序列化                     |
| `aes-gcm`               | 0.10+    | AES-256-GCM 加密           |
| `tracing`               | 0.1+     | 结构化日志                 |
| `tracing-subscriber`    | 0.3+     | 日志订阅器                 |
| `tracing-appender`      | 0.2+     | 日志按天轮转               |
| `regex`                 | 1.x      | 文本清洗                   |
| `pulldown-cmark`        | 0.10+    | Markdown 解析              |
| `tera`                  | 0.19+    | Prompt 模板渲染（可选）    |

#### 11.1.2 前端

| 依赖                | 版本   | 用途                |
| ------------------- | ------ | ------------------- |
| Vue                 | 3.4+   | 前端框架            |
| TypeScript          | 5.x    | 类型安全            |
| Element Plus        | latest | UI 组件库           |
| TailwindCSS         | 3.x    | 样式                |
| Pinia               | latest | 状态管理            |
| `@tauri-apps/api`   | 2.x    | Tauri IPC（invoke/listen）|
| `@tauri-apps/plugin-dialog` | 2.x | 文件选择对话框     |
| ECharts             | 5.x    | 图表                |
| echarts-wordcloud   | latest | 词云插件            |

### 11.2 标准 Prompt 模板

```
你是一个聊天回复助手。请根据以下聊天内容，生成 3 条简短、自然的回复候选。

【对方消息】
{{selected_text}}

【风格参考】
{{lexicon_words}}

【知识背景】
{{rag_context}}

【回复示例】
{{habit_examples}}

要求：
1. 每条回复不超过 50 字
2. 语气自然，贴合个人风格
3. 直接输出回复内容，不要编号，用换行分隔
```

### 11.3 全链路异常错误码与前端提示对照表

| 错误码 | 场景                          | 前端提示                             |
| ------ | ----------------------------- | ------------------------------------ |
| E001   | 非微信/QQ 窗口触发            | （静默不提示）                       |
| E002   | UI Automation 获取失败        | （静默降级 Ctrl+C）                  |
| E003   | Ctrl+C 备选也失败             | 文本捕获失败，请重新选中文本         |
| E004   | 选中文本为空或过短            | （静默不提示）                       |
| E005   | LLM 云端调用超时              | AI 响应超时，正在切换本地模型…       |
| E006   | LLM 云端余额不足              | 云端模型余额不足，已切换本地模型     |
| E007   | LLM 本地未启动                | 本地模型未启动，请检查 Ollama         |
| E008   | LLM 全部失败                  | AI 生成失败，请稍后重试               |
| E009   | 逐字输入被中断                | （静默，已输入内容保留）             |
| E010   | 窗口类名未识别                | 当前窗口暂不支持，可在设置中添加     |
| E011   | 词库导入失败                  | 词库导入失败：{原因}                  |
| E012   | 知识库文档解析失败            | 文档解析失败：{原因}                  |
| E013   | 向量库损坏                    | 数据库异常，已自动重建，请重新导入    |
| E014   | 配置保存失败                  | 配置保存失败，请检查文件权限          |
| E015   | AES 加密初始化失败            | 加密功能初始化失败，已回退明文存储    |

### 11.4 项目目录结构

```
创意输入法/
├── src-tauri/                    # Tauri v2 后端（Rust）
│   ├── src/
│   │   ├── main.rs               # Tauri 主入口
│   │   ├── lib.rs                # 模块入口
│   │   ├── commands/             # Tauri commands（前端可调用）
│   │   │   ├── mod.rs
│   │   │   ├── config.rs         # 配置相关 command
│   │   │   ├── lexicon.rs        # 词库相关 command
│   │   │   ├── prompt.rs         # Prompt 模板 command
│   │   │   ├── llm.rs            # LLM 配置 command
│   │   │   ├── reply.rs          # 回复生成 command
│   │   │   ├── history.rs        # 历史记录 command
│   │   │   └── system.rs         # 系统状态 command
│   │   ├── capture/              # 文本捕获
│   │   │   ├── mod.rs
│   │   │   ├── window.rs         # 窗口识别（进程名+类名）
│   │   │   ├── uiautomation.rs   # UI Automation
│   │   │   └── clipboard.rs      # Ctrl+C 备选
│   │   ├── cleaner/              # 文本清洗
│   │   ├── llm/                  # LLM 服务
│   │   ├── rag/                  # 词库 RAG
│   │   ├── habit/                # 习惯记忆
│   │   ├── keyboard/             # 键盘模拟（SendInput Unicode）
│   │   ├── prompt/               # Prompt 模板渲染
│   │   ├── config/               # 配置存储
│   │   ├── crypto/               # AES 加密
│   │   ├── orchestrator/         # 主流程调度
│   │   ├── tray/                 # 系统托盘
│   │   ├── window/               # 窗口管理（多窗口、悬浮窗 WS_EX_NOACTIVATE）
│   │   └── error.rs              # 统一错误类型
│   ├── Cargo.toml                # Rust 依赖
│   ├── tauri.conf.json           # Tauri 配置
│   ├── build.rs                  # 构建脚本
│   └── icons/                    # 应用图标
├── src/                          # Vue3 前端源码
│   ├── views/                    # 管理面板页面
│   │   ├── Dashboard.vue         # 仪表盘
│   │   ├── Lexicon.vue           # 词库管理
│   │   ├── Prompt.vue            # Prompt 模板
│   │   ├── LlmConfig.vue         # LLM 配置
│   │   ├── Habit.vue             # 习惯记忆
│   │   ├── WordCloud.vue         # 词云
│   │   └── Settings.vue          # 设置
│   ├── overlay/                  # 悬浮窗独立应用
│   │   ├── Overlay.vue           # 悬浮候选窗
│   │   ├── main.ts               # 悬浮窗入口
│   │   └── index.html            # 悬浮窗 HTML
│   ├── components/               # 共享组件
│   ├── stores/                   # Pinia 状态
│   ├── api/                      # Tauri invoke 封装
│   ├── App.vue                   # 管理面板根组件
│   ├── main.ts                   # 管理面板入口
│   └── index.html                # 管理面板 HTML
├── package.json                  # 前端依赖
├── vite.config.ts                # Vite 配置（双入口）
├── tsconfig.json
├── docs/                         # 文档
├── scripts/                      # 脚本
└── README.md
```

### 11.5 待用户评审决策项（v5.0）

| 编号 | 决策项                                   | PM 建议                     | 等待确认 |
| ---- | ---------------------------------------- | --------------------------- | -------- |
| D1   | 监听方案：UI Automation + Ctrl+C 备选    | 推荐采纳                    | ☐        |
| D2   | Ctrl+C 主体：程序自动模拟                | 推荐采纳                    | ☐        |
| D3   | v3.0 模块：废弃截图/OCR/场景标签/备份/向导 | 推荐采纳                    | ☐        |
| D4   | 保留 RAG/Ollama/习惯记忆/AES             | 推荐采纳                    | ☐        |
| D5   | 词库角色：变量注入 + 语义检索            | 推荐采纳                    | ☐        |
| D6   | 词库与 RAG：合并为一个系统                | 推荐采纳                    | ☐        |
| D7   | 词云与习惯：词云=习惯记忆可视化          | 推荐采纳                    | ☐        |
| D8   | 架构：**Rust + Tauri v2 + Vue3**（v5.0 调整，原 Go+Wails 弃用） | 推荐采纳          | ☐        |
| D9   | 逐字输入：5 字/秒，50-150ms 随机延迟      | 推荐采纳                    | ☐        |
| D10  | 浮窗位置：选区右下角，避让屏幕边缘        | 推荐采纳                    | ☐        |
| D11  | 窗口架构：**Tauri v2 原生多窗口 + WS_EX_NOACTIVATE 悬浮窗**（v5.0 调整） | 推荐采纳 | ☐        |
| D12  | AES 加密：仅敏感字段，默认关闭，P2        | 推荐采纳                    | ☐        |
| D13  | 词云数据：仅采纳回复，近 7 天             | 推荐采纳                    | ☐        |
| D14  | 防抖：500ms，30 秒同文本不重复            | 推荐采纳                    | ☐        |
| D15  | Ctrl+C 冲突：仅失败时介入                | 推荐采纳                    | ☐        |
| D16  | 浮窗时机：立即显示占位                    | 推荐采纳                    | ☐        |
| D17  | 输入中断：ESC 中断                       | 推荐采纳                    | ☐        |
| D18  | Tab 键：默认，可自定义 + 冲突检测         | 推荐采纳                    | ☐        |
| D19  | 窗口识别：进程名 + 类名双重验证           | 推荐采纳                    | ☐        |
| D20  | 词库 RAG 结构：分表共库                   | 推荐采纳                    | ☐        |
| D21  | Ollama 模式：auto 可选可关               | 推荐采纳                    | ☐        |
| D22  | 刷新机制：浮窗内刷新按钮                 | 推荐采纳                    | ☐        |
| D23  | 启动行为：每次启动打开管理面板           | 推荐采纳                    | ☐        |
| D24  | 候选数量：默认 3 条可配置                | 推荐采纳                    | ☐        |
| D25  | 词云周期：默认近 7 天                     | 推荐采纳                    | ☐        |
| D26  | 词库导入：本地导入 + LLM 整理分类        | 推荐采纳                    | ☐        |

---

**文档结束。请用户评审以上内容，特别关注：**

1. **第三章 3.1 节**：Rust + Tauri v2 + Vue3 架构调整是否合理（v5.0 核心变更）
2. **第三章 3.2 节**：UI Automation + Ctrl+C 双轨捕获方案是否落地可行
3. **第三章 3.6 节**：SendInput Unicode 逐字模拟输入方案是否可行
4. **第四章 4.2 节**：模块清单是否完整，职责是否清晰
5. **第五章**：各模块前后端职责划分是否清晰
6. **第十一章 11.5 节 D1-D26**：26 项决策项请确认（特别是 D8、D11 已根据 v5.0 架构调整）
