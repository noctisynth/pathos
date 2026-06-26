# 事故报告：解析器无限循环与标题丢失

**日期**：2026-06-26
**严重程度**：严重 — 测试挂死（无限循环）与静默数据损坏（passage `id` 丢失）
**受影响模块**：`pathos-parser`

## 概述

`pathos-parser` 在 Phase 1 开发期间发现两个独立 bug。
Bug 1 导致 passage 标题被静默吞入正文文本。
Bug 2 导致任何行内指令（`{state:}`、`{set:}`、`{display:}`、`{ai*:}`）触发无限解析循环。
两者叠加导致 `.pathos` 格式的所有测试无法通过，且 `cargo test` 无限挂死。

## Bug 1：前导空行使标题被吞

### 位置

`crates/pathos-parser/src/pathos_parser.rs` — `split_into_passage_blocks()`

### 根因

`split_source()` 用 `\n` 连接 frontmatter 闭合 `---` 之后的剩余行，产生如下文本：

```
\n# intro\n...content...
```

`split_into_passage_blocks()` 把前导空行当作了 passage 块的一部分而非跳过，
导致块内容为 `"\n# intro\n..."`。

在 `parse_passage_block()` 中，`lines.next()` 拿到了空行 `""` 当作"标题"。
由于 `"".strip_prefix("# ")` 返回 `None`，`id` 被留空，真正的标题
`# intro` 被归入正文文本。

### 影响面

- 所有 passage 的 `id` 字段均为空字符串
- 标题原文出现在 passage body 中
- `rebuild_edges()` 产出的 edge 的 `from` 字段为空
- 5 个 `.pathos` 格式测试中 3 个失败

### 修复

为 `split_into_passage_blocks()` 增加 `seen_content` 标志。第一个标题行或正文行之前
的前导空行不再追加到 `current`。

## Bug 2：行内解析器位置重置（无限循环）

### 位置

`crates/pathos-parser/src/inline.rs` — `parse_state_directive()`、`parse_set_directive()`、
`parse_display_directive()`、`parse_ai_directive()`

### 根因

`parse_inline()` 主循环依赖 `pos` 追踪解析进度：

```rust
if let Some((node, new_pos)) = parse_directive(&chars, pos, &mut diagnostics) {
    nodes.push(node);
    pos = new_pos;
    continue;
}
```

`parse_directive()` 内部正确计算了局部 `pos`（指向闭合 `}` 之后），
但四个 helper 函数 `parse_state_directive`、`parse_set_directive`、
`parse_display_directive`、`parse_ai_directive` 均返回
`Some((ContentNode, 0))`——把位置硬编码为 `0`。

match 分支直接透传：

```rust
// 修复前（有 bug）：
"state" => parse_state_directive(&args_str),    // 返回 (node, 0)
"set"   => parse_set_directive(&args_str),      // 返回 (node, 0)
"ai"    => parse_ai_directive(&name, &args_str), // 返回 (node, 0)

// 修复后：
"state" => Some((parse_state_directive(&args_str), pos)),  // 使用正确的 pos
"set"   => Some((parse_set_directive(&args_str), pos)),
"ai"    => Some((parse_ai_directive(&name, &args_str), pos)),
```

解析 `{state: "player.hp"}` 后，`pos` 被设为 `0`。主循环从源头重新开始，
重复解析同一指令，形成无限循环。

### 为何之前没有被发现

行内单元测试在首次 `cargo test` 运行时全部"通过"。但实际上测试从未完成——
它在无限循环中挂死了。所谓的"通过"来自行内解析器接入 `parse_passage_block` 之前的
早期快照。当行内解析器后续通过 `parse_inline` 被 `parse_passage_block` 调用后，
无限循环导致整个测试套件无法完成。

### 影响面

- 任何包含 `{state:}`、`{set:}`、`{display:}` 或 `{ai*:}` 的 passage body 都会触发无限循环
- `cargo test` 挂死，只能强杀
- 多次测试运行导致系统不稳定（用户反馈系统崩溃）

### 修复

- 将四个 helper 函数改为直接返回 `ContentNode`（而非 `Option<(ContentNode, usize)>`）
- 每个 helper 调用用 `Some((helper(&args), pos))` 包裹，使用正确计算的 pos
- 更新 `parse_mixed_content` 测试断言 4 个节点 → 5 个节点（尾部文本 " HP remaining."
  现在被正确解析，不再被无限循环吞掉）

## 修复后测试结果

```
running 20 tests
test inline::tests::parse_* ... ok × 10
test pathos_parser::tests::parse_* ... ok × 5
test toml_parser::tests::* ... ok × 5

test result: ok. 20 passed; 0 failed; 0 ignored
```

## 教训

1. **携带位置信息的返回值必须端到端验证。** 当函数自行计算 pos
   并将节点构造委托给 helper 时，helper 不能丢弃 pos。

2. **格式边界处的空行处理是常见 bug 来源。** frontmatter 闭合标记（`---`）与
   第一个 passage 标题之间的分隔会产生隐式空行，必须在块分割器中处理。

3. **`parse_passage_block` 需要防御性编程。** 尽管 `split_into_passage_blocks`
   现已剥离前导空行，但 `parse_passage_block` 仍需在标题解析前跳过 leading whitespace，
   因为它可能被其他代码路径调用（如 TOML 解析器）。

4. **测试超时是信号，不应忽视。** `cargo test` 挂死应当尽早排查，而非假定构建慢。

## 后续行动

- [x] 修复 `split_into_passage_blocks` 前导空行处理
- [x] 修复行内解析器 pos 传递
- [x] 更新 `parse_mixed_content` 测试预期
- [x] 为无限循环场景增加回归测试（解析指令后验证 `pos` 正确前进）
- [x] 为 frontmatter 后跟空行的标题解析增加回归测试
 - [x] 为所有解析测试集成超时守卫（`with_timeout`，默认 10 秒上限）
 - [x] 测试套件运行时采用 `--test-threads=1` 避免竞争导致的内存压力
 - [x] 集成测试使用常规字符串（非 raw string），避免 `\u{2192}` 在 raw string 中不被解释的问题
