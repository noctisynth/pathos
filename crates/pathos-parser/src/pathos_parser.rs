//! Parser for `.pathos` files (the native Pathos format).
//!
//! Four-pass pipeline:
//!   P1 — Split source into YAML frontmatter + passage blocks (via `---`)
//!   P2 — Parse block-level elements (headings, @hook, fenced code blocks)
//!   P3 — Parse inline elements (links, macros, AI blocks, state interpolation)
//!   P4 — Semantic analysis (build graph edges, validate references)

use pathos_core::{ContentNode, HookDeclaration, PassageGraph, PassageNode, PassageScript, ScriptLang, StoryConfig,
};
use pathos_core::expression::Expression;
use crate::format::{Diagnostic, FormatParser, ParseOutput, Severity};

pub struct PathosParser;

impl FormatParser for PathosParser {
    fn extensions(&self) -> &[&str] {
        &["pathos"]
    }

    fn parse(&self, source: &str) -> ParseOutput {
        let mut diagnostics = Vec::new();

        // ── P1: Split source into frontmatter + passage blocks ──────────
        let (frontmatter_str, passage_blocks) = split_source(source, &mut diagnostics);

        // ── Parse frontmatter ────────────────────────────────────────────
        let (config, fm_diags) = parse_frontmatter(&frontmatter_str);
        diagnostics.extend(fm_diags);

        // ── P2 + P3: Parse each passage block ────────────────────────────
        let mut nodes: Vec<PassageNode> = Vec::new();
        for block in &passage_blocks {
            let (node, pb_diags) = parse_passage_block(block);
            diagnostics.extend(pb_diags);
            nodes.push(node);
        }

        // Verify start passage exists
        if !nodes.iter().any(|n| n.id == config.start) && !config.start.is_empty() {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                message: format!("start passage '{}' not found", config.start),
                span: None,
            });
        }

        // ── P4: Semantic analysis ────────────────────────────────────────
        let mut graph = PassageGraph { nodes, edges: Vec::new() };
        graph.rebuild_edges();

        let p4_diags = semantic_analysis(&graph, &config);
        diagnostics.extend(p4_diags);

        ParseOutput { config, graph, diagnostics }
    }
}

// ── P1: Source splitting ──────────────────────────────────────────────────

fn split_source(source: &str, diagnostics: &mut Vec<Diagnostic>) -> (String, Vec<String>) {
    let mut _frontmatter = String::new();


    let mut lines = source.lines().peekable();

    // Check for YAML frontmatter (starts with `---`)
    if let Some(first) = lines.peek() {
        if first.trim() == "---" {
            lines.next(); // consume opening `---`
            let mut fm_lines = Vec::new();
            for line in lines.by_ref() {
                if line.trim() == "---" {
                    break; // closing `---`
                }
                fm_lines.push(line);
            }
            _frontmatter = fm_lines.join("\n");
        }
    }

    // Collect remaining text and split into passage blocks
    let remaining: Vec<&str> = lines.collect();
    let remaining_text = remaining.join("\n");

    // Split by `# heading` or `---` boundaries
    let blocks = split_into_passage_blocks(&remaining_text, diagnostics);
    (_frontmatter, blocks)
}

/// Split text into passage blocks. A new passage starts at `# name` or `---`.
fn split_into_passage_blocks(text: &str, _diagnostics: &mut Vec<Diagnostic>) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    // Track whether we've seen a non-empty line yet in this block.
    let mut seen_content = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // `---` separator starts a new passage (only at block boundaries)
        if trimmed == "---" && !seen_content {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            seen_content = false;
            continue;
        }

        // `# name` starts a new passage
        if trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
            current.push('\n');
            seen_content = true;
            continue;
        }

        // Skip leading blank lines before the first heading
        if !seen_content && trimmed.is_empty() {
            continue;
        }

        current.push_str(line);
        current.push('\n');
        if !trimmed.is_empty() {
            seen_content = true;
        }
    }

    if !current.trim().is_empty() {
        blocks.push(current);
    }

    blocks
}

// ── Frontmatter parsing ──────────────────────────────────────────────────

