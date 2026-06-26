//! Pathos — Interactive narrative engine core.
//!
//! This crate defines all core types, the state machine, passage graph,
//! hook system, script engine, macro system, and the narrative execution loop.
//! It has zero I/O dependencies — no filesystem, no network, no async runtime.

pub mod config;
pub mod content;
pub mod error;
pub mod expression;
pub mod hook;
pub mod macros;
pub mod passage;
pub mod runtime;
pub mod scripts;
pub mod state;
pub mod types;
pub mod value;

// Re-export key types for convenience
pub use config::{PassageId, StoryConfig};
pub use content::{AIMode, ContentNode, MacroArg};
pub use error::{CoreError, CoreResult};
pub use expression::Expression;
pub use hook::HookRegistry;
pub use macros::{MacroHandler, MacroContext, MacroResult, builtin_macros};
pub use passage::{EdgeKind, HookDeclaration, PassageEdge, PassageGraph, PassageNode, PassageScript};
pub use runtime::NarrativeRuntime;
pub use scripts::{ScriptEngine, ScriptLang, ScriptSignal};
pub use state::{StateChange, StateSnapshot, StoryState};
pub use types::{Choice, LLMContext, RenderCommand, StepResult, UserInput};
pub use value::{Scope, Value};
