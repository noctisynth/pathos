//! Rhai scripting engine integration.
//!
//! The Rhai engine is always compiled in (no feature gate).  This module
//! owns the `StateHandle` bridge type, the Value <-> Dynamic conversion
//! helpers, and the Rhai-specific `eval` implementation.
//!
//! ## Navigation interrupt protocol
//!
//! When a script calls `state.game_goto(...)` or `state.game_restart()`,
//! the handler stores the request in `StateHandle.signal` and immediately
//! returns a `NavInterrupt` error to stop script execution.  The `eval`
//! function catches this error and converts it back into a typed
//! `ScriptSignal` — no string matching, no continuation after navigation.

use crate::error::CoreResult;
use crate::value::Value;
use crate::state::StoryState;
use super::ScriptSignal;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

// ── Navigation interrupt marker ──────────────────────────────────────

/// Private marker type registered with Rhai.  When `eval_with_scope`
/// returns `Err(EvalAltResult::ErrorRuntime(d, ..))` and `d.is::<NavInterrupt>()`,
/// we know the script intentionally stopped for navigation — not a real error.
#[derive(Debug, Clone)]
struct NavInterrupt;

impl From<NavInterrupt> for Box<rhai::EvalAltResult> {
    fn from(_: NavInterrupt) -> Self {
        Box::new(rhai::EvalAltResult::ErrorRuntime(
            rhai::Dynamic::from(NavInterrupt),
            rhai::Position::NONE,
        ))
    }
}

/// Check whether a Rhai error is a navigation interrupt (not a user error).
fn is_nav_interrupt(err: &rhai::EvalAltResult) -> bool {
    matches!(
        err,
        rhai::EvalAltResult::ErrorRuntime(d, _) if d.is::<NavInterrupt>()
    )
}

// ── Value conversion helpers ──────────────────────────────────────────

fn value_to_dynamic(v: &Value) -> rhai::Dynamic {
    match v {
        Value::Null => rhai::Dynamic::UNIT,
        Value::Bool(b) => rhai::Dynamic::from(*b),
        Value::Int(i) => rhai::Dynamic::from(*i),
        Value::Float(f) => rhai::Dynamic::from(f.into_inner()),
        Value::String(s) => rhai::Dynamic::from(s.clone()),
        Value::Array(arr) => {
            let a: rhai::Array = arr.iter().map(value_to_dynamic).collect();
            rhai::Dynamic::from_array(a)
        }
        Value::Object(map) => {
            let m: rhai::Map = map
                .iter()
                .map(|(k, v)| (k.clone().into(), value_to_dynamic(v)))
                .collect();
            rhai::Dynamic::from_map(m)
        }
    }
}

fn dynamic_to_value(d: rhai::Dynamic) -> Value {
    if d.is_unit() {
        return Value::Null;
    }
    if d.is::<bool>() {
        return Value::Bool(d.clone().as_bool().unwrap_or(false));
    }
    if d.is::<i64>() {
        return Value::Int(d.clone().as_int().unwrap_or(0));
    }
    if d.is::<f64>() {
        let f = d.clone().as_float().unwrap_or(0.0);
        return Value::float(f).unwrap_or(Value::Null);
    }
    if d.is::<rhai::ImmutableString>() || d.is::<String>() {
        return Value::String(d.clone().into_string().unwrap_or_default());
    }
    if let Some(map) = d.clone().try_cast::<rhai::Map>() {
        let m: HashMap<String, Value> = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_value(v)))
            .collect();
        return Value::Object(m);
    }
    if let Some(arr) = d.clone().try_cast::<rhai::Array>() {
        let a: Vec<Value> = arr.into_iter().map(dynamic_to_value).collect();
        return Value::Array(a);
    }
    Value::String(d.to_string())
}

// ── StateHandle ────────────────────────────────────────────────────

