# Pathos — 技术架构设计

> 本文档定义 Pathos 交互式叙事引擎的技术架构、核心设计和关键决策。
> 与 [TODO.md](./TODO.md) 配套，后者跟踪实施进度。


## 1. 项目愿景

Pathos 是一个**现代的、可嵌入的交互式叙事引擎**，用 Rust 编写。目标是成为 SugarCube / Twine 生态在 Rust 世界的精神继任者，同时在工程实践、扩展性和 LLM 集成上实现代际跨越。

### 设计原则

- **客户端无关** — 核心引擎不关心渲染目标；CLI、Web(WASM)、GUI、甚至 Discord Bot 都应视为一等客户端
- **分层可扩展** — 叙事作者用简单格式写作；进阶用户用脚本语言扩展；贡献者用 Rust 开发渲染后端
- **LLM 友好** — 引擎原生支持 AI 生成占位语法，实际调用由渲染后端驱动，核心引擎不耦合 LLM 实现
- **格式兼容** — 原生支持 `.pathos` 格式，同时完整兼容 Twee 3 格式的导入/导出
- **MOD 友好** — 支持覆盖、补丁、依赖解析、自定义宏注册
- **零 I/O 核心** — `pathos-core` 不依赖任何 I/O、异步运行时、或渲染库，纯状态机


## 2. 总体架构

```
                          ┌──────────────────┐
                          │    .pathos 文件    │
                          │   (YAML+MD 扩展)   │
                          └────────┬─────────┘
                                   │
                          ┌────────▼─────────┐
                          │   pathos-parser   │
                          │  winnow 组合子     │
                          │  文本 → AST       │
                          └────────┬─────────┘
                                   │
 ┌──────────────┐         ┌───────▼──────────────┐
 │  pathos-mod   │────────│     pathos-core       │
 │ 覆盖/补丁/宏   │        │ 状态/图/钩子/脚本/宏   │
 └──────────────┘         └───────┬──────────────┘
                                   │
                   ┌───────────────┼───────────────┐
                   │                               │
            ┌──────▼──────┐                 ┌──────▼──────┐
            │ pathos-render│                │  pathos-llm   │
            │ RenderBackend│                │ LLM 调用库    │
            │ trait        │                │ (后端使用)    │
            └──────┬──────┘                 └──────┬──────┘
                   │                               │
       ┌───────────┼───────────┐                   │
       │           │           │                   │
  ┌────▼───┐ ┌────▼───┐ ┌────▼───┐                │
  │  tui   │ │  web   │ │  gui   │                │
  │Ratatui │ │Leptos  │ │ egui   │                │
  └────┬───┘ └────┬───┘ └────┬───┘                │
       │          │          │                     │
       └──────────┼──────────┘                     │
                  │                                │
                  └───────────┬────────────────────┘
                              │
                         (用户交互)
```

**数据流向**：文件 → 解析器 → AST → 核心引擎 → 渲染后端 → 用户交互 → 引擎状态变更 → 循环。

**关键约束**：
- `pathos-core` 零 I/O，不持有异步运行时，不依赖任何渲染库
- 脚本引擎和宏系统直接内嵌在 core 中（纯 Rust 依赖，不引入额外的 trait 抽象层）
- `RenderBackend` trait 是 core 与外部之间唯一的跨 crate trait
- 引擎本身是同步的（叙事流天然是线性的），LLM 调用由渲染后端异步驱动，引擎侧仅产出 `WaitingForStream` 信号


## 3. Crate 设计

### 3.0 Workspace 配置

根 `Cargo.toml` 使用 `members = ["crates/*"]` 引入所有子 crate。各 crate 在各自的 `Cargo.toml` 中独立声明 `[package]` 版本号，**不使用 `version.workspace`** 统一版本。所有依赖添加通过 `cargo add` 命令，避免手动写入版本号。

```
crates/
├── pathos-core       # 核心类型、状态机、passage 图、钩子系统、脚本引擎、宏系统、叙事运行时
├── pathos-parser     # .pathos 解析器 + .twee 导入器，产出 core AST
├── pathos-llm        # LLM 调用库：OpenAI / Anthropic / Ollama / Mock，供渲染后端使用
├── pathos-render     # 渲染抽象层：RenderBackend trait + 公共渲染工具
├── pathos-mod        # Mod 加载、覆盖、补丁、自定义宏注册、依赖解析
├── pathos-tui        # 终端渲染器（Ratatui）
├── pathos-web        # WASM 浏览器渲染器（Leptos）
├── pathos-gui        # 原生 GUI 渲染器（egui）
├── pathos-cli        # CLI 二进制入口：pathos run / build / check / pack
└── pathos-lsp        # LSP 服务器（语法检查、跳转、补全）
```

> **注意**：`pathos-script` 和 `pathos-macro` 已合并入 `pathos-core`。脚本引擎是纯 Rust enum 而非 trait 抽象，宏系统使用统一的 `MacroHandler` trait（core 内唯一对外扩展 trait）。

### 3.1 `pathos-core` — 核心引擎

**职责**：定义所有核心类型、状态机、passage 图、钩子系统、脚本引擎、宏系统、叙事执行循环。

**不包含**：文件 I/O、异步运行时、渲染、LLM 网络调用。

**核心类型**：

```rust
/// 叙事状态
pub struct StoryState {
    /// 跨 passage 持久变量，存档时序列化。
    /// 路径访问示例：state.get("player.inventory.0.name")
    pub globals: Value,
    /// 故事级元数据（从 .pathos frontmatter 提取）：
    /// title, author, version 等。由引擎在初始化时设置，脚本只读。
    pub metadata: Value,
}

pub enum Scope { Global, Temp }

/// 游戏配置（从 .pathos frontmatter 提取，不包含 AI 配置）
pub struct StoryConfig {
    pub title: String,
    pub author: String,
    pub start: PassageId,
    pub version: String,
    pub save_slots: u8,
}
```

