use std::collections::HashMap;
use crate::config::{PassageId, StoryConfig};
use crate::content::ContentNode;
use crate::error::CoreResult;
use crate::expression::EvalContext;
use crate::hook::HookRegistry;
use crate::macros::{MacroHandler, MacroContext, MacroResult, builtin_macros};
use crate::passage::PassageGraph;
use crate::scripts::{ScriptEngine, ScriptSignal};
use crate::state::StoryState;
use crate::types::{Choice, RenderCommand, StepResult, UserInput};
use crate::value::{Scope, Value};

/// The main narrative runtime.
///
/// Holds all engine state and drives the step-by-step execution loop.
/// The render backend calls `step()` in a loop, consumes `StepResult`
/// signals, and feeds user input back via `submit_input()` / `feed_stream_token()`.
pub struct NarrativeRuntime {
    pub config: StoryConfig,
    pub graph: PassageGraph,
    pub state: StoryState,
    pub script: ScriptEngine,
    pub hooks: HookRegistry,
    pub macros: Vec<Box<dyn MacroHandler>>,
    pub turn: u64,

    // ── internal book-keeping ──────────────────────────────────────────────
    current_passage: Option<PassageId>,
    /// Choices collected from the current passage (waiting for user).
    pending_choices: Vec<Choice>,
    /// Buffered render commands produced during the current step.
    buffer: Vec<RenderCommand>,
    /// If we're in the middle of streaming, this holds the AI fallback text.
    ai_fallback: Option<String>,
    /// Passage history for the LLM context (ring buffer of recent passage texts).
    recent_passage_texts: Vec<String>,
}

impl NarrativeRuntime {
    /// Create a new runtime from config, graph, and state.
    ///
    /// Hooks are auto-registered from the graph's `@hook:` declarations.
    pub fn new(config: StoryConfig, graph: PassageGraph, state: StoryState) -> Self {
        let mut hooks = HookRegistry::default();
        for node in &graph.nodes {
            hooks.register_all(&node.hooks);
        }
        let script = ScriptEngine::new_rhai();

        // Populate metadata from config into state
        let mut state = state;
        let mut m = HashMap::new();
        m.insert("title".into(), Value::String(config.title.clone()));
        m.insert("author".into(), Value::String(config.author.clone()));
        m.insert("version".into(), Value::String(config.version.clone()));
        m.insert("save_slots".into(), Value::Int(config.save_slots as i64));
        state.metadata = Value::Object(m);

        Self {
            config,
            graph,
            state,
            script,
            hooks,
            macros: builtin_macros(),
            turn: 0,
            current_passage: None,
            pending_choices: Vec::new(),
            buffer: Vec::new(),
            ai_fallback: None,
            recent_passage_texts: Vec::new(),
        }
    }

    /// Navigate to a passage and execute it.
    ///
    /// Returns the first `ScriptSignal` emitted by hooks or scripts during
    /// the navigation lifecycle.  `ScriptSignal::None` means execution
    /// completed without interruption.
    pub fn navigate_to(&mut self, target: &str) -> CoreResult<ScriptSignal> {
        // Validate target exists
        if self.graph.get(target).is_none() {
            return Err(crate::error::CoreError::PassageNotFound(target.into()));
        }

        // Clear temp variables from previous passage
        self.state.clear_temp();

        // 1. on_passage_end (previous passage)
        if self.current_passage.is_some() {
            let sig = self.hooks.trigger("on_passage_end", &mut self.state, &self.script)?;
            if sig.is_navigation() { return Ok(sig); }
        }

        self.current_passage = Some(target.to_string());
        self.state.current_passage = Some(target.to_string());
        self.state.mark_visited(&target.to_string());
        self.pending_choices.clear();
        self.buffer.clear();
        self.ai_fallback = None;

        // 2. on_passage_start
        let sig = self.hooks.trigger("on_passage_start", &mut self.state, &self.script)?;
        if sig.is_navigation() { return Ok(sig); }

        // 3. Passage-level scripts
        let scripts: Vec<_> = self.graph.get(target)
            .map(|n| n.scripts.clone())
            .unwrap_or_default();
        for s in &scripts {
            let (_value, sig) = self.script.eval(&mut self.state, &s.code, &s.lang)?;
            if sig.is_navigation() { return Ok(sig); }
        }

        // 4. on_passage_render
        let sig = self.hooks.trigger("on_passage_render", &mut self.state, &self.script)?;
        if sig.is_navigation() { return Ok(sig); }

        Ok(ScriptSignal::None)
    }