/// Mutable bridge between Rhai scripts and `StoryState`.
///
/// Wraps `StoryState::globals` and `StoryState::temp` in `Rc<RefCell<...>>`
/// so Rhai method-call closures can reach them without violating the `'static`
/// constraint that Rhai imposes on registered function arguments.
///
/// The `signal` field uses `Cell` because it is written at most once per
/// `eval` call (by `game_goto` / `game_restart`) and read once afterward.
#[derive(Clone)]
struct StateHandle {
    globals: Rc<RefCell<Value>>,
    temp: Rc<RefCell<Value>>,
    signal: Rc<Cell<ScriptSignal>>,
}

impl StateHandle {
    // ── state.* ──────────────────────────────────────────────────

    fn get(&mut self, path: &str) -> rhai::Dynamic {
        let temp = self.temp.borrow();
        if let Some(v) = temp.get(path) {
            return value_to_dynamic(v);
        }
        let globals = self.globals.borrow();
        if let Some(v) = globals.get(path) {
            return value_to_dynamic(v);
        }
        rhai::Dynamic::UNIT
    }

    fn set(&mut self, path: &str, val: rhai::Dynamic) {
        let v = dynamic_to_value(val);
        self.globals.borrow_mut().set(path, v);
    }

    fn inc(&mut self, path: &str) -> rhai::Dynamic {
        let cur = self
            .globals
            .borrow()
            .get(path)
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let new = cur + 1;
        self.globals.borrow_mut().set(path, Value::Int(new));
        rhai::Dynamic::from(new)
    }

    fn dec(&mut self, path: &str) -> rhai::Dynamic {
        let cur = self
            .globals
            .borrow()
            .get(path)
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let new = cur - 1;
        self.globals.borrow_mut().set(path, Value::Int(new));
        rhai::Dynamic::from(new)
    }

    fn delete(&mut self, path: &str) {
        if let Value::Object(map) = &mut *self.globals.borrow_mut() {
            let key = path.split('.').next().unwrap_or(path);
            map.remove(key);
        }
    }

    // ── game.* (navigation) ──────────────────────────────────────

    /// `state.game_goto("battle")` — store signal + interrupt script.
    fn game_goto(&mut self, target: &str) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
        self.signal.set(ScriptSignal::Goto(target.to_string()));
        Err(NavInterrupt.into())
    }

    /// `state.game_restart()` — store signal + interrupt script.
    fn game_restart(&mut self) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
        self.signal.set(ScriptSignal::Restart);
        Err(NavInterrupt.into())
    }

    /// `state.game_visited("passage")` -> bool (stub).
    fn game_visited(&mut self, _target: &str) -> bool {
        false
    }

    /// `state.game_count()` -> i64 (stub).
    fn game_count(&mut self) -> i64 {
        0
    }
}

// ── Engine construction ────────────────────────────────────────────

pub(super) fn new_engine() -> rhai::Engine {
    let mut engine = rhai::Engine::new_raw();

    // Register NavInterrupt as a known type so Rhai can carry it in errors.
    engine.register_type::<NavInterrupt>();

    // ── Top-level utility functions ──────────────────────────────
    engine.register_fn("random", |min: i64, max: i64| -> i64 {
        if min >= max {
            return min;
        }
        let range = max - min;
        min + (((range as u64).wrapping_mul(17).wrapping_add(3)) % range as u64) as i64
    });
    engine.register_fn("random_float", || -> f64 { 0.5 });

    // ── StateHandle custom type + methods ────────────────────────
    engine.register_type_with_name::<StateHandle>("StateHandle");
    engine.register_fn("get", StateHandle::get);
    engine.register_fn("set", StateHandle::set);
    engine.register_fn("inc", StateHandle::inc);
    engine.register_fn("dec", StateHandle::dec);
    engine.register_fn("delete", StateHandle::delete);

    // game.* — navigation methods return Result so they can throw.
    engine.register_fn("game_goto", StateHandle::game_goto);
    engine.register_fn("game_restart", StateHandle::game_restart);
    engine.register_fn("game_visited", StateHandle::game_visited);
    engine.register_fn("game_count", StateHandle::game_count);

    engine
}