fn parse_frontmatter(yaml: &str) -> (StoryConfig, Vec<Diagnostic>) {
    if yaml.trim().is_empty() {
        return (StoryConfig {
            title: String::new(),
            author: String::new(),
            start: String::new(),
            version: String::new(),
            save_slots: 10,
        }, Vec::new());
    }

    #[derive(serde::Deserialize)]
    struct Frontmatter {
        #[serde(default)]
        title: String,
        #[serde(default)]
        author: String,
        #[serde(default)]
        start: String,
        #[serde(default = "default_version")]
        version: String,
        #[serde(default = "default_save_slots")]
        save_slots: u8,
    }

    fn default_version() -> String { "0.1.0".into() }
    fn default_save_slots() -> u8 { 10 }

    match serde_yaml::from_str::<Frontmatter>(yaml) {
        Ok(fm) => (StoryConfig {
            title: fm.title,
            author: fm.author,
            start: fm.start,
            version: fm.version,
            save_slots: fm.save_slots,
        }, Vec::new()),
        Err(e) => (StoryConfig::default(), vec![Diagnostic {
            severity: Severity::Error,
            message: format!("frontmatter parse error: {e}"),
            span: None,
        }]),
    }
}



// ── P2: Passage block parsing (heading, @hook, scripts, body) ────────────

fn parse_passage_block(block: &str) -> (PassageNode, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut id = String::new();
    let mut tags = Vec::new();
    let _body_lines: Vec<&str> = Vec::new();
    let mut scripts = Vec::new();
    let mut hooks = Vec::new();

    let mut lines = block.lines().peekable();

    // Parse heading line: `# name {tag1, tag2}`
    if let Some(heading) = lines.next() {
        let heading = heading.trim();
        if let Some(rest) = heading.strip_prefix("# ") {
            let rest = rest.trim();
            if let Some(brace) = rest.find('{') {
                id = rest[..brace].trim().to_string();
                let tag_part = &rest[brace..];
                if let Some(end_brace) = tag_part.find('}') {
                    let tags_str = &tag_part[1..end_brace];
                    tags = tags_str.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                } else {
                    // Fallback: use remaining as id
                    id = rest.to_string();
                }
            } else {
                id = rest.to_string();
            }
        }
    }

    // Parse remaining lines — scan for @hook: and fenced code blocks
    let remaining: Vec<&str> = lines.collect();
    let mut i = 0;
    let mut body_text = String::new();
    let mut in_block_comment = false;

    while i < remaining.len() {
        let line = remaining[i];

        // @hook: directive
        if line.trim().starts_with("@hook:") {
            let event = line.trim().strip_prefix("@hook:").unwrap_or("").trim().to_string();
            i += 1;
            // Expect a fenced code block next
            if i < remaining.len() && remaining[i].trim().starts_with("```") {
                let info = remaining[i].trim().strip_prefix("```").unwrap_or("").trim();
                let lang = if info.is_empty() { "rhai".to_string() } else { info.to_string() };
                // P2: validate language tag against known ScriptLang variants
                if ScriptLang::from_info_string(&lang).is_none() {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!("unknown script language: '{}'", lang),
                        span: None,
                    });
                }
                i += 1;
                let mut code_lines = Vec::new();
                while i < remaining.len() && !remaining[i].trim().starts_with("```") {
                    code_lines.push(remaining[i]);
                    i += 1;
                }
                if i < remaining.len() {
                    i += 1; // skip closing ```
                }
                let script = PassageScript { lang: lang.clone(), code: code_lines.join("\n") };
                hooks.push(HookDeclaration { event, script });
                continue;
            }
        }

        // Standalone fenced code block (passage script)
        if line.trim().starts_with("```") {
            let info = line.trim().strip_prefix("```").unwrap_or("").trim();
            let lang = if info.is_empty() { "rhai".to_string() } else { info.to_string() };
                // P2: validate language tag against known ScriptLang variants
                if ScriptLang::from_info_string(&lang).is_none() {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!("unknown script language: '{}'", lang),
                        span: None,
                    });
                }
            i += 1;
            let mut code_lines = Vec::new();
            while i < remaining.len() && !remaining[i].trim().starts_with("```") {
                code_lines.push(remaining[i]);
                i += 1;
            }
            if i < remaining.len() {
                i += 1; // skip closing ```
            }
            scripts.push(PassageScript { lang, code: code_lines.join("\n") });
            continue;
        }

        // Regular body line — strip line and block comments
        if in_block_comment {
            if let Some(end) = line.find("-->") {
                in_block_comment = false;
                let after = &line[end + 3..];
                if !after.is_empty() {
                    body_text.push_str(after);
                    body_text.push('\n');
                }
            }
        } else {
            let slash_pos = line.find("//");
            let block_open = line.find("<!--");

            match (slash_pos, block_open) {
                (Some(sp), Some(bp)) if sp < bp => {
                    body_text.push_str(&line[..sp]);
                    body_text.push('\n');
                }
                (_, Some(bp)) => {
                    body_text.push_str(&line[..bp]);
                    let rest = &line[bp + 4..];
                    if let Some(end) = rest.find("-->") {
                        body_text.push_str(&rest[end + 3..]);
                    } else {
                        in_block_comment = true;
                    }
                    body_text.push('\n');
                }
                (Some(sp), None) => {
                    body_text.push_str(&line[..sp]);
                    body_text.push('\n');
                }
                (None, None) => {
                    body_text.push_str(line);
                    body_text.push('\n');
                }
            }
        }
        i += 1;
    }

    // ── P3: Inline parsing of body text ──────────────────────────────────
    let (body, inline_diags) = crate::inline::parse_inline(&body_text);
    diagnostics.extend(inline_diags);

    (PassageNode { id, tags, body, scripts, hooks }, diagnostics)
}

