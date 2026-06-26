//! Inline parser (P3) — parses inline directives within passage body text.
//!
//! Built on `winnow` combinator library per architecture §3.2 / §10.1.
//! Handles: `[[link]]`, `{state:}`, `{set:}`, `{ai:}`, `{display:}`, `{if:}`,
//! `{if:}...{else}...{end}`, and generic `{name: args}` macro calls.

use pathos_core::{AIMode, ContentNode, Expression, MacroArg};
use crate::format::{Diagnostic, Severity};

use winnow::{
    combinator::{alt, opt, preceded, terminated},
    error::ContextError,
    token::{take_till, take_until, take_while},
    Parser,
};

type PResult<'a, O> = winnow::Result<O, ContextError>;

// ── public entry point ───────────────────────────────────────────────────

/// Parse a passage body string into `(nodes, diagnostics)`.
pub fn parse_inline(source: &str) -> (Vec<ContentNode>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let nodes = match parse_inline_nodes(&mut diagnostics).parse(source) {
        Ok(n) => n,
        Err(_) => Vec::new(),
    };


    (nodes, diagnostics)
}

// ── shared types ─────────────────────────────────────────────────────────

/// Unifies output types for `alt` combinator — all parsers produce this.
#[derive(Debug)]
enum InlineItem {
    Node(ContentNode),
    Skip,
}

// ── top-level: repeated node parsing ─────────────────────────────────────

/// Parse a sequence of inline nodes. Uses `alt` for backtracking; `parse_text`
/// is the final fallback, so this always succeeds.
fn parse_inline_nodes<'a>(
    diagnostics: &'a mut Vec<Diagnostic>,
) -> impl Parser<&'a str, Vec<ContentNode>, ContextError> + 'a {
    move |input: &mut &'a str| {
        let mut nodes = Vec::new();
        while !input.is_empty() {
            // alt tries each parser once per token; text fallback ensures success
            let item = alt((
                parse_link.map(InlineItem::Node),
                parse_comment.map(|_| InlineItem::Skip),
                parse_directive_pure.map(InlineItem::Node),
                parse_text.map(|t: &str| InlineItem::Node(ContentNode::Text(t.to_string()))),
            ))
            .parse_next(input);

            match item {
                Ok(InlineItem::Node(n)) => {
                    if let ContentNode::Text(ref s) = n {
                        if s.is_empty() {
                            // Safety valve: skip current char
                            if let Some(c) = input.chars().next() {
                                *input = &input[c.len_utf8()..];
                            }
                        }
                    }
                    nodes.push(n);
                }
                Ok(InlineItem::Skip) => {}
                Err(_) => {
                    // Error recovery: skip one char
                    if let Some(c) = input.chars().next() {
                        *input = &input[c.len_utf8()..];
                    }
                }
            }
        }
        // Post-process: scan for orphan {else}/{end} to emit diagnostics
        // (these appear as Text nodes from parse_text fallback)
        add_standalone_block_diagnostics(&nodes, diagnostics);
        Ok(nodes)
    }
}

/// Detect orphan {{else}}/{{end}} markers that were parsed as text
/// (because parse_directive didn't match them) and emit diagnostics.
fn add_standalone_block_diagnostics(
    nodes: &[ContentNode],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in nodes {
        match node {
            ContentNode::Text(s) if s == "else" => {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    message: "unexpected {else} outside {if:} block".into(),
                    span: None,
                });
            }
            ContentNode::Text(s) if s == "end" => {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    message: "unexpected {end} outside {if:} block".into(),
                    span: None,
                });
            }
            _ => {}
        }
    }
}

// ── text ─────────────────────────────────────────────────────────────────

/// Consume text until the next special character (`[`, `{`, `<`, `/`).
fn parse_text<'a>(input: &mut &'a str) -> PResult<'a, &'a str> {
    take_till(1.., |c: char| c == '[' || c == '{' || c == '<' || c == '/')
        .parse_next(input)
}

// ── comment ──────────────────────────────────────────────────────────────

/// Skip a line comment (`// ...`) or block comment (`<!-- -->`).
fn parse_comment<'a>(input: &mut &'a str) -> PResult<'a, ()> {
    alt((
        preceded("//", take_till(0.., |c: char| c == '\n')).void(),
        preceded("<!--", terminated(take_until(0.., "-->"), "-->")).void(),
    ))
    .parse_next(input)
}