    /// Execute one step and return the signal for the backend.
    pub fn step(&mut self) -> StepResult {
        self.buffer.clear();

        let passage_id = match self.current_passage.clone() {
            Some(id) => id,
            None => {
                // No passage loaded yet — navigate to start
                self.run_step(&self.config.start.clone());
                return self.step();
            }
        };

        // Clone the data we need to avoid holding an immutable borrow on self.graph
        // while we mutate self through walk_content.
        let (body, _tags, _hooks) = match self.graph.get(&passage_id) {
            Some(n) => (n.body.clone(), n.tags.clone(), n.hooks.clone()),
            None => {
                self.buffer.push(RenderCommand::Text(format!(
                    "Passage not found: {}", passage_id
                )));
                return StepResult::Render(std::mem::take(&mut self.buffer));
            }
        };

        // Sync state.current_passage for expression evaluator (has_tag, etc.)
        self.state.current_passage = self.current_passage.clone();

        // Walk the content AST and produce render commands + collect choices
        self.walk_content(&body);

        // Collect the passage's rendered text for LLM context
        let passage_text: String = self.buffer.iter().filter_map(|cmd| {
            match cmd {
                RenderCommand::Text(s) => Some(s.as_str()),
                _ => None,
            }
        }).collect::<Vec<_>>().join("\n");
        if !passage_text.is_empty() {
            self.recent_passage_texts.push(passage_text);
            if self.recent_passage_texts.len() > 20 {
                self.recent_passage_texts.remove(0);
            }
        }

        // Emit choices if we have any
        if !self.pending_choices.is_empty() {
            let choices = self.pending_choices.clone(); // keep originals for submit_input lookup
            self.buffer.push(RenderCommand::Separator);
            self.buffer.push(RenderCommand::Choice(choices));
            return StepResult::Render(std::mem::take(&mut self.buffer));
        }

        // No choices → narrative is finished
        StepResult::Finished
    }

    /// Drive a navigation to `target` and recursively follow any
    /// `ScriptSignal::Goto` emitted during the lifecycle hooks.
    /// Errors are buffered as render commands and surfaced on the next `step()`.
    fn run_step(&mut self, target: &str) {
        match self.navigate_to(target) {
            Ok(ScriptSignal::Goto(next)) => return self.run_step(&next),
            Err(e) => {
                self.buffer.clear();
                self.buffer.push(RenderCommand::Text(format!("Error: {e}")));
                return;
            }
            _ => {}
        }
        // Reached a stable passage — step() will pick up the render next.
    }

    /// Feed a user input back into the engine after a pause.
    pub fn submit_input(&mut self, input: UserInput) {
        match input {
            UserInput::Choice(idx) => {
                if let Some(choice) = self.pending_choices.get(idx) {
                    let target = choice.target.clone();
                    self.pending_choices.clear();
                    self.run_step(&target);
                }
            }
            UserInput::Text(text) => {
                let _ = self.state.set("_input", Value::String(text), Scope::Temp);
            }
        }
    }

    /// Feed a stream token from LLM response back into the engine.
    pub fn feed_stream_token(&mut self, token: String) {
        self.buffer.push(RenderCommand::StreamToken(token));
    }

