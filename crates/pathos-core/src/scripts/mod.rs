//! Multi-language scripting subsystem.
//!
//! `ScriptLang` and `ScriptEngine` are the public entry points; per-language
//! implementations live in sibling modules (`rhai`, future `js`, `lua`).

use crate::error::CoreResult;
use crate::state::StoryState;
use crate::value::Value;

mod rhai_backend;

/// The language tag of a script block, extracted from fenced code block info
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLang {
    Rhai,
    #[cfg(feature = "script-js")]
    Js,
    #[cfg(feature = "script-lua")]
    Lua,
}

impl ScriptLang {
    /// Parse a language tag from an info string (e.g. `"rhai"`, `"js"`).
    pub fn from_info_string(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "rhai" => Some(ScriptLang::Rhai),
            #[cfg(feature = "script-js")]
            "js" | "javascript" => Some(ScriptLang::Js),
            #[cfg(feature = "script-lua")]
            "lua" => Some(ScriptLang::Lua),
            _ => None,
        }
    }

    /// The canonical info string for this language.
    pub fn as_str(&self) -> &str {
        match self {
            ScriptLang::Rhai => "rhai",
            #[cfg(feature = "script-js")]
            ScriptLang::Js => "js",
            #[cfg(feature = "script-lua")]
            ScriptLang::Lua => "lua",
        }
    }
}

/// Signal emitted by a script when it requests an immediate control-flow
/// change (navigation, restart).  `None` means the script completed normally.
///
/// When a script calls `state.game_goto("battle")`, execution stops
/// immediately and `eval` returns `Ok(ScriptSignal::Goto("battle"))`.
#[derive(Debug, Clone, Default)]
pub enum ScriptSignal {
    /// Script completed without requesting navigation.
    #[default]
    None,
    /// Script requested navigation to `passage_id`.
    Goto(String),
    /// Script requested a story restart.
    Restart,
}

impl ScriptSignal {
    /// True if this signal represents a navigation request.
    pub fn is_navigation(&self) -> bool {
        !matches!(self, ScriptSignal::None)
    }
}

/// Multi-language script engine.  Rhai is always compiled; JS and Lua are
/// behind feature gates.  All languages are treated as peer variants in the
/// enum.
#[allow(clippy::large_enum_variant)]
pub enum ScriptEngine {
    Rhai(rhai::Engine),
    #[cfg(feature = "script-js")]
    Js(boa_engine::Context),
    #[cfg(feature = "script-lua")]
    Lua(mlua::Lua),
}

impl ScriptEngine {
    /// Create a new Rhai-based script engine with the full Pathos built-in
    /// API registered.
    pub fn new_rhai() -> Self {
        ScriptEngine::Rhai(rhai_backend::new_engine())
    }

    /// Evaluate a script block against the current story state.
    ///
    /// `code` is the script source; `lang_tag` is the info string from the
    /// fenced code block (e.g. "rhai", "js").
    ///
    /// Returns `(result_value, signal)` where `signal` captures any
    /// `game.goto` / `game.restart` requests.  When a signal is present
    /// the script was interrupted and `result_value` is `Value::Null`.
    pub fn eval(
        &self,
        state: &mut StoryState,
        code: &str,
        lang_tag: &str,
    ) -> CoreResult<(Value, ScriptSignal)> {
        let lang = ScriptLang::from_info_string(lang_tag).ok_or_else(|| {
            crate::error::CoreError::Script(format!("unknown script language: {}", lang_tag))
        })?;
        match (self, lang) {
            (ScriptEngine::Rhai(engine), ScriptLang::Rhai) => {
                rhai_backend::eval(engine, state, code).map(|(v, s)| (v, s))
            }
            #[cfg(feature = "script-js")]
            (ScriptEngine::Js(_ctx), ScriptLang::Js) => {
                let _ = state;
                let _ = code;
                Ok((Value::Null, ScriptSignal::None))
            }
            #[cfg(feature = "script-lua")]
            (ScriptEngine::Lua(_lua), ScriptLang::Lua) => {
                let _ = state;
                let _ = code;
                Ok((Value::Null, ScriptSignal::None))
            }
        }
    }
}

impl std::fmt::Debug for ScriptEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptEngine::Rhai(_) => f.debug_tuple("Rhai").finish(),
            #[cfg(feature = "script-js")]
            ScriptEngine::Js(_) => f.debug_tuple("Js").finish(),
            #[cfg(feature = "script-lua")]
            ScriptEngine::Lua(_) => f.debug_tuple("Lua").finish(),
        }
    }
}