// ── link ─────────────────────────────────────────────────────────────────

/// Parse `[[label -> target]]` or `[[label → target]]`.
fn parse_link<'a>(input: &mut &'a str) -> PResult<'a, ContentNode> {
    let _ = "[[".parse_next(input)?;

    // label: everything up to arrow or `]]`
    let label = take_till(1.., |c: char| c == '-' || c == '\u{2192}' || c == ']')
        .map(|s: &str| s.trim().to_string())
        .parse_next(input)?;
    if label.is_empty() {
        return Err(ContextError::new());
    }

    // Arrow: `->` or `→`
    let _ = alt(("->", "\u{2192}")).parse_next(input)?;

    // target: everything up to `]]`
    let target = take_till(1.., |c: char| c == ']')
        .map(|s: &str| s.trim().to_string())
        .parse_next(input)?;
    if target.is_empty() {
        return Err(ContextError::new());
    }

    let _ = "]]".parse_next(input)?;

    Ok(ContentNode::Link {
        label,
        target,
        enabled_if: None,
    })
}

// ── directive dispatch — closure-based to capture diagnostics ──────────────

/// Parse a `{name: args}` or `{name}` directive. Returns backtrack error
/// if the input doesn't start with `{`. For `{else}`/`{end}`, fails with backtrack
/// so `alt` can try text fallback; diagnostics are handled by post-processing.
fn parse_directive_pure<'a>(input: &mut &'a str) -> PResult<'a, ContentNode> {
    let _ = '{'.parse_next(input)?;

    let name = take_while(1.., |c: char| c.is_alphanumeric() || c == '_' || c == '-')
        .map(|s: &str| s.to_string())
        .parse_next(input)?;

    let _ = opt(':').parse_next(input)?;

    match name.as_str() {
        "if" => parse_if_block.parse_next(input),
        "else" | "end" => {
            // Consume these as text — post-process will emit diagnostics
            Ok(ContentNode::Text(name))
        }
        _ => {
            let args_str = if input.starts_with('}') {
                String::new()
            } else {
                let s: &str = take_till(0.., |c: char| c == '}').parse_next(input)?;
                s.to_string()
            };
            let _ = '}'.parse_next(input)?;
            Ok(dispatch_directive(&name, &args_str))
        }
    }
}

/// Dispatch a non-if directive by name.
fn dispatch_directive(name: &str, args_str: &str) -> ContentNode {
    match name {
        "state" => parse_state_directive(args_str),
        "set" => parse_set_directive(args_str),
        "display" => parse_display_directive(args_str),
        "ai" | "ai-stream" | "ai-cached" => parse_ai_directive(name, args_str),
        _ => {
            let args = parse_macro_args(args_str);
            ContentNode::Macro {
                name: name.to_string(),
                args,
            }
        }
    }
}

// ── conditional block {if: expr} ... {else} ... {end} ───────────────────

fn parse_if_block<'a>(input: &mut &'a str) -> PResult<'a, ContentNode> {
    let condition_str = if input.starts_with('}') {
        String::new()
    } else {
        let s: &str = take_till(0.., |c: char| c == '}').parse_next(input)?;
        s.to_string()
    };
    let _ = '}'.parse_next(input)?;

    let condition = match crate::expression::parse_expression(condition_str.trim()) {
        Ok(expr) => expr,
        Err(_e) => Expression::Literal(pathos_core::value::Value::Bool(false)),
    };

    let (then_text, else_text) = scan_conditional_body(input)?;

    let (then_nodes, _) = parse_inline(then_text);
    let else_nodes = else_text
        .map(|t| parse_inline(t).0)
        .unwrap_or_default();

    Ok(ContentNode::Conditional {
        condition,
        then_branch: then_nodes,
        else_branch: else_nodes,
    })
}

