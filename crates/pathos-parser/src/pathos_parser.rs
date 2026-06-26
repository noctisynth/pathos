//! Parser for `.pathos` files (the native Pathos format).
//!
//! Four-pass pipeline:
//!   P1 — Split source into YAML frontmatter + passage blocks (via `---`)
//!   P2 — Parse block-level elements (headings, @hook, fenced code blocks)
//!   P3 — Parse inline elements (links, macros, AI blocks, state interpolation)
//!   P4 — Semantic analysis (build graph edges, validate references)

use pathos_core::{HookDeclaration, PassageGraph, PassageNode, PassageScript, StoryConfig,
};
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

        let p4_diags = semantic_analysis(&graph);
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

        // Regular body line
        body_text.push_str(line);
        body_text.push('\n');
        i += 1;
    }

    // ── P3: Inline parsing of body text ──────────────────────────────────
    let (body, inline_diags) = crate::inline::parse_inline(&body_text);
    diagnostics.extend(inline_diags);

    (PassageNode { id, tags, body, scripts, hooks }, diagnostics)
}

// ── P4: Semantic analysis ────────────────────────────────────────────────

fn semantic_analysis(graph: &PassageGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check for orphan link targets
    for edge in &graph.edges {
        if graph.get(&edge.to).is_none() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("link target '{}' not found (from '{}')", edge.to, edge.from),
                span: None,
            });
        }
    }

    // Check for unused passages (no incoming edges, not start)
    // (Only warn about non-tag-navigated passages)
    diagnostics
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
}