    /// Signal that the LLM stream has ended (success or failure).
    /// On failure, the engine will push the fallback text.
    pub fn end_stream(&mut self, result: Result<(), String>) {
        if let Err(_err) = result {
            if let Some(fallback) = self.ai_fallback.take() {
                self.buffer.push(RenderCommand::StreamFailed { fallback });
            }
        } else {
            self.buffer.push(RenderCommand::StreamEnd);
        }
    }

    // ── internal helpers ───────────────────────────────────────────────────

    /// Walk a content node tree and produce RenderCommands.
    fn walk_content(&mut self, nodes: &[ContentNode]) {
        for cn in nodes {
            match cn {
                ContentNode::Text(s) => {
                    for para in s.split("\n\n") {
                        let trimmed = para.trim();
                        if !trimmed.is_empty() {
                            self.buffer.push(RenderCommand::Text(trimmed.to_string()));
                        }
                    }
                }
                ContentNode::Link { label, target, enabled_if } => {
                    let enabled = enabled_if
                        .as_ref()
                        .map(|expr| {
                            expr.eval(&EvalContext::with_graph(&self.state, &self.graph))
                                .ok()
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true)
                        })
                        .unwrap_or(true);
                    self.pending_choices.push(Choice {
                        label: label.clone(),
                        target: target.clone(),
                        enabled,
                        tooltip: None,
                    });
                }
                ContentNode::StateInterp { path } => {
                    if let Some(val) = self.state.get(path, Scope::Global) {
                        self.buffer.push(RenderCommand::Text(val.to_string()));
                    } else {
                        self.buffer.push(RenderCommand::Text(format!("{{state: \"{}\"}}", path)));
                    }
                }
                ContentNode::Conditional { condition, then_branch, else_branch } => {
                    let result = condition.eval(&EvalContext::with_graph(&self.state, &self.graph))
                        .ok()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if result {
                        self.walk_content(then_branch);
                    } else {
                        self.walk_content(else_branch);
                    }
                }
                ContentNode::Display { passage } => {
                    // Clone the body of the embedded passage to release graph borrow
                    let sub_body = self.graph.get(passage)
                        .map(|n| n.body.clone());
                    if let Some(body) = sub_body {
                        self.walk_content(&body);
                    }
                }
                ContentNode::AIBlock { fallback, .. } => {
                    // Phase 1: no LLM integration — render fallback directly.
                    self.buffer.push(RenderCommand::StreamFailed {
                        fallback: fallback.clone(),
                    });
                }
                ContentNode::Macro { name, args } => {
                    let handler_idx = self.macros.iter().position(|m| m.name() == name);
                    if let Some(idx) = handler_idx {
                        // Safety: we borrow self.macros immutably, then call handle with &mut self.state
                        // All macros in the vec are behind Arc<dyn MacroHandler>
                        let macro_list = std::mem::take(&mut self.macros);
                        let result = {
                            let h = &macro_list[idx];
                            let mut ctx = MacroContext {
                                state: &mut self.state,
                                script: &self.script,
                                args,
                            };
                            h.handle(&mut ctx)
                        };
                        self.macros = macro_list;
                        match result {
                            Ok(MacroResult::Content(nodes)) => {
                                for node in &nodes {
                                    match node {
                                        ContentNode::Text(s) => {
                                            self.buffer.push(RenderCommand::Text(s.clone()));
                                        }
                                        ContentNode::Display { passage: p } => {
                                            let sub_body = self.graph.get(p)
                                                .map(|n| n.body.clone());
                                            if let Some(body) = sub_body {
                                                self.walk_content(&body);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(MacroResult::SideEffect) => {}
                            Err(e) => {
                                self.buffer.push(RenderCommand::Text(
                                    format!("[macro error: {}]", e)
                                ));
                            }
                        }
                    } else {
                        self.buffer.push(RenderCommand::Text(
                            format!("{{{}: ...}}", name)
                        ));
                    }
                }
            }
        }
    }
}