// ── P4: Semantic analysis ────────────────────────────────────────────────

fn semantic_analysis(graph: &PassageGraph, config: &StoryConfig) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let start_id = &config.start;

    // ── Collect all passage IDs reachable via links / display / edges ───
    let mut referenced: std::collections::HashSet<&str> = std::collections::HashSet::new();
    referenced.insert(start_id.as_str());

    // Walk all content nodes recursively to collect references + validate
    for node in &graph.nodes {
        walk_content_for_semantics(
            &node.body,
            &node.id,
            graph,
            &mut diagnostics,
            &mut referenced,
        );
    }

    // Edge targets from links
    for edge in &graph.edges {
        referenced.insert(edge.to.as_str());
        if graph.get(&edge.to).is_none() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "link target '{}' not found (from '{}')",
                    edge.to, edge.from
                ),
                span: None,
            });
        }
    }

    // ── Unused passages (not start, not referenced by any link/display) ──
    for node in &graph.nodes {
        if node.id == *start_id {
            continue;
        }
        if !referenced.contains(node.id.as_str()) {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("passage '{}' is never referenced", node.id),
                span: None,
            });
        }
    }

    diagnostics
}

/// Recursively walk content nodes for P4 semantic validation.
fn walk_content_for_semantics<'a>(
    nodes: &'a [ContentNode],
    current_passage: &str,
    graph: &PassageGraph,
    diagnostics: &mut Vec<Diagnostic>,
    referenced: &mut std::collections::HashSet<&'a str>,
) {
    for cn in nodes {
        match cn {
            ContentNode::Link { target, enabled_if, .. } => {
                referenced.insert(target.as_str());
                if graph.get(target).is_none() {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!(
                            "link target '{}' not found (from '{}')",
                            target, current_passage
                        ),
                        span: None,
                    });
                }
                if let Some(expr) = enabled_if {
                    validate_expression(expr, diagnostics);
                }
            }
            ContentNode::Display { passage } => {
                referenced.insert(passage.as_str());
                if graph.get(passage).is_none() {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!(
                            "display target '{}' not found (from '{}')",
                            passage, current_passage
                        ),
                        span: None,
                    });
                }
            }
            ContentNode::Conditional { condition, then_branch, else_branch } => {
                validate_expression(condition, diagnostics);
                walk_content_for_semantics(then_branch, current_passage, graph, diagnostics, referenced);
                walk_content_for_semantics(else_branch, current_passage, graph, diagnostics, referenced);
            }
            ContentNode::Macro { args, .. } => {
                // Macros may contain inline links / display — but macro args
                // are KeyValue / Positional, not ContentNode.  Skip for now.
                let _ = args;
            }
            _ => {}
        }
    }
}

