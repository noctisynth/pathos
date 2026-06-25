use serde::{Deserialize, Serialize};

/// Game configuration extracted from a `.pathos` frontmatter block.
///
/// LLM configuration (API key, provider, model) is NOT here — it is supplied
/// by the user at runtime (CLI flags, env vars, or Web settings panel).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoryConfig {
    pub title: String,
    pub author: String,
    /// Passage ID of the initial passage.
    pub start: PassageId,
    /// Semantic version string.
    pub version: String,
    /// Maximum number of save slots.
    pub save_slots: u8,
}

/// Uniquely identifies a passage.
pub type PassageId = String;

/// Identifies a hook event name.
pub type HookEvent = String;
