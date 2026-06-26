---
name: pathos-parser
description: >
  Use when working on the Pathos parser (crates/pathos-parser), especially
  the inline parser (P3) built on winnow combinators. Covers architecture,
  common patterns, conditional block parsing, error recovery, and pitfalls.
---

# Pathos Parser Skill

## Architecture

The parser lives in `crates/pathos-parser/src/`. Key files:

| File | Role |
|---|---|
| `inline.rs` | P3 inline parser — winnow combinators for `[[link]]`, `{directive}`, conditionals |
| `expression.rs` | Recursive-descent expression parser for `{if: expr}` |
| `pathos_parser.rs` | P1–P4 pipeline: split, block parse, inline parse, semantic analysis |
| `format.rs` | `FormatParser` trait, `Diagnostic`, `FormatRegistry` |

## Winnow Patterns

### Type aliases

```rust
type PResult<'a, O> = winnow::Result<O, ContextError>;
// = Result<O, ContextError>, NOT ModalResult (no ErrMode)
```

### Function-based parsers

Standalone parsers take `&mut &str` and return `PResult<O>`:

```rust
fn parse_link<'a>(input: &mut &'a str) -> PResult<'a, ContentNode> {
    let _ = "[[".parse_next(input)?;
    // ...
    Ok(ContentNode::Link { ... })
}
```

### alt with unified output

When alternatives return different types, use an enum:

```rust
enum InlineItem { Node(ContentNode), Skip }

alt((
    parse_link.map(InlineItem::Node),
    parse_comment.map(|_| InlineItem::Skip),
    // ...
)).parse_next(input)
```

### Closure-based parsers for state

When a parser needs mutable state (e.g., diagnostics), return `impl Parser`:

```rust
fn parse_directive_pure<'a>(
) -> impl Parser<&'a str, ContentNode, ContextError> + 'a {
    move |input: &mut &'a str| {
        // parser logic here
    }
}
```

### Text fallback pattern

`parse_text` MUST use `take_till(1.., ...)` (not `0..`) to prevent infinite loops:

```rust
fn parse_text(input: &mut &str) -> PResult<&str> {
    take_till(1.., |c: char| c == '[' || c == '{' || c == '<' || c == '/')
        .parse_next(input)
}
```

`1..` ensures at least 1 char consumed; `0..` would match empty string and loop forever.

## Conditional Block Parsing

`{if: expr} ... {else} ... {end}` is handled by two components:

1. **`parse_if_block`**: Parses the condition expression, then delegates to `scan_conditional_body`
2. **`scan_conditional_body`**: Imperative scan using `&[u8]` (bytes) for zero-copy. Tracks `cond_depth` for nested `{if:}` blocks. Returns `(then_text, else_text)` slices.

Winnow cannot express depth-tracked block scanning natively. The imperative approach is correct and documented as a pragmatic choice.

## Expression Parser

`expression.rs` — recursive-descent parser with explicit precedence ladder:
`|| < && < ==/!= < </>/<=/>= < +/- < *// < ! < primary`

Uses `tokenize()` → `Parser` struct pattern (own tokens, not winnow). Works correctly; could optionally migrate tokenizer to winnow `take_while` etc.

## Diagnostic Handling

Orphan `{else}`/`{end}` detection uses a post-processing step:

1. `parse_directive_pure` matches `{else}`/`{end}` and returns `ContentNode::Text("else")`/`ContentNode::Text("end")`
2. `add_standalone_block_diagnostics` scans the node list and emits diagnostics

This avoids threading `&mut Vec<Diagnostic>` through the winnow combinator chain.

## Key Combinators Used

| Combinator | Usage |
|---|---|
| `alt((a, b, c))` | Try alternatives in order, backtrack on failure |
| `preceded(tag, parser)` | Match tag then parser, discard tag |
| `terminated(parser, tag)` | Match parser then tag, discard tag |
| `delimited(open, content, close)` | Match surrounded content |
| `take_while(range, predicate)` | Take while predicate is TRUE |
| `take_till(range, predicate)` | Take while predicate is FALSE |
| `take_until(range, tag)` | Take until literal is found |
| `opt(parser)` | Optional parser (succeeds with None on failure) |
| `parser.map(f)` | Transform output |
| `parser.void()` | Discard output, return `()` |
| `parser.parse_next(input)` | Execute parser, returns `Result<O, E>` |
| `parser.parse(input)` | Execute parser top-level, converts ModalResult |

## Testing

```bash
# All parser tests
timeout 60 cargo test -p pathos-parser

# Single test with output
cargo test -p pathos-parser test_name -- --nocapture

# Full suite (always with timeout)
timeout 120 cargo test --all -- --test-threads=1
```

Tests use `timeout` wrapper to prevent infinite-loop hangs (historical incident).
