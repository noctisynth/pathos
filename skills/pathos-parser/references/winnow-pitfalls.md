## Winnow pitfalls encountered during migration

### 1. `take_till(0.., ...)` infinite loop

`0..` permits zero-length matches. When text is the last `alt` fallback and the
first char is a special char (`{`, `[`, etc.), `take_till` returns an empty
slice immediately. The while loop sees non-empty input but makes no progress
→ infinite loop.

**Fix**: Always use `take_till(1.., ...)` for text fallback. Add error recovery
that skips one char if all alt alternatives fail.

### 2. `ContentNode::Text(t)` requires `String`, not `&str`

`ContentNode::Text(String)` stores an owned string. `map(|t: &str| ...)` must
call `.to_string()` on the text slice.

### 3. `then_text` truncation at `{else}` boundary

In `scan_conditional_body`, when `{else}` is found before `{end}`, the
`then_text` must be `original[..else_marker]`, NOT `original[..pos]` (where
`pos` is at `{end}`). Otherwise the else block and `{else}` marker leak into
the then branch.

### 4. `alt` with closure-based parsers

Closures returning `impl Parser` cannot be mixed with function pointers in
`alt` tuples due to type inference. Use `impl Parser`-returning functions
instead of inline closures within `alt`.

### 5. `ModalResult` vs `Result`

`winnow::ModalResult<O, E>` = `Result<O, ErrMode<E>>` — used internally by
combinators for backtracking decisions. User-facing parsers should return
`winnow::Result<O, E>` = `Result<O, E>` (no `ErrMode` wrapper). The `?`
operator works directly with `winnow::Result`.

### 6. Parser functions as `Parser` trait impl

Functions `fn(&mut I) -> Result<O, E>` automatically implement `Parser<I, O, E>`
via a blanket impl. However, method calls like `.map()` on function items
require the compiler to resolve the trait impl, which can fail due to HRTB
lifetime issues. If `.map()` fails, use explicit closure wrapper:
`|i: &mut &str| parse_link.parse_next(i).map(InlineItem::Node)`.

### 7. `add_context` signature

`ContextError::new().add_context(ctx, input, token)` takes 3 args, not 1.
For simple error messages, use `ContextError::new()` without context.
