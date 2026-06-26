//! Inline parser (P3) — parses inline directives within passage body text.
//!
//! Handles: `[[link]]`, `{state:}`, `{set:}`, `{ai:}`, `{display:}`, `{if:}`,
//! and generic `{name: args}` macro calls.

use pathos_core::{ContentNode, MacroArg, AIMode};
use crate::format::{Diagnostic, Severity, SourceSpan};

/// Parse a passage body string into a `Vec<ContentNode>` with inline elements resolved.
pub fn parse_inline(source: &str) -> (Vec<ContentNode>, Vec<Diagnostic>) {
    let mut nodes = Vec::new();
    let mut diagnostics = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        if pos + 1 < len && chars[pos] == '[' && chars[pos + 1] == '[' {
            if let Some((node, new_pos)) = parse_link(&chars, pos, &mut diagnostics) {
                nodes.push(node);
                pos = new_pos;
                continue;
            }
        }
        if chars[pos] == '{' {
            if let Some((node, new_pos)) = parse_directive(&chars, pos, &mut diagnostics) {
                nodes.push(node);
                pos = new_pos;
                continue;
            }
        }
        if pos + 3 < len && chars[pos] == '<' && chars[pos+1] == '!' 
           && chars[pos+2] == '-' && chars[pos+3] == '-' {
            if let Some(new_pos) = skip_comment(&chars, pos) {
                pos = new_pos;
                continue;
            }
        }
        if pos + 1 < len && chars[pos] == '/' && chars[pos + 1] == '/' {
            if let Some(new_pos) = skip_line_comment(&chars, pos) {
                pos = new_pos;
                continue;
            }
        }
        // Plain text
        let start = pos;
        while pos < len {
            if chars[pos] == '[' && pos + 1 < len && chars[pos + 1] == '[' { break; }
            if chars[pos] == '{' { break; }
            if chars[pos] == '<' && pos + 3 < len && chars[pos+1] == '!' 
               && chars[pos+2] == '-' && chars[pos+3] == '-' { break; }
            if chars[pos] == '/' && pos + 1 < len && chars[pos + 1] == '/' { break; }
            pos += 1;
        }
        if pos > start {
            let text: String = chars[start..pos].iter().collect();
            nodes.push(ContentNode::Text(text));
        }
    }
    (nodes, diagnostics)
}

fn parse_link(chars: &[char], start: usize, diagnostics: &mut Vec<Diagnostic>) 
    -> Option<(ContentNode, usize)> 
{
    let mut pos = start + 2;
    let label_start = pos;
    while pos < chars.len() 
        && !(chars[pos] == '-' && pos + 1 < chars.len() && chars[pos + 1] == '>')
        && !(chars[pos] == '\u{2192}')
        && !(chars[pos] == ']' && pos + 1 < chars.len() && chars[pos + 1] == ']')
    { pos += 1; }
    let label: String = chars[label_start..pos].iter().collect();
    let label = label.trim().to_string();
    if label.is_empty() {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "link missing label".into(),
            span: Some(SourceSpan { line: 0, column: start, length: pos - start }),
        });
        return None;
    }
    if chars[pos] == '-' && pos + 1 < chars.len() && chars[pos + 1] == '>' {
        pos += 2;
    } else if chars[pos] == '\u{2192}' {
        pos += 1;
    } else {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "link missing arrow (-> or \u{2192})".into(),
            span: Some(SourceSpan { line: 0, column: start, length: pos - start }),
        });
        return None;
    }
    let target_start = pos;
    while pos < chars.len() 
        && !(chars[pos] == ']' && pos + 1 < chars.len() && chars[pos + 1] == ']')
    { pos += 1; }
    let target: String = chars[target_start..pos].iter().collect();
    let target = target.trim().to_string();
    if target.is_empty() {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "link missing target".into(),
            span: Some(SourceSpan { line: 0, column: start, length: pos - start }),
        });
        return None;
    }
    if pos + 1 < chars.len() && chars[pos] == ']' && chars[pos + 1] == ']' {
        pos += 2;
    } else {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "link missing closing ]]".into(),
            span: Some(SourceSpan { line: 0, column: start, length: pos - start }),
        });
        return None;
    }
    Some((ContentNode::Link { label, target, enabled_if: None }, pos))
}

fn parse_directive(chars: &[char], start: usize, diagnostics: &mut Vec<Diagnostic>) 
    -> Option<(ContentNode, usize)> 
{
    let mut pos = start + 1;
    let name_start = pos;
    while pos < chars.len() && (chars[pos].is_alphanumeric() 
        || chars[pos] == '_' || chars[pos] == '-') 
    { pos += 1; }
    let name: String = chars[name_start..pos].iter().collect();
    if name.is_empty() { return None; }

    if pos < chars.len() && chars[pos] == ':' {
        pos += 1;
    } else if pos < chars.len() && chars[pos] == '}' {
        pos += 1;
        return Some((ContentNode::Macro { name, args: vec![] }, pos));
    } else {
        return None;
    }

    let args_start = pos;
    let mut depth = 1;
    while pos < chars.len() && depth > 0 {
        if chars[pos] == '{' { depth += 1; }
        if chars[pos] == '}' { depth -= 1; }
        if depth > 0 { pos += 1; }
    }
    if depth != 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: format!("unclosed directive: {{{name}:..."),
            span: Some(SourceSpan { line: 0, column: start, length: pos - start }),
        });
        return None;
    }
    let args_str: String = chars[args_start..pos].iter().collect();
    pos += 1;

    match name.as_str() {
        "state" => Some((parse_state_directive(&args_str), pos)),
        "set" => Some((parse_set_directive(&args_str), pos)),
        "display" => Some((parse_display_directive(&args_str), pos)),
        "ai" | "ai-stream" | "ai-cached" => Some((parse_ai_directive(&name, &args_str), pos)),
        "if" => Some((ContentNode::Macro { name: "if".into(), args: vec![
            MacroArg::Positional(pathos_core::value::Value::String(args_str))
        ]}, pos)),
        "else" | "end" => {
            // Block markers — treat as text passthrough at inline level
            Some((ContentNode::Text(format!("{{{name}}}")), pos))
        }
        _ => {
            let args = parse_macro_args(&args_str);
            Some((ContentNode::Macro { name, args }, pos))
        }
    }
}

