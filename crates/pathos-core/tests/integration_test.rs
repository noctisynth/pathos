//! Integration tests for Pathos — end-to-end story execution.
//!
//! Parser tests are wrapped in a timeout guard. Runtime/execution tests
//! run inline (non-Send types involved).

use pathos_core::{
    ContentNode, NarrativeRuntime, PassageEdge, StoryState,
};
use pathos_parser::{FormatParser, PathosParser, Severity};
use pathos_render::{MockBackend, RenderBackend as _};

// ── Timeout guard (for Send-safe parser tests only) ────────────────────

fn with_timeout<T: Send + 'static>(ms: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let handle = std::thread::spawn(f);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < deadline {
        if handle.is_finished() {
            return handle.join().unwrap_or_else(|e| {
                std::panic::resume_unwind(e);
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("test timed out after {ms}ms — possible infinite loop");
}

// ── Parser tests (with timeout guard) ──────────────────────────────────

#[test]
fn parse_and_verify_structure() {
    let source = "\
---
title: Test Story
author: Tester
start: intro
---

# intro {opening}
Welcome to the test.

[[Go to room \u{2192} room]]

---

# room
You are in a room.

[[Continue \u{2192} end]]

---

# end
The story ends here.
";
    let (config, graph) = with_timeout(10_000, move || {
        let output = PathosParser.parse(source);
        let errors: Vec<_> = output.diagnostics.iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        (output.config, output.graph)
    });
    assert_eq!(graph.nodes.len(), 3);

    let intro = graph.get("intro").expect("intro missing");
    assert!(intro.tags.contains(&"opening".to_string()));
    assert!(intro.body.iter().any(|n| matches!(n, ContentNode::Text(s) if s.contains("Welcome"))));
    assert!(intro.body.iter().any(|n| matches!(n, ContentNode::Link { label, target, .. }
        if label == "Go to room" && target == "room")));
    let _ = config;
}

#[test]
fn verify_edges_built() {
    let source = "\
---
title: Test Story
author: Tester
start: intro
---

# intro {opening}
Welcome.

[[Go \u{2192} room]]

---

# room
A room.

[[Go \u{2192} end]]

---

# end
End.
";
    let (_config, graph) = with_timeout(10_000, move || {
        let output = PathosParser.parse(source);
        assert!(output.diagnostics.iter().all(|d| d.severity != Severity::Error));
        (output.config, output.graph)
    });
    assert_eq!(graph.edges.len(), 2);
    assert!(graph.edges.iter().any(|e: &PassageEdge| e.from == "intro" && e.to == "room"));
    assert!(graph.edges.iter().any(|e: &PassageEdge| e.from == "room" && e.to == "end"));
}

#[test]
fn verify_hook_parsing() {
    let source = "\
---
title: Hook Test
author: Tester
start: intro
---

# intro
Welcome.

---

# room
@hook: on_passage_start
```rhai
state.set(\"hook_fired\", true);
```

You are in a room.
";
    let (_config, graph) = with_timeout(10_000, move || {
        let output = PathosParser.parse(source);
        assert!(output.diagnostics.iter().all(|d| d.severity != Severity::Error));
        (output.config, output.graph)
    });
    let room = graph.get("room").expect("room missing");
    assert_eq!(room.hooks.len(), 1);
    assert_eq!(room.hooks[0].event, "on_passage_start");
    assert_eq!(room.hooks[0].script.lang, "rhai");
    assert!(room.hooks[0].script.code.contains("hook_fired"));
}

#[test]
fn parser_diagnostics_orphan_link_warning() {
    let source = "\
---
title: Test
author: A
start: intro
---

# intro
[[Go \u{2192} nowhere]]
";
    let output = with_timeout(10_000, move || PathosParser.parse(source));
    let warnings: Vec<_> = output.diagnostics.iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(!warnings.is_empty(), "expected orphan link warning");
    assert!(warnings.iter().any(|d| d.message.contains("nowhere")));
}

#[test]
fn parser_diagnostics_missing_start() {
    let source = "\
---
title: Test
author: A
start: missing
---

# intro
Hello.
";
    let output = with_timeout(10_000, move || PathosParser.parse(source));
    let errors: Vec<_> = output.diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected missing start error");
}

// ── Runtime execution tests (inline — no timeout needed) ───────────────

#[test]
fn full_story_execution() {
    let source = "\
---
title: Execution Test
author: Tester
start: intro
---

# intro {opening}
Welcome to the test.

[[Go to room \u{2192} room]]

---

# room
You are in a room.

[[Continue \u{2192} end]]

---

# end
The story ends here.
";
    let output = PathosParser.parse(source);
    assert!(output.diagnostics.iter().all(|d| d.severity != Severity::Error));

    let (config, graph) = (output.config, output.graph);
    let state = StoryState::default();
    let mut runtime = NarrativeRuntime::new(config, graph, state);
    let mut backend = MockBackend::new();

    runtime.navigate_to("intro").unwrap();

    let result = runtime.step();
    match &result {
        pathos_core::StepResult::Render(cmds) => {
            backend.render(cmds.clone());
            let text = backend.text();
            assert!(text.contains("Welcome"), "expected Welcome, got: {text}");
        }
        other => panic!("expected Render, got {other:?}"),
    }
}
