use std::collections::HashMap;
use serde::Deserialize;
use pathos_core::{
    HookDeclaration,
    PassageGraph,
    PassageNode,
    PassageScript,
    StoryConfig,
};

use crate::format::{Diagnostic, FormatParser, ParseOutput, Severity, SourceSpan};

// ── TOML schema types (private) ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StoryFileToml {
    story: StoryMetaToml,
    #[serde(default)]
    passages: HashMap<String, PassageToml>,
    #[serde(default)]
    scripts: Vec<PassageScriptToml>,
    #[serde(default)]
    hooks: Vec<PassageHookToml>,
}

#[derive(Debug, Deserialize)]
struct StoryMetaToml {
    title: String,
    author: String,
    start: String,
    #[serde(default = "default_version")]
    version: String,
    #[serde(default = "default_save_slots")]
    save_slots: u8,
}

fn default_version() -> String {
    "0.1.0".into()
}

fn default_save_slots() -> u8 {
    10
}

#[derive(Debug, Deserialize)]
struct PassageToml {
    #[serde(default)]
    tags: Vec<String>,
    /// The passage body text. May contain inline directives (`{ai:}`, `{state:}`,
    /// `[[link]]`, etc.) that will be parsed by the inline parser (P3).
    body: String,
}

#[derive(Debug, Deserialize)]
struct PassageScriptToml {
    /// The passage this script belongs to.
    passage: String,
    /// Script language tag: `"rhai"`, `"js"`, `"lua"`.
    lang: String,
    /// Script source code.
    code: String,
}

#[derive(Debug, Deserialize)]
struct PassageHookToml {
    /// The passage this hook belongs to.
    passage: String,
    /// Hook event name (e.g. `"on_passage_start"`).
    event: String,
    /// Script language tag.
    lang: String,
    /// Hook script source code.
    code: String,
}

// ── TomlParser ────────────────────────────────────────────────────────────

/// Parser for `.toml` story files.
///
/// ## TOML format
///
/// ```toml
/// [story]
/// title = "My Story"
/// author = "Author"
/// start = "intro"
///
/// [passages.intro]
/// tags = ["opening"]
/// body = "Welcome.\n\n[[Go → room]]"
///
/// [passages.room]
/// body = "You are in a room."
///
/// [[scripts]]
/// passage = "intro"
/// lang = "rhai"
/// code = "state.set('hp', 100);"
///
/// [[hooks]]
/// passage = "intro"
/// event = "on_passage_start"
/// lang = "rhai"
/// code = "state.set('hp', 100);"
/// ```
pub struct TomlParser;

impl FormatParser for TomlParser {
    fn extensions(&self) -> &[&str] {
        &["toml"]
    }

