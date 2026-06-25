# Pathos — 实施路线图

> 与 [architecture.md](./architecture.md) 中的技术方案配套，随开发进度更新。

## Phase 1: 核心引擎 MVP（已完成）

- [x] 搭建 Cargo workspace（`members = ["crates/*"]`）和全部 10 个 crate 骨架
- [x] `pathos-core`：AST 类型定义（`ContentNode`、`StoryState`、`StoryConfig`、`PassageGraph`、`HookRegistry`）
- [x] `pathos-core`：`ScriptEngine` enum（含 Rhai 引擎初始化、`eval()` dispatch；JS/Lua feature gate 预留给后续 Phase）
- [x] `pathos-core`：`MacroHandler` trait + `MacroContext` + 内建宏实现（`SetMacro`、`DisplayMacro`、`PrintMacro`、`ForMacro`、`SwitchMacro`）
- [x] `pathos-core`：`NarrativeRuntime` 结构体 + 基础叙事执行循环（`step()` / `submit_input()` / `feed_stream_token()` / `end_stream()`）
- [x] `pathos-core`：状态管理（`Value` 类型、路径访问、变更追踪 `StateChange`、`Global`/`Temp` scope）
- [x] `pathos-core`：表达式求值器（字面量、运算符、函数调用 `random()` / `has_tag()`）
- [x] `pathos-core`：单元测试 51 个（`StoryState`、`Value`、`Expression` 全覆盖）
- [x] `pathos-parser`：多格式解析框架（`FormatParser` trait + `FormatRegistry` 按扩展名自动分发；TOML 格式完整实现含测试）
- [x] `pathos-parser`：`.pathos` 格式四遍解析（P1 分段 → P2 块解析 → P3 行内解析 → P4 语义分析）
- [x] `pathos-parser`：`@hook:` directive + fenced code block（含 language tag）解析
- [x] `pathos-parser`：`{if:}`、`{ai:}`、`{ai-stream:}`、`{ai-cached:}`、`{state:}`、`{display:}`、`{set:}` 行内指令解析
- [x] `pathos-parser`：`[[link]]` 链接解析（含 `->` 和 `→` 两种箭头写法）
- [x] `pathos-parser`：20 个测试全部通过（含 inline、pathos、toml 格式 + 诊断验证）
- [x] `pathos-render`：`RenderBackend` trait 定义 + `MockBackend` 实现
- [x] `pathos-tui`：Ratatui 终端渲染器（`TuiBackend` 实现 `RenderBackend`，含完整事件循环）
- [x] `pathos-cli`：`pathos run`（交互式运行）+ `pathos check`（静态诊断检查）
- [x] 集成测试：6 个测试覆盖解析 + 运行时执行 + 诊断验证（含 3 秒超时守卫）
- [x] Bug 修复：前导空行导致标题丢失问题 + 行内解析器 pos=0 无限循环问题

### 测试总结

| 测试套件 | 数量 | 状态 |
|---|---|---|
| `pathos-core` 单元测试 | 51 | ✅ 全过 |
| `pathos-parser` 测试 | 20 | ✅ 全过 |
| `pathos-render` 测试 | 3 | ✅ 全过 |
| 集成测试 | 6 | ✅ 全过（含超时守卫） |
| 其它 crate 骨架测试 | 4 | ✅ 全过 |

## Phase 2: 脚本与 LLM（预计 3–4 周）

- [ ] `pathos-core`：Rhai 脚本 API 注册（`state.get/set/inc/dec/delete`、`game.goto/back/restart/visited/count`、`random/random_float`、`ai.prompt`）
- [ ] `pathos-core`：多语言脚本块解析完善（` ```rhai ` ` ```js ` ` ```lua ` → 对应 `ScriptLang`）
- [ ] `pathos-llm`：LLM 调用库搭建（`LLMConfig` 类型 + OpenAI / Anthropic / Ollama provider）
- [ ] `pathos-llm`：Mock provider（预置文本回放，默认启用）
- [ ] `pathos-llm`：流式调用支持（逐 token 回调接口）
- [ ] `pathos-tui`：LLM 调用循环集成（收到 `WaitingForStream` → 异步调用 `pathos-llm` → token feed 回引擎）
- [ ] `pathos-tui`：Fallback 渲染（`StreamFailed { fallback }` 显示）
- [ ] `pathos-tui`：`{ai-cached:}` 支持（首次调用后缓存到内存/磁盘，重复请求直接返回）
- [ ] `.twee` 导入器（`:: passage` → `# passage`、`<<set>>` → `{set:}`、`<<link>>` → `[[link]]`）
- [ ] 属性测试（`proptest`）：表达式求值器 1000+ 随机输入 + 状态机
- [ ] 钩子系统集成测试（`@hook:` 注册 + 多语言脚本执行 + 状态变更验证）

## Phase 3: 完整生态（预计 4–6 周）

- [ ] `pathos-web`：Leptos WASM 浏览器客户端，完整 UI
- [ ] `pathos-web`：a11y 实现（语义 HTML、ARIA live region、键盘导航、焦点管理、流式节流）
- [ ] `pathos-web`：LLM 调用集成（fetch 转发到服务端或浏览器内 WASM 线程）
- [ ] `pathos-mod`：Mod 加载系统，`mod.toml` 清单解析
- [ ] `pathos-mod`：覆盖/补丁执行（override / prepend / append / replace / inject_hook）
- [ ] `pathos-mod`：自定义宏注册（`[macros]` 段 → `RhaiMacroHandler` → 注入 `NarrativeRuntime.macros`）
- [ ] `pathos-mod`：依赖拓扑排序 + 冲突管理（优先级：conflicts > ordering > priority > 字典序）
- [ ] `pathos build`：构建产物打包（含 LLM 缓存预生成 `{ai-cached:}`）
- [ ] `pathos pack`：打包 `.pathos-mod` 归档
- [ ] 存档/读档（`SaveFile` MessagePack 序列化 + 版本兼容契约）
- [ ] WASM 二进制体积优化（目标 gzip ≤500KB；Rhai minimal、winnow 无 verbose error、LLM provider 排除、JS/Lua feature gate 默认关闭）

## Phase 4: 打磨与生态（持续）

- [ ] `pathos-lsp`：LSP 服务器（语法检查、补全、跳转；利用 parser 的 `(PartialAst, Vec<Diagnostic>)` 产出提供实时诊断）
- [ ] `pathos-gui`：egui 原生桌面客户端
- [ ] VS Code / Zed 语法高亮扩展
- [ ] 性能基准测试与 WASM binary 体积优化
- [ ] JS 脚本引擎评估与集成（`boa` feature gate，评估体积/性能代价，启动 `pathos build --features script-js` 构建验证）
- [ ] Lua 脚本引擎评估与集成（`mlua` 在 CLI/服务端场景的可选 feature gate）
- [ ] 安全模型加固（脚本资源限制压测、LLM prompt injection 测试、存档反序列化健壮性测试）
- [ ] 文档站点 + 教程