/// Scan forward to find matching `{else}` / `{end}` at the current
/// conditional nesting level. Returns `(then_text, else_text)`.
fn scan_conditional_body<'a>(
    input: &mut &'a str,
) -> PResult<'a, (&'a str, Option<&'a str>)> {
    let original = *input;
    let mut cond_depth: i32 = 0;
    let mut pos: usize = 0;
    let mut else_after: Option<usize> = None;
    let mut else_marker: Option<usize> = None;

    let bytes = original.as_bytes();
    while pos < bytes.len() {
        if bytes[pos] == b'{' {
            let k_start = pos + 1;
            let mut k_end = k_start;
            while k_end < bytes.len()
                && (bytes[k_end].is_ascii_alphanumeric()
                    || bytes[k_end] == b'_'
                    || bytes[k_end] == b'-')
            {
                k_end += 1;
            }
            let keyword = std::str::from_utf8(&bytes[k_start..k_end]).unwrap_or("");

            // Find closing `}`, handling nested braces
            let mut j = k_end;
            let mut inner_brace = 0;
            while j < bytes.len() {
                if bytes[j] == b'{' {
                    inner_brace += 1;
                }
                if bytes[j] == b'}' {
                    if inner_brace == 0 {
                        break;
                    }
                    inner_brace -= 1;
                }
                j += 1;
            }
            let after_dir = if j < bytes.len() { j + 1 } else { k_end + 1 };

            let is_if = keyword == "if"
                && k_end < bytes.len()
                && (bytes[k_end] == b':' || bytes[k_end] == b' ' || bytes[k_end] == b'}');
            let is_else = keyword == "else";
            let is_end = keyword == "end";

            if is_end && cond_depth == 0 {
                let then_text: &str = if let Some(em) = else_marker {
                    &original[..em]
                } else {
                    &original[..pos]
                };
                let else_text = else_after.map(|ea| &original[ea..pos]);
                *input = &original[after_dir..];
                return Ok((then_text, else_text));
            } else if is_else && cond_depth == 0 {
                else_marker = Some(pos);
                else_after = Some(after_dir);
                pos = after_dir;
                continue;
            } else if is_if {
                cond_depth += 1;
            } else if is_end {
                cond_depth -= 1;
            }
            pos = after_dir;
            continue;
        }
        pos += 1;
    }

    // No {end} found — consume rest as then-text
    let then_text = *input;
    *input = &original[original.len()..];
    Ok((then_text, None))
}

// ── individual directive parsers ─────────────────────────────────────────

fn parse_state_directive(args: &str) -> ContentNode {
    let path = args.trim().trim_matches('"').trim();
    ContentNode::StateInterp {
        path: path.to_string(),
    }
}

fn parse_set_directive(args: &str) -> ContentNode {
    let mut macro_args = Vec::new();
    for part in args.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(eq) = part.find('=') {
            let key = part[..eq].trim().to_string();
            let val_str = part[eq + 1..].trim();
            let val = parse_value_literal(val_str);
            macro_args.push(MacroArg::KeyValue(key, val));
        } else {
            macro_args.push(MacroArg::Positional(
                pathos_core::value::Value::String(part.to_string()),
            ));
        }
    }
    ContentNode::Macro {
        name: "set".into(),
        args: macro_args,
    }
}

fn parse_display_directive(args: &str) -> ContentNode {
    let name = args.trim().trim_matches('"').trim();
    ContentNode::Display {
        passage: name.to_string(),
    }
}

fn parse_ai_directive(name: &str, args: &str) -> ContentNode {
    let mode = match name {
        "ai-stream" => AIMode::Streaming,
        "ai-cached" => AIMode::Cached,
        _ => AIMode::Blocking,
    };
    let args = args.trim();
    let mut cache_key = None;
    let rest = if mode == AIMode::Cached && args.starts_with("key=\"") {
        if let Some(end) = args[5..].find('"') {
            cache_key = Some(args[5..5 + end].to_string());
            args[5 + end + 1..].trim()
        } else {
            args
        }
    } else if mode == AIMode::Cached && args.starts_with("key=") {
        if let Some(space) = args[4..].find(' ') {
            cache_key = Some(args[4..4 + space].to_string());
            args[4 + space..].trim()
        } else {
            args
        }
    } else {
        args
    };

    let (prompt, fallback) = if let Some(pipe) = rest.find('|') {
        (
            rest[..pipe].trim().to_string(),
            rest[pipe + 1..].trim().to_string(),
        )
    } else {
        (rest.to_string(), String::new())
    };
    ContentNode::AIBlock {
        mode,
        prompt,
        fallback,
        cache_key,
    }
}

// ── macro argument & value parsing ───────────────────────────────────────