**脚本引擎**（enum，多语言对等，非 trait 抽象）：

```rust
/// 多语言脚本引擎。Rhai 始终编译；JS/Lua 由 feature gate 控制。
pub enum ScriptEngine {
    Rhai(rhai::Engine),
    #[cfg(feature = "script-js")]
    Js(boa_engine::Context),
    #[cfg(feature = "script-lua")]
    Lua(mlua::Lua),
}

pub enum ScriptLang {
    Rhai,
    Js,
    Lua,
}

impl ScriptEngine {
    /// 按 language tag 执行脚本。lang 决定 dispatch 到哪个内部引擎。
    pub fn eval(
        &self,
        state: &mut StoryState,
        code: &str,
        lang: ScriptLang,
    ) -> Result<Value, ScriptError>;
}
```

`ScriptLang` 由解析器从 fenced code block 的 info string 提取：` ```rhai ` → `ScriptLang::Rhai`；` ```js ` → `ScriptLang::Js`。三个语言在 enum 中完全对等，dispatch 靠 `lang` tag。

**宏系统**（统一的 dyn trait——内建和 Mod 注册的宏走同一接口）：

```rust
/// 宏执行上下文，由引擎在求值 {name: args} 时构造
pub struct MacroContext<'a> {
    pub state: &'a mut StoryState,
    pub script: &'a ScriptEngine,
    pub args: &'a [MacroArg],
}

pub trait MacroHandler: Send + Sync {
    /// 宏名，用于匹配 {name: ...}
    fn name(&self) -> &str;
    /// 执行宏，产出 ContentNode 列表或副作用
    fn handle(&self, ctx: &mut MacroContext) -> Result<MacroResult, MacroError>;
}
```

- **内建宏**（`SetMacro`、`DisplayMacro`、`PrintMacro`、`ForMacro`、`SwitchMacro`）是 core 内部的 `pub(crate)` struct，均实现 `MacroHandler`
- **Mod 注册的 Rhai 宏**由 `pathos-mod` 构造一个 `RhaiMacroHandler` struct，同样 impl `MacroHandler`，内部通过 `ctx.script.eval()` 执行 Rhai 脚本
- `NarrativeRuntime` 持有 `macros: Vec<Box<dyn MacroHandler>>`，`step()` 在遇到 `{custom: args}` 时查表调用

**叙事运行时**：

```rust
pub struct NarrativeRuntime {
    pub config: StoryConfig,
    pub graph: PassageGraph,
    pub state: StoryState,
    pub script: ScriptEngine,
    pub hooks: HookRegistry,
    pub macros: Vec<Box<dyn MacroHandler>>,
    pub history: RingBuffer<StateSnapshot>,
    pub turn: u64,
}
```

- `script` 字段始终存在（含 Rhai），非 Option
- `macros` 中内建宏与 Mod 注册的宏混排，运行时所有 `{name: args}` 都走统一的 `MacroHandler` 分发

### 3.2 其他 crate 职责摘要

| Crate | 依赖 | 说明 |
|---|---|---|
| `pathos-parser` | `core` | winnow 组合子实现 `.pathos` 和 `.twee` 解析；`miette` 做错误诊断；产出一对 `(PartialAst, Vec<Diagnostic>)`，支持 LSP 级错误恢复（每个 pass 独立收集诊断，不 bail-out） |
| `pathos-llm` | — | LLM 调用封装：OpenAI / Anthropic / Ollama / Mock。纯后端库，不依赖 core；提供同步和流式接口供渲染后端调用 |
| `pathos-render` | `core` | `RenderBackend` trait 定义 + `RenderCommand` / `StepResult` / `Choice` 枚举；通用渲染辅助 |
| `pathos-mod` | `core`, `parser` | 加载 mod 目录/zip，解析 `mod.toml`，执行覆盖/补丁/自定义宏注册/依赖拓扑排序 |
| `pathos-tui` | `core`, `render`, `llm`, `ratatui` | 终端交互式阅读器，内嵌 LLM 调用循环 |
| `pathos-web` | `core`, `render`, `llm`, `leptos` | WASM 浏览器客户端，LLM 调用转发到服务端或浏览器内 WASM 线程 |
| `pathos-gui` | `core`, `render`, `egui` | 原生桌面客户端 |
| `pathos-cli` | `tui`, `parser`, `mod`, `clap` | CLI 工具入口；LLM 配置通过 CLI flag / 环境变量传入 |


## 4. `.pathos` 文件格式

### 4.1 设计目标

- 对新人友好：写作体验接近 Markdown
- 编辑器原生高亮：利用 YAML frontmatter + ATX heading + fenced code block
- 结构化：元数据、内容、脚本、AI 指令清晰分离
- 可增量解析：支持 LSP 和热重载
- 兼容 `.twee`：通过导入路径而非混用格式

### 4.2 格式规范示例

```pathos
---
pathos: "1.0"
title: "星落之森"
author: "作者名"
start: "intro"
version: "0.1.0"
save_slots: 10
---

# intro {opening, forest}

你站在一片古老的森林边缘。

夕阳的余晖穿过树冠，{ai: 用感官细节描述此时此地的氛围，50 字以内 | 斑驳的光影在地面摇曳，空气中弥漫着松脂与泥土的气息。}

两条小径延伸向黑暗深处：

- [[跟随发光的蘑菇 → mushroom_path]]
- [[走向幽暗的灌木丛 → dark_thicket]]

---

# mushroom_path {forest, exploration}

\`\`\`rhai
let wisdom = state.get("wisdom").unwrap_or(0);
state.set("wisdom", wisdom + 1);
\`\`\`

蘑菇闪烁着柔和的蓝光。你感到某种古老的智慧在心中苏醒。

> 智慧 +1（当前：{state: "wisdom"}）

[[继续深入 → deep_forest]]
[[原路返回 → intro]]

---

# deep_forest {forest, danger}

{if: state.get("wisdom").unwrap_or(0) > 2}
  你凭借积累的智慧，识别出前方树丛中的陷阱。

  [[小心绕过 → safe_clearing]]
{else}
  黑暗笼罩一切，你隐约感到不安。

  [[继续前进 → trap_encounter]]
  [[返回蘑菇小径 → mushroom_path]]
{end}

---

# init {system}
@hook: on_story_init
\`\`\`rhai
state.set("wisdom", 0);
state.set("courage", 0);
state.set("has_torch", false);
\`\`\`

---
```

