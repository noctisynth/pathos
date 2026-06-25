use serde::{Deserialize, Serialize};
use crate::config::PassageId;
use crate::value::Value;
use std::collections::HashMap;

/// A choice presented to the user (link to another passage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub target: PassageId,
    /// If false, the choice is displayed but greyed out / not clickable.
    pub enabled: bool,
    pub tooltip: Option<String>,
}

/// Render commands pushed from the engine to the render backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenderCommand {
    /// Clear the current output.
    Clear,
    /// Render a block of text.
    Text(String),
    /// Start streaming LLM output (backend should switch to streaming mode).
    StreamBegin,
    /// A single token from a streaming LLM response.
    StreamToken(String),
    /// Streaming has completed.
    StreamEnd,
    /// Streaming failed; the backend should render this fallback text directly.
    StreamFailed { fallback: String },
    /// Present a set of choices to the user.
    Choice(Vec<Choice>),
    /// Request text input from the user.
    Input { prompt: String, default: Option<String> },
    /// A horizontal rule / separator.
    Separator,
}

/// Signals produced by `NarrativeRuntime::step()`.  The render backend
/// acts on these to drive the UI.
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Normal render output — the backend should display these commands.
    Render(Vec<RenderCommand>),
    /// The engine is waiting for the user to pick a choice.
    WaitingForChoice,
    /// The engine is waiting for free-form text input.
    WaitingForInput { prompt: String, default: Option<String> },
    /// The engine is waiting for an LLM response.  The backend must call
    /// `pathos-llm` and then feed results back via `feed_stream_token()` /
    /// `end_stream()`.
    WaitingForStream {
        prompt: String,
        fallback: String,
        cache_key: Option<String>,
        context: LLMContext,
    },
    /// The narrative has ended (no more passages).
    Finished,
}

/// Context passed alongside an LLM request to help the model understand
/// the narrative position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMContext {
    pub passage_name: String,
    pub passage_tags: Vec<String>,
    pub story_title: String,
    /// Recent passage output text (token-truncated ring buffer).
    pub recent_text: Vec<String>,
    /// State variables marked as visible to the LLM.
    pub visible_state: HashMap<String, Value>,
}

/// User input enum — fed back into the engine after a pause.
#[derive(Debug, Clone)]
pub enum UserInput {
    /// The user chose a link target.
    Choice(usize),
    /// The user typed some text.
    Text(String),
}