fn parse_macro_args(args_str: &str) -> Vec<MacroArg> {
    let mut args = Vec::new();
    for part in split_args(args_str) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(eq) = part.find('=') {
            let key = part[..eq].trim().to_string();
            let val_str = part[eq + 1..].trim();
            let val = parse_value_literal(val_str);
            args.push(MacroArg::KeyValue(key, val));
        } else {
            let val = parse_value_literal(part);
            args.push(MacroArg::Positional(val));
        }
    }
    args
}

fn split_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut in_quote = false;
    let mut quote_char = '"';
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, &ch) in bytes.iter().enumerate() {
        if in_quote {
            if ch == quote_char as u8 {
                in_quote = false;
            }
        } else {
            match ch {
                b'"' | b'\'' => {
                    in_quote = true;
                    quote_char = ch as char;
                }
                b'{' => depth += 1,
                b'}' => depth -= 1,
                b',' if depth == 0 => {
                    result.push(s[start..i].to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    result.push(s[start..].to_string());
    result
}

fn parse_value_literal(s: &str) -> pathos_core::value::Value {
    let s = s.trim();
    if s == "true" {
        pathos_core::value::Value::Bool(true)
    } else if s == "false" {
        pathos_core::value::Value::Bool(false)
    } else if s == "null" {
        pathos_core::value::Value::Null
    } else if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        pathos_core::value::Value::String(s[1..s.len() - 1].to_string())
    } else if let Ok(i) = s.parse::<i64>() {
        pathos_core::value::Value::Int(i)
    } else if let Ok(f) = s.parse::<f64>() {
        pathos_core::value::Value::float(f).unwrap_or(pathos_core::value::Value::Null)
    } else {
        pathos_core::value::Value::String(s.to_string())
    }
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_text() {
        let (nodes, diags) = parse_inline("Hello world.");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], ContentNode::Text(ref s) if s == "Hello world."));
    }

    #[test]
    fn parse_simple_link() {
        let (nodes, diags) = parse_inline("[[Go \u{2192} room]]");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], ContentNode::Link { ref label, ref target, .. }
            if label == "Go" && target == "room"));
    }

    #[test]
    fn parse_link_with_arrow_style() {
        let (nodes, diags) = parse_inline("[[Enter -> dungeon]]");
        assert!(diags.is_empty());
        assert!(matches!(nodes[0], ContentNode::Link { ref label, ref target, .. }
            if label == "Enter" && target == "dungeon"));
    }

    #[test]
    fn parse_state_interp() {
        let (nodes, diags) = parse_inline(r#"{state: "player.hp"}"#);
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], ContentNode::StateInterp { ref path } if path == "player.hp"));
    }

    #[test]
    fn parse_state_interp_unquoted() {
        let (nodes, diags) = parse_inline("{state: player.hp}");
        assert!(diags.is_empty());
        assert!(matches!(nodes[0], ContentNode::StateInterp { ref path } if path == "player.hp"));
    }

    #[test]
    fn parse_set_macro() {
        let (nodes, diags) = parse_inline("{set: hp = 10}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            ContentNode::Macro { name, args } => {
                assert_eq!(name, "set");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected Macro"),
        }
    }

    #[test]
    fn parse_display_directive() {
        let (nodes, diags) = parse_inline(r#"{display: "room"}"#);
        assert!(diags.is_empty());
        assert!(matches!(nodes[0], ContentNode::Display { ref passage } if passage == "room"));
    }

    #[test]
    fn parse_ai_block() {
        let (nodes, diags) = parse_inline("{ai: describe | fallback text}");
        assert!(diags.is_empty());
        assert!(matches!(nodes[0], ContentNode::AIBlock {
            mode: AIMode::Blocking, ref prompt, ref fallback, ..
        } if prompt == "describe" && fallback == "fallback text"));
    }

    #[test]
    fn parse_ai_stream_block() {
        let (nodes, diags) = parse_inline("{ai-stream: describe room | A room.}");
        assert!(diags.is_empty());
        assert!(matches!(nodes[0], ContentNode::AIBlock {
            mode: AIMode::Streaming, ref prompt, ..
        } if prompt == "describe room"));
    }

    #[test]
    fn parse_mixed_content() {
        let (nodes, diags) = parse_inline("You see a [[door \u{2192} hallway]]. {state: player.hp} HP remaining.");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 5);
    }

    #[test]
    fn parse_link_in_text() {
        let (nodes, diags) = parse_inline("Go [[north \u{2192} forest]] now.");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0], ContentNode::Text(ref s) if s == "Go "));
        assert!(matches!(nodes[1], ContentNode::Link { .. }));
        assert!(matches!(nodes[2], ContentNode::Text(ref s) if s == " now."));
    }

    #[test]
    fn regression_pos_advances_past_directive() {
        let (nodes, diags) = parse_inline("{state: hp} and {display: room}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0], ContentNode::StateInterp { .. }));
        assert!(matches!(nodes[1], ContentNode::Text(ref s) if s == " and "));
        assert!(matches!(nodes[2], ContentNode::Display { .. }));
    }

    #[test]
    fn regression_pos_adjacent_directives() {
        let (nodes, diags) = parse_inline("{state: hp}{display: room}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], ContentNode::StateInterp { .. }));
        assert!(matches!(nodes[1], ContentNode::Display { .. }));
    }

    // ── conditional blocks ─────────────────────────────────────────────

    #[test]
    fn parse_simple_if_end() {
        let (nodes, diags) = parse_inline("{if: $hp > 0} alive {end}");
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            ContentNode::Conditional { condition, then_branch, else_branch } => {
                assert!(matches!(condition, Expression::Gt(_, _)));
                assert_eq!(then_branch.len(), 1);
                assert!(matches!(then_branch[0], ContentNode::Text(ref s) if s == " alive "));
                assert!(else_branch.is_empty());
            }
            other => panic!("expected Conditional, got {:?}", other),
        }
    }

    #[test]
    fn parse_if_else_end() {
        let (nodes, diags) = parse_inline("{if: $wisdom > 2} wise {else} foolish {end}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            ContentNode::Conditional { then_branch, else_branch, .. } => {
                assert_eq!(then_branch.len(), 1);
                assert!(matches!(then_branch[0], ContentNode::Text(ref s) if s == " wise "));
                assert_eq!(else_branch.len(), 1);
                assert!(matches!(else_branch[0], ContentNode::Text(ref s) if s == " foolish "));
            }
            other => panic!("expected Conditional, got {:?}", other),
        }
    }

    #[test]
    fn parse_nested_if_blocks() {
        let src = "{if: $a > 0} a_pos {if: $b > 0} ab_pos {else} a_pos_b_neg {end} {else} a_neg {end}";
        let (nodes, diags) = parse_inline(src);
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            ContentNode::Conditional { then_branch, else_branch, .. } => {
                assert!(!then_branch.is_empty());
                assert_eq!(else_branch.len(), 1);
                assert!(matches!(else_branch[0], ContentNode::Text(ref s) if s == " a_neg "));
            }
            other => panic!("expected Conditional, got {:?}", other),
        }
    }

    #[test]
    fn parse_if_with_inline_directives_inside() {
        let src = "{if: $hp > 0} HP: {state: player.hp} [[heal -> healer]] {end}";
        let (nodes, diags) = parse_inline(src);
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            ContentNode::Conditional { then_branch, .. } => {
                assert!(then_branch.iter().any(|n| matches!(n, ContentNode::StateInterp { .. })));
                assert!(then_branch.iter().any(|n| matches!(n, ContentNode::Link { .. })));
            }
            other => panic!("expected Conditional, got {:?}", other),
        }
    }

    #[test]
    fn parse_standalone_else_diagnostic() {
        let (_nodes, diags) = parse_inline("text {else} more text");
        assert!(!diags.is_empty());
        assert!(diags.iter().any(|d| d.message.contains("{else}")),
            "expected diagnostic for orphan {{else}}, got: {:?}", diags);
    }

    #[test]
    fn parse_standalone_end_diagnostic() {
        let (_nodes, diags) = parse_inline("text {end} more");
        assert!(!diags.is_empty());
        assert!(diags.iter().any(|d| d.message.contains("{end}")),
            "expected diagnostic for orphan {{end}}, got: {:?}", diags);
    }

    #[test]
    fn regression_pos_advances_past_conditional() {
        let (nodes, diags) = parse_inline("{if: true} then {end} follow-up text");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], ContentNode::Conditional { .. }));
        assert!(matches!(nodes[1], ContentNode::Text(ref s) if s == " follow-up text"));
    }

    #[test]
    fn regression_pos_adjacent_to_conditional() {
        let (nodes, diags) = parse_inline("{if: true} then {end}{state: hp}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], ContentNode::Conditional { .. }));
        assert!(matches!(nodes[1], ContentNode::StateInterp { .. }));
    }

}