> **注意**：`.pathos` frontmatter 中**不包含 AI 配置**（API key、provider、model 等）。这些由用户运行时通过 CLI flag、环境变量或 Web 设置面板传入。

### 4.3 语法规则

| 元素 | 语法 | 示例 |
|---|---|---|
| 元数据 | YAML frontmatter `---...---` | `title: "故事名"` |
| Passage 头 | `# name {tag1, tag2}` | `# intro {opening}` |
| Passage 分隔 | `---` | |
| 链接 | `[[文本 → 目标]]` | `[[进入房间 → room]]` |
| LLM 插值 | `{ai: prompt \| fallback}` | `{ai: 描述房间 \| 一间昏暗的房间。}` |
| 状态插值 | `{state: "path"}` | `{state: "player.hp"}` |
| 条件块 | `{if: expr}...{else}...{end}` | |
| 宏调用 | `{set: key = value}` | `{set: player.hp = 10}` |
| 脚本块 | ` ```lang ... ``` ` | ` ```rhai ... ``` ` |
| 钩子注册 | `@hook: event_name` 紧接 ` ```lang ... ``` ` | `@hook: on_passage_start` |
| 注释 | `// 单行` 或 `<!-- 块 -->` | |

#### `@hook` 语法详解

一个 passage 可包含任意数量的 `@hook:` 块，每个块在下一个 block 级元素处自动结束：

```
@hook: event_name
```rhai
// 脚本内容
```

- `@hook:` 后接事件名（内置事件名或自定义字符串）
- 下一行必须紧跟以 ` ```rhai `（或 ` ```js ` / ` ```lua `）开头的 fenced code block
- 代码块以 ` ``` ` 闭合
- 钩子块在遇到下一个 block 级元素（`@hook:` / `---` / `# heading` / EOF）时自动结束
- 钩子脚本具有完整的状态读写权限：`state.get/set/inc/dec` 等 API 均可调用

**示例：钩子触发状态变更（-10hp）**

```pathos
# damage_event {combat}
@hook: on_player_damaged
\`\`\`rhai
let hp = state.get("player.hp").unwrap_or(100);
state.set("player.hp", hp - 10);
\`\`\`

你受到了 10 点伤害！当前 HP：{state: "player.hp"}
---
```

### 4.4 与 Twee 3 的关系

`.twee` 格式通过 `pathos-parser` 的 Twee 子解析器导入，转换为相同的 `PassageGraph` AST。`.pathos` 是一等格式，`.twee` 是兼容格式。

**转换映射**：
- `:: StoryTitle` → `title:` frontmatter
- `:: passage [tags]` → `# passage {tags}`
- `<<set $var = val>>` → `{set: var = val}` 或脚本块
- `<<link "text" "target">>` → `[[text → target]]`
- `<<display "passage">>` → `{display: "passage"}`

### 4.5 解析流水线

解析分四遍执行，每遍独立收集错误，支持 LSP 级错误恢复（每个 pass 输出 `(PartialAst, Vec<Diagnostic>)` 而非 `Result<Ast, Error>`）：

| 遍 | 输入 | 输出 | 职责 |
|---|---|---|---|
| **P1: 分段** | 原始文本 | `(Frontmatter, Vec<PassageBlock>)` | 提取 YAML frontmatter，按 `---` 和 `# heading` 切分 passage |
| **P2: 块解析** | `PassageBlock` | `(PassageHeader, Vec<BlockElement>)` | 识别 `# name {tags}` 标题行；解析 `@hook:` directive 行 + 紧跟的 fenced code block；提取段落、注释 |
| **P3: 行内解析** | `BlockElement` | `Vec<ContentNode>` | 解析 `{if:}`、`{ai:}`、`{state:}`、`{display:}`、`{set:}` 等行内指令和 `[[link]]` |
| **P4: 语义分析** | `Vec<PassageNode>` | `PassageGraph` + 诊断 | 构建图边、验证 `{if:}` 表达式、检查钩子引用完整性、孤立 passage 检测 |

### 4.6 ContentNode — 内容 AST

```rust
/// 叙事内容的 AST 节点——parser (P3) 产出，core 执行
pub enum ContentNode {
    /// 静态文本
    Text(String),
    /// Passage 链接 [[label → target]]
    Link {
        label: String,
        target: PassageId,
        /// 可选：显示条件表达式
        enabled_if: Option<Expression>,
    },
    /// 状态变量插值 {state: "player.hp"}
    StateInterp { path: String },
    /// 条件块 {if: expr} ... {else} ... {end}
    Conditional {
        condition: Expression,
        then_branch: Vec<ContentNode>,
        else_branch: Vec<ContentNode>,
    },
    /// 子 passage 嵌入 {display: "passage_name"}
    Display { passage: PassageId },
    /// AI 生成块 {ai: | ai-stream: | ai-cached:}
    AIBlock {
        mode: AIMode,
        prompt: String,
        fallback: String,
        cache_key: Option<String>,
    },
    /// 通用宏调用 {name: args}（内建宏和自定义宏统一走 MacroHandler 分发）
    Macro { name: String, args: Vec<MacroArg> },
}

pub enum AIMode {
    Blocking,
    Streaming,
    Cached,
}
```