/// Validate a single expression AST (check function call names).
fn validate_expression(expr: &Expression, diagnostics: &mut Vec<Diagnostic>) {
    match expr {
        Expression::Call { name, args } => {
            let known = matches!(
                name.as_str(),
                "random" | "has_tag" | "visited" | "count"
            );
            if !known {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!("unknown function in expression: '{}'", name),
                    span: None,
                });
            }
            // Validate arg count for known functions
            match name.as_str() {
                "has_tag" | "visited" | "count" => {
                    if args.len() != 1 {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!(
                                "'{}' expects 1 argument, got {}", name, args.len()
                            ),
                            span: None,
                        });
                    }
                }
                "random" => {
                    if args.len() > 2 {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!(
                                "'random' expects 0-2 arguments, got {}", args.len()
                            ),
                            span: None,
                        });
                    }
                }
                _ => {}
            }
            // Recursively validate sub-expressions in arguments
            for arg in args {
                validate_expression(arg, diagnostics);
            }
        }
        Expression::Not(inner) => {
            validate_expression(inner, diagnostics);
        }
        Expression::StateVar(_) => {
            // StateVar is runtime — no validation needed at P4
        }
        Expression::And(lhs, rhs)
        | Expression::Or(lhs, rhs)
        | Expression::Eq(lhs, rhs)
        | Expression::NotEq(lhs, rhs)
        | Expression::Lt(lhs, rhs)
        | Expression::Lte(lhs, rhs)
        | Expression::Gt(lhs, rhs)
        | Expression::Gte(lhs, rhs)
        | Expression::Add(lhs, rhs)
        | Expression::Sub(lhs, rhs)
        | Expression::Mul(lhs, rhs)
        | Expression::Div(lhs, rhs) => {
            validate_expression(lhs, diagnostics);
            validate_expression(rhs, diagnostics);
        }
        Expression::Literal(_) => {}
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use pathos_core::ContentNode;

    #[test]
    fn parse_minimal_pathos() {
        let src = "\
---
title: Test
author: Author
start: intro
---

# intro {opening}
Welcome to the story.

[[Go \u{2192} room]]

# room
You are in a room.
";
        let output = PathosParser.parse(src);
        assert_eq!(output.config.title, "Test");
        assert_eq!(output.config.start, "intro");
        assert_eq!(output.graph.nodes.len(), 2);
        // Check P4 built edges from [[link]]
        assert!(!output.graph.edges.is_empty());
        assert_eq!(output.graph.edges[0].from, "intro");
        assert_eq!(output.graph.edges[0].to, "room");
    }

    #[test]
    fn parse_pathos_with_hooks() {
        let src = "\
---
title: Test
author: A
start: intro
---

# intro
@hook: on_passage_start
```rhai
state.set('hp', 100);
```

Welcome.

# room
Some room.
";
        let output = PathosParser.parse(src);
        let intro = output.graph.get("intro").unwrap();
        assert_eq!(intro.hooks.len(), 1);
        assert_eq!(intro.hooks[0].event, "on_passage_start");
        assert_eq!(intro.hooks[0].script.lang, "rhai");
        assert!(intro.hooks[0].script.code.contains("state.set"));
    }

    #[test]
    fn parse_pathos_with_scripts() {
        let src = "\
---
title: Test
author: A
start: intro
---

# intro
```rhai
state.set('wisdom', 1);
```

You feel wiser.

# room
A room.
";
        let output = PathosParser.parse(src);
        let intro = output.graph.get("intro").unwrap();
        assert_eq!(intro.scripts.len(), 1);
        assert_eq!(intro.scripts[0].lang, "rhai");
        assert!(intro.scripts[0].code.contains("state.set"));
    }

    #[test]
    fn parse_pathos_inline_directives() {
        let src = "\
---
title: Test
author: A
start: intro
---

# intro
HP: {state: player.hp}

[[Fight \u{2192} battle]]
";
        let output = PathosParser.parse(src);
        let intro = output.graph.get("intro").unwrap();
        // Body should contain StateInterp and Link
        assert!(intro.body.iter().any(|n| matches!(n, ContentNode::StateInterp { .. })));
        assert!(intro.body.iter().any(|n| matches!(n, ContentNode::Link { .. })));
    }

    #[test]
    fn parse_pathos_orphan_link_warning() {
        let src = "\
---
title: Test
author: A
start: intro
---

# intro
[[Go \u{2192} nowhere]]
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| d.severity == Severity::Warning).collect();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|d| d.message.contains("nowhere")));
    }

    /// Regression test: frontmatter followed by a blank line must not
    /// swallow the first passage heading.
    #[test]
    fn regression_frontmatter_blank_line_before_heading() {
        let src = "\
---
title: Blank Line Test
start: intro
---

# intro
Hello.
";
        let output = PathosParser.parse(src);
        let intro = output.graph.get("intro").expect("passage 'intro' should exist");
        assert_eq!(intro.id, "intro");
        assert!(intro.body.iter().any(|n| matches!(n, ContentNode::Text(s) if s.trim() == "Hello.")));
    }

    /// Variant: two blank lines before the heading.
    #[test]
    fn regression_frontmatter_two_blank_lines_before_heading() {
        let src = "\
---
title: Two Blanks
start: start
---


# start
Begin.
";
        let output = PathosParser.parse(src);
        let start = output.graph.get("start").expect("passage 'start' should exist");
        assert_eq!(start.id, "start");
    }

    /// Unknown script language in fenced code block emits a Warning diagnostic.
    #[test]
    fn unknown_script_language_warning() {
        let src = "---
title: Test
author: A
start: intro
---

# intro
```python
x = 1
```

Hello.
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            warnings.iter().any(|d| d.message.contains("python")),
            "expected warning about unknown script language 'python', got: {:?}",
            warnings
        );
        // The script should still be parsed and stored (degraded gracefully)
        let intro = output.graph.get("intro").unwrap();
        assert_eq!(intro.scripts.len(), 1);
        assert_eq!(intro.scripts[0].lang, "python");
    }


    // ── Comment stripping tests ────────────────────────────────────

    #[test]
    fn strip_double_slash_line_comment() {
        let src = "\
---
title: T
start: p1
---

# p1
visible text // this is a comment
more text
";
        let output = PathosParser.parse(src);
        let p1 = output.graph.get("p1").unwrap();
        assert!(!p1.body.iter().any(|n| {
            matches!(n, ContentNode::Text(s) if s.contains("this is a comment"))
        }));
        assert!(p1.body.iter().any(|n| {
            matches!(n, ContentNode::Text(s) if s.contains("visible text"))
        }));
    }

    #[test]
    fn strip_block_comment() {
        let src = "\
---
title: T
start: p1
---

# p1
before <!-- hidden block --> after
";
        let output = PathosParser.parse(src);
        let p1 = output.graph.get("p1").unwrap();
        let body_text: String = p1.body.iter().filter_map(|n| {
            match n {
                ContentNode::Text(s) => Some(s.as_str()),
                _ => None,
            }
        }).collect::<Vec<_>>().join(" ");
        assert!(body_text.contains("before"), "expected 'before' in: {}", body_text);
        assert!(body_text.contains("after"), "expected 'after' in: {}", body_text);
        assert!(!body_text.contains("hidden block"), "unexpected 'hidden block' in: {}", body_text);
    }

    #[test]
    fn strip_multiline_block_comment() {
        let src = "\
---
title: T
start: p1
---

# p1
keep this <!-- start of hidden
this should be hidden
still hidden --> keep this too
";
        let output = PathosParser.parse(src);
        let p1 = output.graph.get("p1").unwrap();
        let body_text: String = p1.body.iter().filter_map(|n| {
            match n {
                ContentNode::Text(s) => Some(s.as_str()),
                _ => None,
            }
        }).collect::<Vec<_>>().join(" ");
        assert!(body_text.contains("keep this"), "expected 'keep this' in: {}", body_text);
        assert!(body_text.contains("keep this too"), "expected 'keep this too' in: {}", body_text);
        assert!(!body_text.contains("should be hidden"), "unexpected hidden text: {}", body_text);
    }




    // ── P4 semantic analysis tests ─────────────────────────────────

    #[test]
    fn p4_warns_unknown_function() {
        let src = "\
---
title: T
start: p1
---

# p1
{if: foobar(1)}
  yes
{else}
  no
{end}
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .collect();
        assert!(
            warnings.iter().any(|d| d.message.contains("unknown function")),
            "expected unknown function warning, got: {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn p4_warns_display_target_not_found() {
        let src = "\
---
title: T
start: p1
---

# p1
{display: nonexistent}

# p2
Hello.
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .collect();
        assert!(
            warnings.iter().any(|d| d.message.contains("display target") && d.message.contains("nonexistent")),
            "expected display target warning, got: {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn p4_warns_link_target_not_found() {
        let src = "\
---
title: T
start: p1
---

# p1
[[Go -> nowhere]]
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .collect();
        assert!(
            warnings.iter().any(|d| d.message.contains("link target") && d.message.contains("nowhere")),
            "expected link target warning, got: {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn p4_warns_unused_passage() {
        let src = "\
---
title: T
start: p1
---

# p1
Hello.

# orphan
Nobody links here.
";
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .collect();
        assert!(
            warnings.iter().any(|d| d.message.contains("never referenced") && d.message.contains("orphan")),
            "expected unused passage warning, got: {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn p4_no_warning_for_valid_if_expression() {
        let src = concat!(
            "---\n",
            "title: T\n",
            "start: p1\n",
            "---\n",
            "\n",
            "# p1\n",
            "{if: visited(\"cave\") && has_tag(\"dark\")}\n",
            "  You remember the cave.\n",
            "{end}\n",
        );
        let output = PathosParser.parse(src);
        let warnings: Vec<_> = output.diagnostics.iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .collect();
        assert!(
            warnings.is_empty(),
            "expected no warnings for valid expression, got: {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }


}
