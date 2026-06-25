use serde::{Deserialize, Serialize};
use crate::config::PassageId;
use crate::value::{Scope, Value};

/// The live narrative state — persists across passage navigations and is
/// serialized in save files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryState {
    /// Persistent variables (Global scope).  Serialized in save files.
    pub globals: Value,
    /// Temporary variables (Temp scope).  Cleared on next passage navigation.
    /// Not serialized in save files.
    #[serde(skip, default)]
    pub temp: Value,
    /// Story-level metadata from `.pathos` frontmatter (title, author, version,
    /// save_slots).  Scripts have read-only access.
    pub metadata: Value,
}

impl StoryState {
    /// Get a variable value by path (e.g. `player.inventory.0.name`).
    /// Checks Temp first, then Global, then metadata.  Returns `None` if not found.
    pub fn get(&self, path: &str, scope: Scope) -> Option<&Value> {
        match scope {
            Scope::Global => {
                // Try Temp first (shadows Global), then Global, then metadata.
                self.temp.get(path)
                    .or_else(|| self.globals.get(path))
                    .or_else(|| self.metadata.get(path))
            }
            Scope::Temp => self.temp.get(path),
        }
    }

    /// Set a variable value.  `scope` determines where the value is stored.
    /// Returns an error if the path targets metadata.
    pub fn set(&mut self, path: &str, val: Value, scope: Scope) -> Result<(), String> {
        // Writing to metadata is forbidden.
        if path == "metadata" || path.starts_with("metadata.") {
            return Err("cannot write to read-only metadata".into());
        }
        let target = match scope {
            Scope::Global => &mut self.globals,
            Scope::Temp => &mut self.temp,
        };
        target.set(path, val).ok_or_else(|| format!("invalid state path: {}", path))
    }

    /// Increment a numeric value by 1. Returns the new value.
    pub fn inc(&mut self, path: &str, scope: Scope) -> Result<i64, String> {
        let cur = self.get(path, scope)
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let new = cur + 1;
        self.set(path, Value::Int(new), scope)?;
        Ok(new)
    }

    /// Decrement a numeric value by 1. Returns the new value.
    pub fn dec(&mut self, path: &str, scope: Scope) -> Result<i64, String> {
        let cur = self.get(path, scope)
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let new = cur - 1;
        self.set(path, Value::Int(new), scope)?;
        Ok(new)
    }

    /// Check if a variable exists at the given path.
    pub fn has(&self, path: &str, scope: Scope) -> bool {
        self.get(path, scope).is_some()
    }

    /// Delete a top-level key from the globals object.
    /// Writing to metadata is forbidden.
    /// Returns false if the key was not present.
    pub fn delete(&mut self, path: &str, scope: Scope) -> Result<(), String> {
        if path == "metadata" || path.starts_with("metadata.") {
            return Err("cannot delete metadata".into());
        }
        let target = match scope {
            Scope::Global => &mut self.globals,
            Scope::Temp => &mut self.temp,
        };
        match target {
            Value::Object(map) => { map.remove(path); Ok(()) }
            _ => Err(format!("cannot delete from non-object at path: {}", path)),
        }
    }

    /// Clear all Temp-scoped variables (called on passage navigation).
    pub fn clear_temp(&mut self) {
        self.temp = Value::Null;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── get / set ───────────────────────────────────────────────────

    #[test]
    fn set_and_get_global() {
        let mut s = StoryState::default();
        s.set("hp", Value::Int(100), Scope::Global).unwrap();
        assert_eq!(s.get("hp", Scope::Global).unwrap().as_int(), Some(100));
    }

    #[test]
    fn set_and_get_temp() {
        let mut s = StoryState::default();
        s.set("x", Value::String("hello".into()), Scope::Temp).unwrap();
        assert_eq!(s.get("x", Scope::Temp).unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn temp_not_visible_from_global() {
        let mut s = StoryState::default();
        s.set("x", Value::Int(1), Scope::Temp).unwrap();
        // Global get checks Temp first
        assert_eq!(s.get("x", Scope::Global).unwrap().as_int(), Some(1));
    }

    #[test]
    fn global_scoped_get_skips_temp() {
        let mut s = StoryState::default();
        s.set("x", Value::Int(1), Scope::Temp).unwrap();
        s.set("x", Value::Int(2), Scope::Global).unwrap();
        assert_eq!(s.get("x", Scope::Temp).unwrap().as_int(), Some(1));
    }

    // ── metadata write protection ────────────────────────────────────

    #[test]
    fn cannot_write_metadata() {
        let mut s = StoryState::default();
        assert!(s.set("metadata", Value::Null, Scope::Global).is_err());
        assert!(s.set("metadata.foo", Value::Null, Scope::Global).is_err());
    }

    #[test]
    fn cannot_delete_metadata() {
        let mut s = StoryState::default();
        assert!(s.delete("metadata", Scope::Global).is_err());
    }

    // ── inc / dec ───────────────────────────────────────────────────

    #[test]
    fn inc_from_zero() {
        let mut s = StoryState::default();
        let v = s.inc("count", Scope::Global).unwrap();
        assert_eq!(v, 1);
        assert_eq!(s.get("count", Scope::Global).unwrap().as_int(), Some(1));
    }

    #[test]
    fn inc_existing() {
        let mut s = StoryState::default();
        s.set("hp", Value::Int(10), Scope::Global).unwrap();
        let v = s.inc("hp", Scope::Global).unwrap();
        assert_eq!(v, 11);
    }

    #[test]
    fn dec_existing() {
        let mut s = StoryState::default();
        s.set("hp", Value::Int(10), Scope::Global).unwrap();
        let v = s.dec("hp", Scope::Global).unwrap();
        assert_eq!(v, 9);
    }

    // ── has / delete / clear_temp ───────────────────────────────────

    #[test]
    fn has_key() {
        let mut s = StoryState::default();
        assert!(!s.has("hp", Scope::Global));
        s.set("hp", Value::Int(10), Scope::Global).unwrap();
        assert!(s.has("hp", Scope::Global));
    }

    #[test]
    fn delete_key() {
        let mut s = StoryState::default();
        s.set("hp", Value::Int(10), Scope::Global).unwrap();
        assert!(s.has("hp", Scope::Global));
        s.delete("hp", Scope::Global).unwrap();
        assert!(!s.has("hp", Scope::Global));
    }

    #[test]
    fn clear_temp_removes_all() {
        let mut s = StoryState::default();
        s.set("a", Value::Int(1), Scope::Temp).unwrap();
        s.set("b", Value::Int(2), Scope::Temp).unwrap();
        s.clear_temp();
        assert!(!s.has("a", Scope::Temp));
        assert!(!s.has("b", Scope::Temp));
    }

    #[test]
    fn clear_temp_leaves_global_intact() {
        let mut s = StoryState::default();
        s.set("a", Value::Int(1), Scope::Global).unwrap();
        s.set("b", Value::Int(2), Scope::Temp).unwrap();
        s.clear_temp();
        assert!(s.has("a", Scope::Global));
    }
}

/// A single state mutation recorded for undo/checkpoint purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub path: String,
    pub old: Option<Value>,
    pub new: Value,
    pub scope: Scope,
}

/// A checkpoint snapshot of the full game state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// MessagePack-serialized full StoryState.
    pub full_state: Vec<u8>,
    pub passage: PassageId,
    pub turn: u64,
}
impl Default for StoryState {
    fn default() -> Self {
        Self {
            globals: Value::Object(std::collections::HashMap::new()),
            temp: Value::Object(std::collections::HashMap::new()),
            metadata: Value::Null,
        }
    }
}