**设计说明**：
- `Conditional` 是结构性节点（影响执行流），不走 `MacroHandler`
- `{set:}`、`{print:}` 等内建宏和 Mod 注册的自定义宏**统一走 `MacroHandler::handle()`** 分发
- `Link.enabled_if` 为 `None` 时链接始终可用；有表达式时求值为 `false` 则链接灰显不可点击


## 5. 核心引擎设计 (`pathos-core`)

### 5.1 叙事执行循环

```
1. 用户选择/触发导航 → target_passage
2. runtime.navigate_to(target_passage)
3. 检查 passage 存在性
4. 触发 on_passage_start 钩子
5. 执行 passage 关联脚本（按 language tag 分发）
6. 触发 on_passage_render 钩子
7. 解析内容 AST：
   - 静态文本 → push 到 render buffer
   - {ai:} 指令 → 产出 StepResult::WaitingForStream（由后端执行实际 LLM 调用）
   - {if:} 块 → 求值条件，选择分支
   - [[link]] → 收集为 choice（enabled_if 决定是否可选）
   - {state:} → 插值 state 值
   - {display:} → 递归渲染子 passage 内容
   - {custom: args} → 查 macros 表，调用 MacroHandler::handle()
8. 触发 on_passage_end 钩子
9. 产出 StepResult：
   - StepResult::Render(cmds) — 推送渲染命令给后端
   - StepResult::WaitingForChoice — 暂停等待用户选择
   - StepResult::WaitingForInput { prompt } — 暂停等待用户文本输入
   - StepResult::WaitingForStream { prompt, fallback, cache_key, context } — 暂停，由后端执行 LLM 调用
   - StepResult::Finished — 叙事结束
10. 后端（TUI/Web/GUI）根据 StepResult 决定行为：
    - CLI：`loop { match runtime.step() { ... } }` 轮询；遇到 WaitingForStream 时调用 pathos-llm 执行请求
    - WASM：事件回调调用 `runtime.submit_input()` / `runtime.feed_stream_token()`
11. 用户输入后回到步骤 1
```

### 5.2 状态管理

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// 浮点数。NaN 和 Infinity 为非法值，序列化时拒绝。
    /// 实现阶段使用 ordered-float 包装以确保全序比较和合法序列化。
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

pub enum Scope {
    Global,   // 跨 passage 持久，存档时序列化
    Temp,     // 一次性变量，下次导航时清除
}
```

**设计说明**：
- 只有 `Global`（跨 passage 持久）和 `Temp`（下次导航时清除）两种作用域，语义明确
- 状态支持**路径访问**（`player.inventory.0.name`），通过点号和索引分隔符解析嵌套结构
- `Value::Float` 中的 NaN 和 Infinity 为非法状态值；序列化阶段遇到 NaN 时引擎将拒绝保存并报错

**变更追踪**：每次 `state.set()` 记录一条 `StateChange{path, old, new}` 到内存 delta buffer。每 `checkpoint_interval`（默认 5）个 passage 拍一次完整 MessagePack 快照，清空 delta buffer。

```rust
pub struct StateChange {
    pub path: String,         // e.g. "player.hp"
    pub old: Option<Value>,
    pub new: Value,
}

pub struct StateSnapshot {
    pub full_state: Vec<u8>,   // MessagePack 完整状态
    pub passage: PassageId,
    pub turn: u64,
}
```

**环形历史**：`NarrativeRuntime.history` 是一个上限 `max_checkpoints`（默认 20）的环形缓冲，仅用于游戏内「回退」功能。存档文件只存最新完整快照 + 当前 delta，不存完整历史。

### 5.3 Passage 图

- 节点：`PassageNode`（name, tags, content AST, scripts, hooks）
- 边：`PassageEdge`（from, to, kind: Link | Display | Script）
- 图构建：解析 passage 内容中的 `[[link]]` 和 `{display:}` 指令，构建有向边
- 静态分析：孤立/不可达/死结束检测、循环检测

### 5.4 钩子系统

钩子采用**可序列化的 handler** 替代闭包，支持 .pathos 作者、Mod 作者、Rust 插件三方注册。

```rust
pub enum HookHandler {
    /// 静默执行指定的 passage（不向用户产生渲染输出，
    /// 但 passage 内的 {display:} / game.goto() 调用仍会生效）
    Passage(PassageId),
    /// 执行一段脚本 — 拥有完整的状态读写权限
    Script { lang: ScriptLang, code: String },
}

pub struct HookRegistry {
    handlers: HashMap<HookEvent, Vec<HookHandler>>,
}

