use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The fundamental value type for all engine state.
///
/// `Float` wraps `OrderedFloat<f64>` to forbid NaN/Infinity (they are rejected
/// at construction and serialization time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(OrderedFloat<f64>),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

impl Value {
    /// Create a `Float`, returning `None` if the value is NaN or Infinity.
    pub fn float(f: f64) -> Option<Self> {
        if f.is_nan() || f.is_infinite() {
            None
        } else {
            Some(Value::Float(OrderedFloat(f)))
        }
    }

    /// Access a nested value by a "path" such as `player.inventory.0.name`.
    pub fn get(&self, path: &str) -> Option<&Value> {
        if path.is_empty() {
            return Some(self);
        }
        let mut cur = self;
        for segment in path.split('.') {
            cur = match cur {
                Value::Object(map) => map.get(segment)?,
                Value::Array(arr) => {
                    let idx: usize = segment.parse().ok()?;
                    arr.get(idx)?
                }
                _ => return None,
            };
        }
        Some(cur)
    }

    /// Set a nested value (creates intermediate Object/Array nodes as needed).
    /// Returns `None` if numeric index is negative or intermediate is the wrong type.
    pub fn set(&mut self, path: &str, val: Value) -> Option<()> {
        if path.is_empty() {
            *self = val;
            return Some(());
        }
        let segments: Vec<&str> = path.split('.').collect();
        let mut cur = self;
        for segment in &segments[..segments.len() - 1] {
            cur = cur.as_mut_nested(segment)?;
        }
        let last_seg = segments.last()?;
        match cur {
            Value::Object(map) => {
                map.insert(last_seg.to_string(), val);
                Some(())
            }
            Value::Array(arr) => {
                let idx: usize = last_seg.parse().ok()?;
                if idx < arr.len() {
                    arr[idx] = val;
                } else if idx == arr.len() {
                    arr.push(val);
                } else {
                    return None; // gap in array
                }
                Some(())
            }
            _ => None,
        }
    }

    fn as_mut_nested(&mut self, segment: &str) -> Option<&mut Self> {
        match self {
            Value::Object(map) => {
                if !map.contains_key(segment) {
                    map.insert(segment.to_string(), Value::Object(HashMap::new()));
                }
                map.get_mut(segment)
            }
            Value::Array(arr) => {
                let idx: usize = segment.parse().ok()?;
                if idx < arr.len() {
                    Some(&mut arr[idx])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Convenience: interpret this value as an `i64`.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Convenience: interpret this value as a `f64`.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(f.into_inner()),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Convenience: interpret this value as a `bool`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Convenience: interpret this value as a `&str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Value::float ─────────────────────────────────────────────────

    #[test]
    fn float_rejects_nan() {
        assert!(Value::float(f64::NAN).is_none());
    }

    #[test]
    fn float_rejects_infinity() {
        assert!(Value::float(f64::INFINITY).is_none());
        assert!(Value::float(f64::NEG_INFINITY).is_none());
    }

    #[test]
    fn float_accepts_normal() {
        let v = Value::float(3.14).unwrap();
        assert!((v.as_float().unwrap() - 3.14).abs() < 0.001);
    }

    // ── set / get path ───────────────────────────────────────────────

    #[test]
    fn set_and_get_top_level() {
        let mut obj = Value::Object(HashMap::new());
        obj.set("x", Value::Int(42)).unwrap();
        assert_eq!(obj.get("x").unwrap().as_int(), Some(42));
    }

    #[test]
    fn set_and_get_nested() {
        let mut obj = Value::Object(HashMap::new());
        obj.set("player.hp", Value::Int(100)).unwrap();
        assert_eq!(obj.get("player.hp").unwrap().as_int(), Some(100));
    }

    #[test]
    fn set_and_get_deeply_nested() {
        let mut obj = Value::Object(HashMap::new());
        obj.set("a.b.c", Value::String("deep".into())).unwrap();
        assert_eq!(obj.get("a.b.c").unwrap().as_str(), Some("deep"));
    }

    #[test]
    fn get_nonexistent_path() {
        let obj = Value::Object(HashMap::new());
        assert!(obj.get("foo").is_none());
    }

    // ── comparison ───────────────────────────────────────────────────

    #[test]
    fn eq_same_type() {
        assert_eq!(Value::Int(1), Value::Int(1));
        assert_ne!(Value::Int(1), Value::Int(2));
        assert_eq!(Value::Bool(true), Value::Bool(true));
        assert_eq!(Value::String("a".into()), Value::String("a".into()));
    }

    #[test]
    fn eq_different_types() {
        assert_ne!(Value::Int(0), Value::Bool(false));
        assert_ne!(Value::Null, Value::String("null".into()));
    }

    // ── as_* conversions ─────────────────────────────────────────────

    #[test]
    fn as_int_from_float() {
        assert_eq!(Value::Float(OrderedFloat(3.0)).as_int(), None);
    }

    #[test]
    fn as_float_from_int() {
        assert_eq!(Value::Int(5).as_float(), Some(5.0));
    }

    #[test]
    fn as_bool() {
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Int(0).as_bool(), None);
    }

    // ── Display ──────────────────────────────────────────────────────

    #[test]
    fn display_null() {
        assert_eq!(Value::Null.to_string(), "null");
    }

    #[test]
    fn display_int() {
        assert_eq!(Value::Int(42).to_string(), "42");
    }

    #[test]
    fn display_string() {
        assert_eq!(Value::String("hi".into()).to_string(), "hi");
    }

    #[test]
    fn display_array() {
        let arr = Value::Array(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(arr.to_string(), "[1, 2]");
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(v) => write!(f, "{}", v.into_inner()),
            Value::String(s) => write!(f, "{}", s),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Object(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// Variable scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// Persists across passages; serialized in save files.
    Global,
    /// Cleared on the next passage navigation.
    Temp,
}