/// Parse a {state: "path"} directive. Always succeeds.
fn parse_state_directive(args: &str) -> ContentNode {
    let path = args.trim().trim_matches('"').trim();
    ContentNode::StateInterp { path: path.to_string() }
}

/// Parse a {set: key = val, ...} directive. Always succeeds.
fn parse_set_directive(args: &str) -> ContentNode {
    let mut macro_args = Vec::new();
    for part in args.split(',') {
        let part = part.trim();
        if let Some(eq) = part.find('=') {
            let key = part[..eq].trim().to_string();
            let val_str = part[eq + 1..].trim();
            let val = parse_value_literal(val_str);
            macro_args.push(MacroArg::KeyValue(key, val));
        } else if !part.is_empty() {
            macro_args.push(MacroArg::Positional(
                pathos_core::value::Value::String(part.to_string())
            ));
        }
    }
    ContentNode::Macro { name: "set".into(), args: macro_args }
}

/// Parse a {display: "passage_name"} directive. Always succeeds.
fn parse_display_directive(args: &str) -> ContentNode {
    let name = args.trim().trim_matches('"').trim();
    ContentNode::Display { passage: name.to_string() }
}

/// Parse {ai:}, {ai-stream:}, or {ai-cached:} directives. Always succeeds.
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
            cache_key = Some(args[5..5+end].to_string());
            args[5+end+1..].trim()
        } else { args }
    } else if mode == AIMode::Cached && args.starts_with("key=") {
        if let Some(space) = args[4..].find(' ') {
            cache_key = Some(args[4..4+space].to_string());
            args[4+space..].trim()
        } else { args }
    } else { args };

    let (prompt, fallback) = if let Some(pipe) = rest.find('|') {
        (rest[..pipe].trim().to_string(), rest[pipe + 1..].trim().to_string())
    } else {
        (rest.to_string(), String::new())
    };
    ContentNode::AIBlock { mode, prompt, fallback, cache_key }
}

fn parse_value_literal(s: &str) -> pathos_core::value::Value {
    let s = s.trim();
    if s == "true" { pathos_core::value::Value::Bool(true) }
    else if s == "false" { pathos_core::value::Value::Bool(false) }
    else if s == "null" { pathos_core::value::Value::Null }
    else if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        pathos_core::value::Value::String(s[1..s.len()-1].to_string())
    } else if let Ok(i) = s.parse::<i64>() {
        pathos_core::value::Value::Int(i)
    } else if let Ok(f) = s.parse::<f64>() {
        pathos_core::value::Value::float(f).unwrap_or(pathos_core::value::Value::Null)
    } else {
        pathos_core::value::Value::String(s.to_string())
    }
}

fn parse_macro_args(args_str: &str) -> Vec<MacroArg> {
    let mut args = Vec::new();
    for part in split_args(args_str) {
        let part = part.trim();
        if part.is_empty() { continue; }
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
    let chars: Vec<char> = s.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if in_quote {
            if ch == quote_char { in_quote = false; }
        } else {
            match ch {
                '"' | '\'' => { in_quote = true; quote_char = ch; }
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
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

fn skip_comment(chars: &[char], start: usize) -> Option<usize> {
    let mut pos = start + 4;
    while pos + 2 < chars.len() {
        if chars[pos] == '-' && chars[pos + 1] == '-' && chars[pos + 2] == '>' {
            return Some(pos + 3);
        }
        pos += 1;
    }
    None
}

fn skip_line_comment(chars: &[char], start: usize) -> Option<usize> {
    let mut pos = start + 2;
    while pos < chars.len() && chars[pos] != '\n' { pos += 1; }
    Some(pos)
}

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

    /// Regression test for infinite-loop bug: verify that parsing consecutive
    /// inline directives properly advances `pos` past each closing `}`, so the
    /// parser does not re-scan the same directive forever.
    #[test]
    fn regression_pos_advances_past_directive() {
        // Two directives separated by whitespace — pos must advance past the
        // first to reach the second.
        let (nodes, diags) = parse_inline("{state: hp} and {display: room}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0], ContentNode::StateInterp { .. }));
        assert!(matches!(nodes[1], ContentNode::Text(ref s) if s == " and "));
        assert!(matches!(nodes[2], ContentNode::Display { .. }));
    }

    /// Two adjacent directives with no whitespace between them; this stresses
    /// the `pos` boundary where `{` is immediately after `}`.
    #[test]
    fn regression_pos_adjacent_directives() {
        let (nodes, diags) = parse_inline("{state: hp}{display: room}");
        assert!(diags.is_empty());
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], ContentNode::StateInterp { .. }));
        assert!(matches!(nodes[1], ContentNode::Display { .. }));
    }
}
