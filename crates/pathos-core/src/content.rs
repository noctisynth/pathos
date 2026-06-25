use serde::{Deserialize, Serialize};
use crate::config::PassageId;
use crate::expression::Expression;
use crate::value::Value;

/// A node in the narrative content AST (output of parse pass P3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentNode {
    /// Static text
    Text(String),
    /// Passage link [[label → target]]
    Link {
        label: String,
        target: PassageId,
        /// If present, the link is only clickable when this evaluates to true.
        enabled_if: Option<Expression>,
    },
    /// State variable interpolation {state: "player.hp"}
    StateInterp {
        path: String,
    },
    /// Conditional block {if: expr} ... {else} ... {end}
    Conditional {
        condition: Expression,
        then_branch: Vec<ContentNode>,
        else_branch: Vec<ContentNode>,
    },
    /// Sub-passage embed {display: "passage_name"}
    Display {
        passage: PassageId,
    },
    /// AI generation block {ai: prompt | fallback}
    AIBlock {
        mode: AIMode,
        prompt: String,
        fallback: String,
        cache_key: Option<String>,
    },
    /// General macro call {name: args} — dispatched through MacroHandler
    Macro {
        name: String,
        args: Vec<MacroArg>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AIMode {
    /// Blocking AI generation
    Blocking,
    /// Streaming AI generation (tokens arrive progressively)
    Streaming,
    /// Cached AI generation (pre-generated at build time)
    Cached,
}

/// An argument to a macro call.  Simple key-value or positional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MacroArg {
    KeyValue(String, Value),
    Positional(Value),
}