pub enum HookEvent {
    StoryInit,
    StorySave,
    StoryLoad,
    PassageStart,
    PassageRender,
    PassageEnd,
    ChoiceSelected,
    Custom(String),
}
```

**注册来源**：
1. `.pathos` 中 `@hook: event_name` + fenced code block → 自动注册为 `HookHandler::Script`
2. Mod 的 `inject_hook` 操作 → 注入 `HookHandler::Script` 或 `HookHandler::Passage`
3. Rust 插件通过 `NarrativeRuntime::register_hook(event, handler)` → 直接注册
4. `.pathos` 作者可手写一个静默 passage，通过 Mod 的 `inject_hook` + `Passage` 变体注册——**完全不依赖脚本引擎**

**执行语义**：
- 钩子 passage/脚本**静默执行**（不产生面向用户的渲染输出，除非显式调用 `{display:}` 或脚本中的 `game.goto()`）
- 同一事件上的多个 handler 按注册顺序依次执行
- 钩子中调用 `game.goto()` 会中断当前 passage 的正常渲染链，接管叙事流
- 钩子脚本具有完整的状态读写权限（`state.get/set/inc/dec` 等），支持跨语言（Rhai 始终可用；JS/Lua 若对应 feature 启用则也支持）
- 钩子脚本执行错误时：引擎记录错误日志，继续执行后续 handler，不中断叙事流

### 5.5 表达式引擎

内建简易表达式求值器：
- 字面量：数字、字符串、布尔、null
- 运算符：`+`, `-`, `*`, `/`, `%`, `&&`, `||`, `!`, `==`, `!=`, `<`, `>`, `<=`, `>=`
- 状态访问：`state.get("path")` — 脚本中使用；`$path` — **仅在 `{if:}` 条件表达式中可用**的语法糖
- 函数调用：`random(min, max)`, `has_tag("tag")`, `visited("passage")`, `count("tag")`

表达式求值器在 `pathos-parser` 中作为子解析器实现，在 `pathos-core` 中执行。保证 `.pathos` 和脚本引擎使用同一套表达式语义。


## 6. 脚本与扩展

### 6.1 三级扩展模型

| 层级 | 面向用户 | 方式 | 示例 |
|---|---|---|---|
| **L0: 叙事作者** | 写手、设计师 | `.pathos` 格式 + 内建宏 | `{if:}` `[[link]]` `{ai:}` `{set:}` |
| **L1: 脚本扩展** | 技术写手、Mod 作者 | 多语言脚本（Rhai/JS/Lua） | 自定义逻辑、复杂数值系统 |
| **L2: 引擎扩展** | Rust 开发者、贡献者 | `MacroHandler` trait 实现 + 渲染后端开发 | 自定义宏、新渲染目标 |

### 6.2 ScriptEngine（多语言对等）

脚本引擎以 **enum** 形式内嵌于 `pathos-core`。Rhai 始终编译（~200KB），JS 和 Lua 通过 feature gate 按需引入。

```rust
pub enum ScriptEngine {
    Rhai(rhai::Engine),
    #[cfg(feature = "script-js")]
    Js(boa_engine::Context),
    #[cfg(feature = "script-lua")]
    Lua(mlua::Lua),
}

pub enum ScriptLang { Rhai, Js, Lua }
```

`.pathos` 中的 fenced code block 以 info string 区分语言：

````
```rhai
state.set("hp", 10);
```

```js
state.set("hp", 10);
```

```lua
state:set("hp", 10)
```
````

`ScriptLang` 由解析器从 info string 提取。不存在「主力」和「可选」的层级关系——三个语言在 enum 和 dispatch 逻辑中完全对等。

### 6.3 Rhai API

Rhai 引擎内置以下 API（面向 `.pathos` 作者和 Mod 作者），其他语言引擎提供等价的语义接口：

- `state.get(key)`, `state.set(key, value)`, `state.inc(key)`, `state.dec(key)`
- `state.has(key)`, `state.delete(key)`
- `game.goto(passage)`, `game.back()`, `game.restart()`
- `game.visited(passage)`, `game.count(tag)`
- `random(min, max)`, `random_float()`
- `ai.prompt(text)` — 触发 LLM 请求（引擎产出 `WaitingForStream`，由后端执行）

### 6.4 语言引擎评估

| 引擎 | 类型 | WASM 目标 | 体积 | Feature gate |
|---|---|---|---|---|
| **rhai** | 纯 Rust DSL | `wasm32-unknown-unknown` ✓ | ~200KB | 始终编译 |
| `boa` (JS) | 纯 Rust ECMAScript | `wasm32-unknown-unknown` ✓ | ~1MB+ | `script-js` |
| `mlua` (Lua) | Lua 绑定 | `wasm32-unknown-emscripten` ✗ | ~500KB | `script-lua`（非 WASM 目标） |

**设计决策**：
- Rhai 是唯一同时满足 WASM 目标兼容、体积可控、沙箱安全的引擎，**始终编译**
- JS 通过 `boa` 可选 feature gate 引入（~1MB 代价），启用方需明确知晓体积影响
- Lua 通过 `mlua` 在非 WASM 目标（CLI / 服务端）下作为可选 feature gate 引入
- 三个引擎通过 `ScriptEngine` enum 对等处理，不存在架构上的主次之分


## 7. LLM 集成 (`pathos-llm`)

### 7.1 设计理念

AI 不是"自动写故事"，而是**叙事增强**。在明确的人类叙事框架内：
- 场景氛围描述（环境、天气、气氛）
- NPC 对话生成（基于角色人设）
- 动态事件（根据 state 变量生成遭遇）

**核心引擎不调用 LLM**。`pathos-core` 遇到 `{ai:}` 指令时，产出 `StepResult::WaitingForStream` 信号。实际的 LLM 网络调用发生在渲染后端（TUI 的事件循环、WASM 的 fetch 回调中），由 `pathos-llm` 库提供具体实现。

### 7.2 LLM 上下文

```rust
/// 引擎传给后端的 LLM 上下文（core 类型，不依赖任何 LLM 库）
pub struct LLMContext {
    pub passage_name: String,
    pub passage_tags: Vec<String>,
    pub story_title: String,
    /// 最近 N 个 passage 的渲染输出原文（按 token 数截断，环形缓冲）
    pub recent_text: Vec<String>,
    /// 标记为 LLM-visible 的状态变量
    pub visible_state: HashMap<String, Value>,
}
```

`WaitingForStream` 信号携带此上下文，后端将其与用户提供的 LLM 配置（provider、model、API key）组合后发起调用。

### 7.3 LLM 配置归属

LLM 配置（API key、provider、model、endpoint）**由用户运行时提供**，不在 `.pathos` 文件中。

| 环境 | 配置来源 |
|---|---|
| CLI | `--provider openai --model gpt-4o --api-key $OPENAI_API_KEY` 或环境变量 |
| Web | 浏览器设置面板，存储在 localStorage |
| GUI | 设置面板 |
| 嵌入 | 宿主应用通过 API 传入 `LLMConfig` struct |

```rust
/// 用户运行时 LLM 配置（pathos-llm 类型，非 core）
pub struct LLMConfig {
    pub provider: LLMProvider,
    pub model: String,
    pub api_key: String,
    pub endpoint: Option<String>,
    pub max_tokens: Option<u32>,
}
```

### 7.4 AI 使用模式

**模式 1：即时生成（同步阻塞）**
```
{ai: prompt | fallback}
```
引擎产出 `WaitingForStream`。后端调用 LLM，阻塞叙事直到文本返回。失败时渲染 fallback。

**模式 2：流式显示（推荐）**
```
{ai-stream: prompt | fallback}
```
引擎产出 `WaitingForStream`。后端调用 LLM 流式接口，逐 token 推送给引擎（`feed_stream_token`），引擎产出 `StreamToken` 渲染命令。

**模式 3：缓存生成（构建时）**
```
{ai-cached: key="cache_key" prompt | fallback}
```
首次生成后缓存结果。`pathos build` 命令预先生成所有 `{ai-cached:}` 块。

### 7.5 Fallback 机制（强制要求）

每一个 `{ai:}` / `{ai-stream:}` / `{ai-cached:}` 指令**必须**提供 fallback 文本。

**语法**：`{ai: prompt | fallback text}`

引擎行为：
1. 引擎产出 `WaitingForStream { prompt, fallback, ... }`
2. 后端尝试调用 LLM
3. 若成功：逐 token feed 回引擎或一次性返回
4. 若失败：后端调用 `runtime.end_stream(Err(...))`，引擎发送 `StreamFailed { fallback }` 渲染命令
5. 若指令中未提供 fallback：解析器在 P3 阶段报错

### 7.6 `pathos-llm` 库

`pathos-llm` 是纯后端调用库，不依赖 `pathos-core`。提供：

| 后端 | Feature gate |
|---|---|
| OpenAI | `llm-openai` |
| Anthropic | `llm-anthropic` |
| Ollama（本地） | `llm-ollama` |
| Mock（预置文本回放） | 默认启用 |

各渲染后端（`pathos-tui`、`pathos-web`）直接依赖 `pathos-llm`，在收到 `WaitingForStream` 时调用相应 provider。

### 7.7 Prompt 策略

系统 prompt 由用户通过 `LLMConfig.system_prompt` 配置。引擎自动注入到 `LLMContext` 的信息：
- 故事标题和当前 passage 名称/标签
- 最近 passage 的渲染输出原文（按 token 数截断，环形缓冲），不进行 AI 摘要（避免递归依赖）
- 标记为 LLM-visible 的状态变量


## 8. Mod 系统 (`pathos-mod`)

### 8.1 Mod 清单格式

```toml
# mod.toml
[package]
name = "dark-forest-expansion"
version = "1.2.0"
pathos = "1.0"
priority = 0