    fn parse(&self, source: &str) -> ParseOutput {
        let mut diagnostics = Vec::new();

        let file: StoryFileToml = match toml::from_str(source) {
            Ok(f) => f,
            Err(e) => {
                let span = e.span().map(|span| SourceSpan {
                    line: span.start,
                    column: span.end.saturating_sub(span.start),
                    length: span.end.saturating_sub(span.start),
                });
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!("TOML parse error: {e}"),
                    span,
                });
                return ParseOutput {
                    config: StoryConfig {
                        title: String::new(),
                        author: String::new(),
                        start: String::new(),
                        version: String::new(),
                        save_slots: 0,
                    },
                    graph: PassageGraph::default(),
                    diagnostics,
                };
            }
        };

        let config = StoryConfig {
            title: file.story.title,
            author: file.story.author,
            start: file.story.start,
            version: file.story.version,
            save_slots: file.story.save_slots,
        };

        // Build a lookup of scripts and hooks per passage
        let mut scripts_map: HashMap<String, Vec<PassageScript>> = HashMap::new();
        for s in &file.scripts {
            scripts_map
                .entry(s.passage.clone())
                .or_default()
                .push(PassageScript {
                    lang: s.lang.clone(),
                    code: s.code.clone(),
                });
        }

        let mut hooks_map: HashMap<String, Vec<HookDeclaration>> = HashMap::new();
        for h in &file.hooks {
            hooks_map
                .entry(h.passage.clone())
                .or_default()
                .push(HookDeclaration {
                    event: h.event.clone(),
                    script: PassageScript {
                        lang: h.lang.clone(),
                        code: h.code.clone(),
                    },
                });
        }

        // Build passage nodes
        let mut nodes: Vec<PassageNode> = Vec::new();
        for (id, pt) in &file.passages {
            
let (body, inline_diags) = if pt.body.trim().is_empty() {
                (Vec::new(), Vec::new())
            } else {
                crate::inline::parse_inline(&pt.body)
            };
            diagnostics.extend(inline_diags);
            let scripts = scripts_map.remove(id).unwrap_or_default();
            let hooks = hooks_map.remove(id).unwrap_or_default();

            nodes.push(PassageNode {
                id: id.clone(),
                tags: pt.tags.clone(),
                body,
                scripts,
                hooks,
            });
        }

        // Check for orphan scripts/hooks
        for passage_id in scripts_map.keys() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "scripts defined for nonexistent passage: {passage_id}"
                ),
                span: None,
            });
        }
        for passage_id in hooks_map.keys() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "hooks defined for nonexistent passage: {passage_id}"
                ),
                span: None,
            });
        }

        let mut graph = PassageGraph { nodes, edges: Vec::new() };
        graph.rebuild_edges();

        // Verify start passage exists
        if graph.get(&config.start).is_none() {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                message: format!(
                    "start passage '{}' not found in passages", config.start
                ),
                span: None,
            });
        }

        ParseOutput { config, graph, diagnostics }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_toml() {
        let src = r#"
[story]
title = "Test"
author = "Tester"
start = "intro"

[passages.intro]
body = "Hello world."
"#;
        let output = TomlParser.parse(src);
        assert_eq!(output.config.title, "Test");
        assert_eq!(output.config.start, "intro");
        assert_eq!(output.graph.nodes.len(), 1);
        assert_eq!(output.diagnostics.len(), 0);
    }

    #[test]
    fn parse_toml_with_hooks_and_scripts() {
        let src = r#"
[story]
title = "RPG"
author = "Dev"
start = "intro"

[passages.intro]
tags = ["start"]
body = "Welcome.\n\n[[Go → room]]"

[passages.room]
body = "A dark room."

[[scripts]]
passage = "intro"
lang = "rhai"
code = "state.set('hp', 100);"

[[hooks]]
passage = "intro"
event = "on_passage_start"
lang = "rhai"
code = "state.set('hp', 100);"
"#;
        let output = TomlParser.parse(src);
        assert_eq!(output.diagnostics.len(), 0);

        let intro = output.graph.get("intro").unwrap();
        assert_eq!(intro.tags, vec!["start"]);
        assert_eq!(intro.scripts.len(), 1);
        assert_eq!(intro.scripts[0].lang, "rhai");
        assert_eq!(intro.hooks.len(), 1);
        assert_eq!(intro.hooks[0].event, "on_passage_start");

        // Note: inline parsing (P3) is not yet implemented, so [[links]] in body
        // text are not extracted.  Once P3 is built, this test will assert edges.
    }

    #[test]
    fn toml_missing_start_passage() {
        let src = r#"
[story]
title = "Bad"
author = "X"
start = "nowhere"

[passages.intro]
body = "Hello."
"#;
        let output = TomlParser.parse(src);
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn toml_orphan_script_warning() {
        let src = r#"
[story]
title = "Test"
author = "A"
start = "intro"

[passages.intro]
body = "Hi."

[[scripts]]
passage = "ghost"
lang = "rhai"
code = "// nobody here"
"#;
        let output = TomlParser.parse(src);
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].severity, Severity::Warning);
    }
}
