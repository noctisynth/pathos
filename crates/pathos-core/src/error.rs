use thiserror::Error;

/// Core errors for the Pathos engine.
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("passage not found: {0}")]
    PassageNotFound(String),

    #[error("state key not found: {0}")]
    StateKeyNotFound(String),

    #[error("invalid state path: {0}")]
    InvalidStatePath(String),

    #[error("script error: {0}")]
    Script(String),

    #[error("expression error: {0}")]
    Expression(String),

    #[error("macro error: {0}")]
    Macro(String),

    #[error("hook error: {0}")]
    Hook(String),

    #[error("state value contains NaN or Infinity, which are not allowed")]
    InvalidFloatState,

    #[error(transparent)]
    Rhai(#[from] Box<rhai::EvalAltResult>),
}

pub type CoreResult<T> = std::result::Result<T, CoreError>;
