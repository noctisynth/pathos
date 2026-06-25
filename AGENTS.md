## 项目概览

Pathos 是一个用 Rust 编写的交互式叙事引擎。工作区结构：`crates/*`（10 个 crate），根 `Cargo.toml` 用 `members = ["crates/*"]`。

核心 crate：`pathos-core`（状态机、表达式、脚本、宏、运行时），`pathos-parser`（.pathos / TOML 解析），`pathos-render`（渲染 trait），`pathos-tui`（终端渲染），`pathos-cli`（CLI 入口）。

## 关键约定

- **依赖添加必须用 `cargo add`**，不要手写 `Cargo.toml` 中的依赖项。
- **各 crate 独立维护 `[package]` 版本号**，不使用 `version.workspace`。
- **测试必须加超时守卫**——本项目历史上出现过解析器无限循环导致系统崩溃的事故。对可能超时的测试用 `with_timeout` 包装（见集成测试），对外部命令用 `timeout 30 cargo test ...`。`timeout` 命令通过 Homebrew 安装。
- **跑完整测试套件用 `cargo test --all -- --test-threads=1`**，避免并行竞争导致内存压力。

## 架构要点

- `ScriptEngine` 是 **enum**（Rhai/JS/Lua），不是 trait。Rhai 始终编译，JS/Lua 在 feature gate 后。
- LLM 配置**由用户运行时提供**（CLI flag / 环境变量 / Web 设置面板），不在 `.pathos` 文件中。
- `MacroHandler` trait 是 core 内唯一对外扩展点——内建宏和 Mod 注册的 Rhai 宏都走这个接口。
- `RenderBackend` trait 是 core 与渲染端之间的唯一跨 crate trait。引擎是同步的，LLM 调用由渲染后端异步驱动。
- Phase 1 没有 LLM：`walk_content` 遇到 AIBlock 直接渲染 fallback，不走 `WaitingForStream`。

## 常见事故与修复

### 无限循环 / 内存耗尽
- **原因**：`step()` 返回 `WaitingForStream` → TUI 调 `end_stream`（fallback 未设置）→ 循环回去再 `step()` → 再次撞到同一个 AIBlock。`ai_fallback` 从未被赋值。
- **修复**：Phase 1 的 `walk_content` 直接把 AIBlock fallback 渲染为 `StreamFailed`，不走异步流程。`step()` 尾部 AIBlock 检查循环已删除。

### 按键无响应
- **原因**：`step()` 用 `std::mem::take(&mut self.pending_choices)` 把 choices 掏空后放入 Render，随后 `submit_input` 调用 `self.pending_choices.get(idx)` 时拿到空 Vec，按键被静默丢弃。
- **修复**：改成 `self.pending_choices.clone()` 保留原数据，`submit_input` 在用户做出选择后才 `clear()`。

### `Value::Null` 默认导致的 `state.set` 失败
- **原因**：`StoryState::default()` 把 `globals` / `temp` 初始化为 `Value::Null`，对 Null 调 `set("x", val)` 会走 `as_mut_nested` 返回 `None`。
- **修复**：`StoryState::default()` 手动实现，`globals` 和 `temp` 初始为 `Value::Object(HashMap::new())`。

### 嵌套路径 set 失败
- **原因**：`Value::as_mut_nested` 创建中间节点时用了 `Value::Null`，下级 `set` 无法匹配 `Object` 分支。
- **修复**：中间节点改为 `Value::Object(HashMap::new())`。

### 原始字符串中的 `\u{2192}` 不被解释
- **原因**：`r#"..."#` 内 `\u{2192}` 是字面文本，不是 Unicode →。解析器找不到箭头，链接解析失败。
- **修复**：用普通字符串 `"..."` 而非 raw string；或直接用 `->` 箭头。（解析器同时支持 `->` 和 `→`。）

### 前导空行吞掉 passage 标题
- **原因**：frontmatter `---` 后的空行被 `split_into_passage_blocks` 带入块内容，`parse_passage_block` 把空行当标题，真正的 `# heading` 被归入正文。
- **修复**：`split_into_passage_blocks` 增加 `seen_content` 标志，在第一个标题或正文行之前跳过空行。

## 测试守则

- 所有单元测试放在各 crate 的 `src/` 内（`#[cfg(test)] mod tests`）。
- 跨 crate 集成测试放在 `pathos-core/tests/`。
- 解析器相关测试用 `with_timeout` 包装，防止无限循环导致系统崩溃。不涉及 `NarrativeRuntime`（非 `Send`）的解析测试可以安全 spawn 线程加超时；运行时测试内联执行即可。
- `cargo test --all` 之前确保没有残留的 cargo 进程（`pkill -9 cargo` 以防 Cargo.lock 冲突）。

## 事故文档

重大 bug 和修复记录在 `docs/incident-report-*.md`。工作进度跟踪在 `docs/TODO.md`。