[meta]
title = "暗森林扩展"
author = "ModAuthor"
description = "在原版森林场景中增加隐藏路线和 NPC 对话"

[dependencies]
base-story = ">=1.0.0"

[ordering]
after = ["some-other-mod"]

[patches]
"intro" = { action = "append", passage = "intro_patch" }
"mushroom_path" = { action = "replace" }

[overrides]
"deep_forest" = "deep_forest_alt"

[macros]
greet = { script = "macros/greet.rhai" }
roll_dice = { script = "macros/roll_dice.rhai" }
```

### 8.2 Mod 操作语义

| 操作 | 说明 |
|---|---|
| `override` | 完全替换目标 passage |
| `prepend` | 在目标内容前插入 |
| `append` | 在目标内容后追加 |
| `replace` | 保留目标名和标签，替换全部内容 |
| `inject_hook` | 向目标钩子注入回调（Script 或 Passage） |
| `add_script` | 为指定 passage 追加脚本块 |
| `add_tag` | 为目标 passage 添加标签 |

**`[macros]` 段**：Mod 可注册自定义宏。每个条目指向一段 Rhai 脚本，引擎启动时编译并构造 `RhaiMacroHandler`，注入 `NarrativeRuntime.macros` 列表。在 `.pathos` 中通过 `{greet: args}` / `{roll_dice: args}` 调用。

### 8.3 加载流程

1. 发现：扫描 `mods/` 目录，查找包含 `mod.toml` 的子目录或 `.pathos-mod` 归档
2. 解析：解析所有 `mod.toml`，构建依赖图
3. 排序：拓扑排序，检测循环依赖和冲突
4. 应用：按顺序应用覆盖、补丁、宏注册
5. 验证：运行静态分析，确保所有链接目标和宏引用存在

### 8.4 冲突管理

当两个 mod 修改同一 passage 时：

1. 显式 `[conflicts]` 声明 → 阻止加载，报错退出
2. `[ordering] after` 显式顺序 → 被依赖者先加载，后来者覆盖
3. `priority` 值 → 高者后加载（胜出），默认为 0
4. 以上均无 → 按 mod 名字典序排序（Z 最后加载），同时发出 warning


## 9. 渲染抽象 (`pathos-render`)

### 9.1 RenderBackend trait（纯 push 模型）

引擎不阻塞。`StepResult` 信号驱动后端行为。`RenderBackend` 是 `pathos-core` 与外部之间唯一的跨 crate trait。

```rust
pub struct Choice {
    pub label: String,
    pub target: PassageId,
    pub enabled: bool,
    pub tooltip: Option<String>,
}

pub enum RenderCommand {
    Clear,
    Text(String),
    StreamBegin,
    StreamToken(String),
    StreamEnd,
    /// 流式 LLM 失败。携带 fallback 文本，
    /// 后端直接渲染此 fallback 即可。
    StreamFailed { fallback: String },
    Choice(Vec<Choice>),
    Input { prompt: String, default: Option<String> },
    Separator,
}

pub trait RenderBackend {
    fn render(&mut self, commands: Vec<RenderCommand>);
    fn clear(&mut self);
}

pub enum StepResult {
    Render(Vec<RenderCommand>),
    WaitingForChoice,
    WaitingForInput { prompt: String, default: Option<String> },
    /// LLM 请求——由后端执行实际调用。携带所有必要信息，
    /// 后端组合用户提供的 LLMConfig 后发起请求。
    WaitingForStream {
        prompt: String,
        fallback: String,
        cache_key: Option<String>,
        context: LLMContext,
    },
    Finished,
}

