use crate::content::{ContentNode, MacroArg};
use crate::error::{CoreError, CoreResult};
use crate::script::ScriptEngine;
use crate::state::StoryState;
use crate::value::Value;

/// The result of executing a macro.
pub enum MacroResult {
    /// The macro produced content to be rendered.
    Content(Vec<ContentNode>),
    /// The macro performed a side effect (e.g. state mutation) with no render output.
    SideEffect,
}

/// Context passed to every `MacroHandler::handle()` invocation.
pub struct MacroContext<'a> {
    pub state: &'a mut StoryState,
    pub script: &'a ScriptEngine,
    pub args: &'a [MacroArg],
}

/// Unified trait for macro handlers — used by both built-in macros and
/// Mod-registered Rhai macros.
pub trait MacroHandler: Send + Sync {
    /// The macro name used in `{name: args}` syntax.
    fn name(&self) -> &str;
    /// Execute this macro.  Returns either content or a side effect.
    fn handle(&self, ctx: &mut MacroContext) -> CoreResult<MacroResult>;
}

// ── Built-in macros ────────────────────────────────────────────────────────

/// `{set: key = value}` — Set a state variable.
pub struct SetMacro;

impl MacroHandler for SetMacro {
    fn name(&self) -> &str { "set" }
    fn handle(&self, ctx: &mut MacroContext) -> CoreResult<MacroResult> {
        for arg in ctx.args {
            if let MacroArg::KeyValue(key, val) = arg {
                ctx.state.set(key, val.clone(), crate::value::Scope::Global)
                    .map_err(|e| CoreError::Macro(e))?;
            }
        }
        Ok(MacroResult::SideEffect)
    }
}

/// `{display: "passage_name"}` — Embed another passage's content.
pub struct DisplayMacro;

impl MacroHandler for DisplayMacro {
    fn name(&self) -> &str { "display" }
    fn handle(&self, ctx: &mut MacroContext) -> CoreResult<MacroResult> {
        let mut nodes = Vec::new();
        for arg in ctx.args {
            if let MacroArg::Positional(Value::String(name)) = arg {
                nodes.push(ContentNode::Display { passage: name.clone() });
            }
        }
        Ok(MacroResult::Content(nodes))
    }
}

/// `{print: "text"}` — Output static text.
pub struct PrintMacro;

impl MacroHandler for PrintMacro {
    fn name(&self) -> &str { "print" }
    fn handle(&self, ctx: &mut MacroContext) -> CoreResult<MacroResult> {
        let mut nodes = Vec::new();
        for arg in ctx.args {
            match arg {
                MacroArg::Positional(val) => {
                    nodes.push(ContentNode::Text(val.to_string()));
                }
                MacroArg::KeyValue(key, val) => {
                    nodes.push(ContentNode::Text(format!("{}: {}", key, val)));
                }
            }
        }
        Ok(MacroResult::Content(nodes))
    }
}

/// `{for: i in 0..N} ... {end}` — Iteration (stub for Phase 1).
pub struct ForMacro;

impl MacroHandler for ForMacro {
    fn name(&self) -> &str { "for" }
    fn handle(&self, _ctx: &mut MacroContext) -> CoreResult<MacroResult> {
        Ok(MacroResult::SideEffect)
    }
}

/// `{switch: expr} {case: val} ... {end}` — Multi-way branch (stub for Phase 1).
pub struct SwitchMacro;

impl MacroHandler for SwitchMacro {
    fn name(&self) -> &str { "switch" }
    fn handle(&self, _ctx: &mut MacroContext) -> CoreResult<MacroResult> {
        Ok(MacroResult::SideEffect)
    }
}

/// Create the default set of built-in macros.
pub fn builtin_macros() -> Vec<Box<dyn MacroHandler>> {
    vec![
        Box::new(SetMacro),
        Box::new(DisplayMacro),
        Box::new(PrintMacro),
        Box::new(ForMacro),
        Box::new(SwitchMacro),
    ]
}