// ── eval entry point ──────────────────────────────────────────────

pub(super) fn eval(
    engine: &rhai::Engine,
    state: &mut StoryState,
    code: &str,
) -> CoreResult<(Value, ScriptSignal)> {
    let handle = StateHandle {
        globals: Rc::new(RefCell::new(state.globals.clone())),
        temp: Rc::new(RefCell::new(state.temp.clone())),
        signal: Rc::new(Cell::new(ScriptSignal::None)),
    };
    let mut scope = rhai::Scope::new();
    scope.push("state", handle.clone());

    let result = engine.eval_with_scope::<rhai::Dynamic>(&mut scope, code);

    // Write mutated state back into StoryState (always, even on interrupt).
    state.globals = handle.globals.borrow().clone();
    state.temp = handle.temp.borrow().clone();

    match result {
        // Navigation interrupt — typed signal, not a user error.
        Err(ref e) if is_nav_interrupt(e) => {
            let signal = handle.signal.take();
            Ok((Value::Null, signal))
        }
        // Real error.
        Err(e) => Err(crate::error::CoreError::Script(e.to_string())),
        // Normal completion.
        Ok(val) => Ok((dynamic_to_value(val), ScriptSignal::None)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ScriptEngine, ScriptSignal};
    use crate::state::StoryState;
    use crate::value::Value;

    // ── state.* tests ──────────────────────────────────────────────

    #[test]
    fn state_get_set_roundtrip() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();
        s.globals.set("hp", Value::Int(100));

        engine
            .eval(&mut s, r#"state.set("hp", state.get("hp") + 10);"#, "rhai")
            .unwrap();
        assert_eq!(s.globals.get("hp").and_then(Value::as_int), Some(110));
    }

    #[test]
    fn state_inc_dec() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();
        engine.eval(&mut s, r#"state.inc("count");"#, "rhai").unwrap();
        assert_eq!(s.globals.get("count").and_then(Value::as_int), Some(1));
        engine.eval(&mut s, r#"state.dec("count");"#, "rhai").unwrap();
        assert_eq!(s.globals.get("count").and_then(Value::as_int), Some(0));
    }

    #[test]
    fn state_delete() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();
        s.globals.set("hp", Value::Int(42));
        engine.eval(&mut s, r#"state.delete("hp");"#, "rhai").unwrap();
        assert!(s.globals.get("hp").is_none());
    }

    // ── navigation signal tests ────────────────────────────────────

    #[test]
    fn game_goto_emits_signal_and_interrupts() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();

        let (_result, signal) = engine
            .eval(
                &mut s,
                // Code after game_goto must NOT execute.
                r#"state.game_goto("battle"); state.set("hp", 999);"#,
                "rhai",
            )
            .unwrap();

        assert!(matches!(signal, ScriptSignal::Goto(ref t) if t == "battle"));
        // The `state.set("hp", 999)` after game_goto must not have run.
        assert!(s.globals.get("hp").is_none());
    }

    #[test]
    fn game_restart_emits_signal() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();

        let (_result, signal) = engine
            .eval(&mut s, r#"state.game_restart();"#, "rhai")
            .unwrap();

        assert!(matches!(signal, ScriptSignal::Restart));
    }

    #[test]
    fn normal_completion_emits_none() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();

        let (result, signal) = engine
            .eval(&mut s, r#"42"#, "rhai")
            .unwrap();

        assert!(matches!(signal, ScriptSignal::None));
        assert_eq!(result.as_int(), Some(42));
    }

    #[test]
    fn random_in_bounds() {
        let engine = ScriptEngine::new_rhai();
        let mut s = StoryState::default();
        let (result, _) = engine.eval(&mut s, "random(1, 10)", "rhai").unwrap();
        let n = result.as_int().unwrap();
        assert!(n >= 1 && n < 10);
    }
}
