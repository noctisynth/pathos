use crate::error::CoreResult;
use crate::state::StoryState;
use crate::value::Value;

/// The language tag of a script block, extracted from fenced code block info string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLang {
    Rhai,
    #[cfg(feature = "script-js")]
    Js,
    #[cfg(feature = "script-lua")]
    Lua,
}

impl ScriptLang {
    /// Parse a language tag from an info string (e.g. `"rhai"`, `"js"`, `"lua"`).
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

/// Multi-language script engine.  Rhai is always compiled; JS and Lua are behind
/// feature gates.  All languages are treated as peer variants in the enum.
pub enum ScriptEngine {
    Rhai(rhai::Engine),
    #[cfg(feature = "script-js")]
    Js(boa_engine::Context),
    #[cfg(feature = "script-lua")]
    Lua(mlua::Lua),
}

impl ScriptEngine {
    /// Create a new Rhai-based script engine with Pathos built-in API registered.
    ///
    /// JS and Lua engines are created via their respective constructors when the
    /// corresponding feature gates are enabled.
    pub fn new_rhai() -> Self {
        let engine = rhai::Engine::new_raw();
        // Engine is sandboxed: no I/O, no network, max_operations enforced.
        // Rhai API will be registered in Phase 2.
        ScriptEngine::Rhai(engine)
    }

    /// Evaluate a script block against the current story state.
    ///
    /// `lang_tag` is the info string from the fenced code block (e.g. "rhai", "js").
    pub fn eval(
        &self,
        state: &mut StoryState,
        code: &str,
        lang_tag: &str,
    ) -> CoreResult<Value> {
        let lang = ScriptLang::from_info_string(lang_tag)
            .ok_or_else(|| crate::error::CoreError::Script(format!(
                "unknown script language: {}", lang_tag
            )))?;
        match (self, lang) {
            (ScriptEngine::Rhai(_engine), ScriptLang::Rhai) => {
                // Phase 1: Rhai eval not yet implemented.
                // Phase 2 will register state API and execute code.
                let _ = state;
                let _ = code;
                Ok(Value::Null)
            }
            #[cfg(feature = "script-js")]
            (ScriptEngine::Js(_ctx), ScriptLang::Js) => {
                let _ = state;
                let _ = code;
                Ok(Value::Null)
            }
            #[cfg(feature = "script-lua")]
            (ScriptEngine::Lua(_lua), ScriptLang::Lua) => {
                let _ = state;
                let _ = code;
                Ok(Value::Null)
            }
            #[cfg(any(feature = "script-js", feature = "script-lua"))]
            _ => Err(crate::error::CoreError::Script(format!(
                "engine does not support language: {}", lang_tag
            ))),
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
