use pathos_parser::format::{FormatParser, ParseOutput};
use pathos_parser::PathosParser;

fn parse(src: &str) -> ParseOutput {
    PathosParser.parse(src)
}

// ── Full passages ─────────────────────────────────────────────────

#[test]
fn snapshot_minimal_pathos() {
    let src = "\
---
title: Test Story
author: Author
start: intro
---

# intro {opening}
Welcome to the story.

[[Go to room -> room]]

# room {safe}
You are in a room. The door is locked.
";
    insta::assert_debug_snapshot!(parse(src));
}

#[test]
fn snapshot_passage_with_tags_and_scripts() {
    let src = "\
---
title: Tagged
start: start
---

# start {opening, safe}
Before you stands a door.

```rhai
state.set(\"door_locked\", true);
```

# end
The end.
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Conditional blocks ────────────────────────────────────────────

#[test]
fn snapshot_if_else_end() {
    let src = "\
---
title: Conditional
start: p1
---

# p1
{if: visited(\"cave\")}
  You've been to the cave before.
{else}
  A dark opening looms ahead.
{end}
";
    insta::assert_debug_snapshot!(parse(src));
}

#[test]
fn snapshot_conditional_link() {
    let src = "\
---
title: CondLink
start: p1
---

# p1
[[Enter the void -> void {if: has_tag(\"brave\")}]]
[[Go back -> back]]

# void
It is dark.

# back
You retreat.
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Display embedding ─────────────────────────────────────────────

#[test]
fn snapshot_display_embed() {
    let src = "\
---
title: Display
start: main
---

# main
Hello. {display: sidebar}

# sidebar
This is a sidebar.
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── AI blocks ─────────────────────────────────────────────────────

#[test]
fn snapshot_ai_block() {
    let src = "\
---
title: AI
start: p1
---

# p1
{ai: Describe the room | You see a room.}
{ai-stream: Narrate the journey | The path winds on.}
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Hooks ─────────────────────────────────────────────────────────

#[test]
fn snapshot_hook_directive() {
    let src = "\
---
title: Hooks
start: p1
---

# p1
@hook: on_passage_start
```rhai
state.set(\"entered\", true);
```

Welcome!

# p2
@hook: on_passage_end
```rhai
state.set(\"left\", true);
```

Goodbye.
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Comments ──────────────────────────────────────────────────────

#[test]
fn snapshot_comments() {
    let src = "\
---
title: Comments
start: p1
---

# p1
visible text // this is a comment
<!-- block comment -->
after block
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Expression parsing ────────────────────────────────────────────

#[test]
fn snapshot_expression_forms() {
    let src = "\
---
title: Expr
start: p1
---

# p1
{if: visited(\"cave\") && !has_tag(\"scared\") && count(\"cave\") < 3}
  Welcome back.
{end}

[[Go deeper -> deep {if: random(1, 10) > 5}]]

# deep
The depths.
";
    insta::assert_debug_snapshot!(parse(src));
}

// ── Diagnostics / errors ──────────────────────────────────────────

#[test]
fn snapshot_start_passage_not_found() {
    let src = "\
---
title: BadStart
start: nowhere
---

# intro
Hello.
";
    insta::assert_debug_snapshot!(parse(src));
}

#[test]
fn snapshot_unknown_function_in_if() {
    let src = "\
---
title: UnknownFn
start: p1
---

# p1
{if: foobar(1)}
  yes
{end}
";
    insta::assert_debug_snapshot!(parse(src));
}

#[test]
fn snapshot_unused_passage_warning() {
    let src = "\
---
title: Unused
start: p1
---

# p1
Hello.

# orphan
Nobody links here.
";
    insta::assert_debug_snapshot!(parse(src));
}