impl NarrativeRuntime {
    pub fn step(&mut self) -> StepResult;
    pub fn submit_input(&mut self, input: UserInput);
    pub fn feed_stream_token(&mut self, token: String);
    pub fn end_stream(&mut self, result: Result<(), String>);
}
```

**StreamFailed 语义**：`StreamFailed { fallback }` 携带 fallback 文本，后端直接使用，不再依赖隐式的「先发 StreamFailed 再发 Text(fallback)」约定。

**WaitingForStream**：引擎不执行任何网络调用。后端收到此信号后，使用 `LLMContext` + 用户 `LLMConfig` 调用 `pathos-llm` 的相应 provider，将结果通过 `feed_stream_token` / `end_stream` 传回引擎。

### 9.2 各后端实现

| 后端 | crate | LLM 调用方式 | 特点 |
|---|---|---|---|
| **TUI** | `pathos-tui` | 终端事件循环内 `tokio::spawn` | Ratatui，彩色、分屏、历史回溯 |
| **Web** | `pathos-web` | WASM fetch 或转发服务端 | Leptos，打字机效果、保存/读取 |
| **GUI** | `pathos-gui` | 原生 HTTP 客户端 | egui 即时模式桌面客户端 |

### 9.3 后端职责边界

渲染后端**不持有**叙事状态。它只接收 `RenderCommand` 流并显示它们，执行 LLM 调用并将结果传回引擎。状态管理完全在引擎侧。


## 10. 技术选型总览

### 10.1 解析器

| 库 | 优点 | 缺点 |
|---|---|---|
| **`winnow`** | nom 继任者，零拷贝，流式，更好的错误信息 | 社区规模尚小 |
| `nom` | 久经考验，文档丰富 | 错误信息差 |
| `chumsky` | 错误恢复极佳，组合子优雅 | 学习曲线陡 |
| `pest` | PEG 语法文件，可读性好 | 运行时慢 |

**结论**：`winnow` + `miette`。

### 10.2 WASM UI 框架

| 框架 | WASM 大小 | 反应式模型 |
|---|---|---|
| **`Leptos`** | ~30KB | 细粒度信号 |
| `Dioxus` | ~50KB | 虚拟 DOM |
| `Yew` | ~40KB | Elm 架构 |

**结论**：`Leptos`。

### 10.3 TUI

**结论**：`ratatui`。

### 10.4 脚本引擎

见 §6.4 完整评估表。

### 10.5 序列化 / 存储

| 格式 | 用途 |
|---|---|
| `serde-saphyr` | `.pathos` 元数据解析 |
| `serde_json` | Debug 输出、Web API 通信 |
| `rmp-serde` (MessagePack) | 存档文件 |
| `toml` | `mod.toml` 配置解析 |

### 10.6 错误与日志

| 库 | 用途 |
|---|---|
| `thiserror` | 库级错误类型派生 |
| `miette` | 解析器诊断（带源文本标注） |
| `tracing` | 结构化日志 |

### 10.7 异步运行时

- `pathos-core`：**零异步依赖**
- `pathos-llm`：`tokio`
- `pathos-web`：`wasm-bindgen-futures`

### 10.8 其他工具依赖

| 用途 | 库 |
|---|---|
| CLI 参数解析 | `clap` 4.x |
| HTTP 客户端 | `reqwest` |


## 11. 存档与版本兼容

### 11.1 存档文件格式

```rust
pub struct SaveFile {
    pub format_version: String,
    pub engine_version: String,
    pub game_id: String,
    pub current_passage: PassageId,
    pub state: StoryState,
    pub passage_history: Vec<PassageId>,
    pub turn: u64,
    pub saved_at: String,
}
```

存档为**完整快照**（非增量），序列化用 MessagePack。内存中的 delta 仅用于运行时 history 环形缓冲。

### 11.2 版本兼容契约

| 版本变化 | 行为 |
|---|---|
| Patch（1.0.0 → 1.0.1） | 完全兼容，直接加载 |
| Minor（1.0.0 → 1.1.0） | 提供迁移函数，加载成功但发出 warning |
| Major（1.x → 2.0） | 拒绝加载，给出明确的版本不匹配错误日志 |


## 12. WASM 二进制体积目标

### 12.1 硬性目标

`pathos-web` WASM 产物（gzip 后）≤ 500KB。

### 12.2 达成策略

| 策略 | 说明 |
|---|---|
| Rhai `minimal` feature | 关闭 `sync`、`no_function` 等非必要 features |
| JS/Lua feature gate 默认关闭 | WASM 构建仅含 Rhai |
| winnow verbose error 关闭 | WASM 构建中禁用详细错误信息 |
| `pathos-llm` 不编译进 WASM | WASM 端通过 fetch 转发 LLM 请求到服务端 |
| LTO + `opt-level = "s"` | Cargo profile 配置 |
| `wasm-opt` | 后处理优化 |
| `twiggy` / `cargo-bloat` | Phase 1 起持续监控 |


## 13. 测试策略

| 层级 | 范围 | 目标 | 工具 |
|---|---|---|---|
| **单元测试** | `pathos-core` 每个公开 API | ≥80% 行覆盖率 | `cargo test` + `cargo-tarpaulin` |
| **快照测试** | `pathos-parser` 每个语法特性 | ≥90% 覆盖率 | `insta` |
| **属性测试** | 表达式求值器、状态机 | 1000+ 随机输入 | `proptest` |
| **集成测试** | 完整 .pathos → 执行 → 验证输出 | 覆盖所有宏、mod 冲突、LLM fallback | `cargo test --test` |
| **E2E** | `pathos run` 跑完整剧本 | 验收门禁 | `trycmd` / `assert_cmd` |
| **WASM smoke** | 浏览器渲染正确性 | 基本 UI 回归 | Playwright + `wasm-pack test` |

Phase 1 必须包含 parser 快照测试、core 单元测试，以及 WASM 体积基线测量。


## 14. Web 客户端无障碍（a11y）

`pathos-web` 以 WCAG AA 为基线：

| 要求 | 实现 |
|---|---|
| 语义 HTML | `<article>` 包裹 passage，`<nav>` 包裹选项列表，`<button>` 作为可点击链接 |
| ARIA 动态通知 | `aria-live="polite"` 标注内容区；passage 切换时 `aria-atomic="true"` |
| 键盘导航 | Tab 在选项间移动，Enter/Space 确认；`focus-visible` 样式 |
| 焦点管理 | passage 切换后自动聚焦到内容区或第一个选项 |
| 流式文本节流 | `StreamToken` 渲染用 `aria-live="polite"` + 500ms debounce |
| 色彩对比度 | 满足 WCAG AA（文本 4.5:1，大文本 3:1） |


## 15. 实施路线图

详见 [TODO.md](./TODO.md)。Phase 1–4 分阶段推进，从核心引擎 MVP 到完整生态打磨。


## A. 附录：设计决策记录

### A.1 为什么引擎是同步的？

叙事流天然是线性的：显示文本 → 等待用户选择 → 跳转。LLM 调用是唯一的异步场景，但通过「引擎产出 WaitingForStream → 后端异步调用 LLM → 结果 feed 回引擎」模式在渲染层处理，不污染核心引擎的同步假设。

### A.2 为什么选 .pathos 而不是纯 TOML/YAML？

纯结构化格式会把叙事文本变成冗长的字符串字面量，丧失可读性。.pathos 的设计是"80% Markdown + 20% 结构化扩展"。

### A.3 为什么脚本引擎以 enum 而非 trait 实现？

Rhai 是纯 Rust、零 I/O 的引擎，与 core 的约束完全匹配。将其抽象为 trait 会引入不必要的间接层，且不利于多语言对等处理。三个引擎（Rhai/JS/Lua）通过 enum variant 扁平表示，dispatch 靠 `lang` tag，扩展新语言只需增加 variant。

### A.4 为什么 LLM 调用不在 core 中执行？

核心引擎应被同步 CLI、异步 Web、嵌入式环境等多种上下文使用。LLM 网络调用是唯一需要异步的场景，将其推到渲染后端处理，使 core 保持零异步依赖。

### A.5 为什么 RenderBackend 采用纯 push 模型？

原有阻塞式 `wait_choice()` / `wait_input()` 在 WASM 浏览器环境中不可用。改为 `StepResult` 纯 push 模型后，CLI 用 loop 轮询，WASM 用事件回调驱动，共用同一套引擎 API。

### A.6 为什么 LLM 上下文不做自动摘要？

用 LLM 给 LLM 做摘要会形成递归依赖。直接携带最近 passage 的渲染输出原文，按 token 数截断。

### A.7 为什么 StreamFailed 携带 fallback？

原先隐式的「先发 StreamFailed 再发 Text(fallback)」约定依赖后端实现者理解。改为 `StreamFailed { fallback }` 显式携带，减少误解。

### A.8 为什么移除 passage_scoped 变量？

`passage_locals` 的生命周期语义模糊（离开时是清理还是保留？）。只保留 `Global`（持久）和 `Temp`（下次导航时清除），语义清晰。

### A.9 为什么 `$path` 只在 `{if:}` 表达式中可用？

`$path` 是 `{if:}` 条件中的语法糖。脚本和 `{state:}` 插值中统一用 `state.get("path")` API，避免同一概念有多种写法。

### A.10 安全模型

Pathos 引擎需运行用户（包括不可信 Mod 作者）提供的内容。安全边界如下：

| 边界 | 策略 |
|---|---|
| **脚本沙箱** | Rhai 引擎以 `Engine::new_raw()` 构建，不注册任何 I/O 或网络函数。脚本施加 `max_operations`（默认 100,000）防止无限循环 |
| **文件系统** | `.pathos` 中 `{display: "passage"}` 只能引用已知 passage 名称，不允许路径遍历。Mod 系统不暴露宿主文件系统 |
| **Mod 信任边界** | Mod 通过 `inject_hook` 注入的脚本与 `.pathos` 作者脚本共享同一沙箱约束。Mod 不可访问宿主文件系统或网络 |
| **LLM prompt injection** | `{ai: prompt}` 中的 prompt 若拼接了 state 变量值，引擎在注入 `LLMContext.visible_state` 前对值进行 sanitize（截断、移除控制字符） |
| **存档安全** | 存档仅包含 `StoryState` 的 MessagePack 序列化数据。加载时校验 `format_version`，不执行反序列化期间触发的代码 |

### A.11 `metadata` 字段语义

`StoryState.metadata` 从 `.pathos` frontmatter 中提取故事级元数据（`title`、`author`、`version`、`save_slots` 等），由引擎在初始化时写入。此字段**脚本可读但不可写**——脚本引擎的 `state.set()` 若 target 为 metadata key 则返回错误。`StoryConfig` 是初始化阶段的配置快照，运行时不会变化；`metadata` 是其在状态系统中的只读映射。

### A.12 解析器错误恢复策略

每个解析 pass 产出 `(PartialAst, Vec<Diagnostic>)` 而非 `Result<Ast, Error>`：

- 遇到语法错误时**不 bail-out**，而是记录诊断、插入 error-recovery 哨兵节点（如 `ContentNode::Text("[parse error]")`），继续解析剩余内容
- 这使解析器可在 Phase 4 的 LSP 中提供完整的诊断列表和部分 AST，而非仅报告第一个错误
- Error recovery 的具体哨兵策略在 parser 实现阶段细化


---

> **本文档持续更新。** 技术选型和设计决策会随实施过程中的发现而调整。任何涉及架构层面的修改都应更新本文档并在 PR 中注明决策理由。
