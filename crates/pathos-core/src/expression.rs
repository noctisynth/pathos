use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use crate::error::CoreResult;
use crate::passage::PassageGraph;
use crate::state::StoryState;
use crate::value::{Scope, Value};
/// Context passed to `Expression::eval`, bundling runtime state needed
/// for function calls like `has_tag()`, `visited()`, and `count()`.
#[derive(Debug, Clone)]
pub struct EvalContext<'a> {
    pub state: &'a StoryState,
    pub graph: Option<&'a PassageGraph>,
}

impl<'a> EvalContext<'a> {
    /// Create a minimal context with no passage graph (for tests / scripts).
    pub fn new(state: &'a StoryState) -> Self {
        Self { state, graph: None }
    }

    /// Create a context with a passage graph (for {if:} evaluation).
    pub fn with_graph(state: &'a StoryState, graph: &'a PassageGraph) -> Self {
        Self { state, graph: Some(graph) }
    }
}


/// A boolean or arithmetic expression (parsed from `{if: expr}` and script code).
///
/// The parser produces expression trees; the engine evaluates them against
/// the current `StoryState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    Literal(Value),
    /// State variable access: `state.get("path")` or the `$path` sugar.
    StateVar(String),
    Not(Box<Expression>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
    Eq(Box<Expression>, Box<Expression>),
    NotEq(Box<Expression>, Box<Expression>),
    Lt(Box<Expression>, Box<Expression>),
    Lte(Box<Expression>, Box<Expression>),
    Gt(Box<Expression>, Box<Expression>),
    Gte(Box<Expression>, Box<Expression>),
    Add(Box<Expression>, Box<Expression>),
    Sub(Box<Expression>, Box<Expression>),
    Mul(Box<Expression>, Box<Expression>),
    Div(Box<Expression>, Box<Expression>),
    /// Function call: `fn_name(arg1, arg2, ...)`
    Call {
        name: String,
        args: Vec<Expression>,
    },
}

impl Expression {
    /// Evaluate this expression against the current state.
    /// Evaluate this expression against the given runtime context.
    pub fn eval(&self, ctx: &EvalContext<'_>) -> CoreResult<Value> {
        match self {
            Expression::Literal(val) => Ok(val.clone()),
            Expression::StateVar(path) => {
                ctx.state
                    .get(path, Scope::Global)
                    .cloned()
                    .ok_or_else(|| {
                        crate::error::CoreError::Expression(format!(
                            "state variable not found: {}", path
                        ))
                    })
            }
            Expression::Not(inner) => {
                let v = inner.eval(ctx)?.as_bool().unwrap_or(false);
                Ok(Value::Bool(!v))
            }
            Expression::And(lhs, rhs) => {
                let a = lhs.eval(ctx)?.as_bool().unwrap_or(false);
                if !a {
                    return Ok(Value::Bool(false));
                }
                let b = rhs.eval(ctx)?.as_bool().unwrap_or(false);
                Ok(Value::Bool(b))
            }
            Expression::Or(lhs, rhs) => {
                let a = lhs.eval(ctx)?.as_bool().unwrap_or(false);
                if a {
                    return Ok(Value::Bool(true));
                }
                let b = rhs.eval(ctx)?.as_bool().unwrap_or(false);
                Ok(Value::Bool(b))
            }
            Expression::Eq(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                Ok(Value::Bool(a == b))
            }
            Expression::NotEq(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                Ok(Value::Bool(a != b))
            }
            Expression::Lt(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                Self::cmp_values(&a, &b, std::cmp::Ordering::Less)
            }
            Expression::Lte(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                match Self::cmp_values(&a, &b, std::cmp::Ordering::Less)? {
                    Value::Bool(true) => Ok(Value::Bool(true)),
                    _ => Self::cmp_values(&a, &b, std::cmp::Ordering::Equal),
                }
            }
            Expression::Gt(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                Self::cmp_values(&a, &b, std::cmp::Ordering::Greater)
            }
            Expression::Gte(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                match Self::cmp_values(&a, &b, std::cmp::Ordering::Greater)? {
                    Value::Bool(true) => Ok(Value::Bool(true)),
                    _ => Self::cmp_values(&a, &b, std::cmp::Ordering::Equal),
                }
            }
            Expression::Add(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
                    _ => {
                        let x = a.as_float().unwrap_or(0.0);
                        let y = b.as_float().unwrap_or(0.0);
                        Value::float(x + y).ok_or(crate::error::CoreError::InvalidFloatState)
                    }
                }
            }
            Expression::Sub(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
                    _ => {
                        let x = a.as_float().unwrap_or(0.0);
                        let y = b.as_float().unwrap_or(0.0);
                        Value::float(x - y).ok_or(crate::error::CoreError::InvalidFloatState)
                    }
                }
            }
            Expression::Mul(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
                    _ => {
                        let x = a.as_float().unwrap_or(0.0);
                        let y = b.as_float().unwrap_or(0.0);
                        Value::float(x * y).ok_or(crate::error::CoreError::InvalidFloatState)
                    }
                }
            }
            Expression::Div(lhs, rhs) => {
                let a = lhs.eval(ctx)?;
                let b = rhs.eval(ctx)?;
                let y = b.as_float().unwrap_or(0.0);
                if y == 0.0 {
                    return Err(crate::error::CoreError::Expression(
                        "division by zero".into()
                    ));
                }
                match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) if *y != 0 => Ok(Value::Int(x / y)),
                    _ => {
                        let x = a.as_float().unwrap_or(0.0);
                        Value::float(x / y).ok_or(crate::error::CoreError::InvalidFloatState)
                    }
                }
            }
            Expression::Call { name, args } => {
                match name.as_str() {
                    "random" => {
                        let low = args.first().and_then(|a| a.eval(ctx).ok()).and_then(|v| v.as_int()).unwrap_or(0);
                        let high = args.get(1).and_then(|a| a.eval(ctx).ok()).and_then(|v| v.as_int()).unwrap_or(1);
                        // Phase 1: deterministic midpoint (Phase 2 will use real RNG)
                        let result = if high > low { low + ((high - low) / 2) } else { low };
                        Ok(Value::Int(result))
                    }
                    "has_tag" => {
                        let tag = args.first()
                            .and_then(|a| a.eval(ctx).ok())
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        Ok(Value::Bool(ctx.state.has_tag(&tag, ctx.graph)))
                    }
                    "visited" => {
                        let passage = args.first()
                            .and_then(|a| a.eval(ctx).ok())
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        Ok(Value::Bool(ctx.state.is_visited(&passage)))
                    }
                    "count" => {
                        let passage = args.first()
                            .and_then(|a| a.eval(ctx).ok())
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        Ok(Value::Int(ctx.state.visit_count_of(&passage) as i64))
                    }
                    _ => Err(crate::error::CoreError::Expression(format!(
                        "unknown function: {}", name
                    ))),
                }
            }
        }
    }

    /// Compare two values and return `Bool(true)` if the ordering matches `expected`.
    fn cmp_values(a: &Value, b: &Value, expected: std::cmp::Ordering) -> CoreResult<Value> {
        let ordering = match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => Some(x.cmp(y)),
            (Value::Int(x), Value::Float(y)) => {
                let fx = OrderedFloat(*x as f64);
                Some(fx.cmp(y))
            }
            (Value::Float(x), Value::Int(y)) => {
                let fy = OrderedFloat(*y as f64);
                Some(x.cmp(&fy))
            }
            _ => None,
        };
        match ordering {
            Some(ord) => Ok(Value::Bool(ord == expected)),
            None => Err(crate::error::CoreError::Expression(
                "comparison between incompatible types".into()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passage::PassageGraph;
use crate::state::StoryState;
    use crate::value::{Scope, Value};

    fn state() -> StoryState {
        let mut s = StoryState::default();
        s.set("hp", Value::Int(10), Scope::Global).unwrap();
        s.set("name", Value::String("hero".into()), Scope::Global).unwrap();
        s
    }

    // ── literals ─────────────────────────────────────────────────────

    #[test]
    fn literal_int() {
        let e = Expression::Literal(Value::Int(42));
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(42));
    }

    #[test]
    fn literal_bool() {
        let e = Expression::Literal(Value::Bool(true));
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    // ── state var ────────────────────────────────────────────────────

    #[test]
    fn state_var() {
        let e = Expression::StateVar("hp".into());
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(10));
    }

    #[test]
    fn state_var_not_found() {
        let e = Expression::StateVar("nonexistent".into());
        assert!(e.eval(&EvalContext::new(&state())).is_err());
    }

    // ── boolean ops ──────────────────────────────────────────────────

    #[test]
    fn not_true() {
        let e = Expression::Not(Box::new(Expression::Literal(Value::Bool(true))));
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(false));
    }

    #[test]
    fn and_short_circuit() {
        let e = Expression::And(
            Box::new(Expression::Literal(Value::Bool(false))),
            Box::new(Expression::StateVar("nonexistent".into())),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(false));
    }

    #[test]
    fn or_short_circuit() {
        let e = Expression::Or(
            Box::new(Expression::Literal(Value::Bool(true))),
            Box::new(Expression::StateVar("nonexistent".into())),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    // ── arithmetic ───────────────────────────────────────────────────

    #[test]
    fn add_ints() {
        let e = Expression::Add(
            Box::new(Expression::Literal(Value::Int(2))),
            Box::new(Expression::Literal(Value::Int(3))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(5));
    }

    #[test]
    fn sub_ints() {
        let e = Expression::Sub(
            Box::new(Expression::Literal(Value::Int(10))),
            Box::new(Expression::Literal(Value::Int(3))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(7));
    }

    #[test]
    fn mul_ints() {
        let e = Expression::Mul(
            Box::new(Expression::Literal(Value::Int(4))),
            Box::new(Expression::Literal(Value::Int(5))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(20));
    }

    #[test]
    fn div_ints() {
        let e = Expression::Div(
            Box::new(Expression::Literal(Value::Int(10))),
            Box::new(Expression::Literal(Value::Int(2))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(5));
    }

    #[test]
    fn div_by_zero() {
        let e = Expression::Div(
            Box::new(Expression::Literal(Value::Int(1))),
            Box::new(Expression::Literal(Value::Int(0))),
        );
        assert!(e.eval(&EvalContext::new(&state())).is_err());
    }

    // ── mixed float/int arithmetic ───────────────────────────────────

    #[test]
    fn add_int_float() {
        let e = Expression::Add(
            Box::new(Expression::Literal(Value::Int(2))),
            Box::new(Expression::Literal(Value::float(3.5).unwrap())),
        );
        let v = e.eval(&EvalContext::new(&state())).unwrap();
        assert!((v.as_float().unwrap() - 5.5).abs() < 0.001);
    }

    // ── comparisons ──────────────────────────────────────────────────

    #[test]
    fn eq_true() {
        let e = Expression::Eq(
            Box::new(Expression::Literal(Value::Int(5))),
            Box::new(Expression::Literal(Value::Int(5))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    #[test]
    fn eq_false() {
        let e = Expression::Eq(
            Box::new(Expression::Literal(Value::Int(5))),
            Box::new(Expression::Literal(Value::Int(6))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(false));
    }

    #[test]
    fn neq() {
        let e = Expression::NotEq(
            Box::new(Expression::Literal(Value::Int(5))),
            Box::new(Expression::Literal(Value::Int(6))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    #[test]
    fn lt() {
        let e = Expression::Lt(
            Box::new(Expression::Literal(Value::Int(3))),
            Box::new(Expression::Literal(Value::Int(5))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    #[test]
    fn lt_false() {
        let e = Expression::Lt(
            Box::new(Expression::Literal(Value::Int(5))),
            Box::new(Expression::Literal(Value::Int(3))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(false));
    }

    #[test]
    fn gte() {
        let e = Expression::Gte(
            Box::new(Expression::Literal(Value::Int(5))),
            Box::new(Expression::Literal(Value::Int(5))),
        );
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(true));
    }

    // ── function calls ───────────────────────────────────────────────

    #[test]
    fn random_midpoint() {
        let e = Expression::Call {
            name: "random".into(),
            args: vec![
                Expression::Literal(Value::Int(10)),
                Expression::Literal(Value::Int(20)),
            ],
        };
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Int(15));
    }

    #[test]
    fn has_tag_phony() {
        let e = Expression::Call {
            name: "has_tag".into(),
            args: vec![
                Expression::Literal(Value::String("any".into())),
            ],
        };
        assert_eq!(e.eval(&EvalContext::new(&state())).unwrap(), Value::Bool(false));
    }

    // ── Expression::PartialEq ────────────────────────────────────────

    #[test]
    fn expression_eq() {
        let a = Expression::Literal(Value::Int(1));
        let b = Expression::Literal(Value::Int(1));
        assert_eq!(a, b);
        let c = Expression::Literal(Value::Int(2));
        assert_ne!(a, c);
    }

    // ── visited / count / has_tag tests ───────────────────────────────

    #[test]
    fn visited_when_not_visited() {
        let state = state();
        let e = Expression::Call {
            name: "visited".into(),
            args: vec![Expression::Literal(Value::String("cave".into()))],
        };
        assert_eq!(
            e.eval(&EvalContext::new(&state)).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn visited_after_visit() {
        let mut state = state();
        state.mark_visited(&"cave".to_string());
        let e = Expression::Call {
            name: "visited".into(),
            args: vec![Expression::Literal(Value::String("cave".into()))],
        };
        assert_eq!(
            e.eval(&EvalContext::new(&state)).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn count_zero_when_not_visited() {
        let state = state();
        let e = Expression::Call {
            name: "count".into(),
            args: vec![Expression::Literal(Value::String("cave".into()))],
        };
        assert_eq!(
            e.eval(&EvalContext::new(&state)).unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn count_after_multiple_visits() {
        let mut state = state();
        state.mark_visited(&"cave".to_string());
        state.mark_visited(&"cave".to_string());
        state.mark_visited(&"cave".to_string());
        let e = Expression::Call {
            name: "count".into(),
            args: vec![Expression::Literal(Value::String("cave".into()))],
        };
        assert_eq!(
            e.eval(&EvalContext::new(&state)).unwrap(),
            Value::Int(3)
        );
    }

    #[test]
    fn has_tag_with_graph_context() {
        use crate::passage::{PassageGraph, PassageNode};
        let mut state = state();
        state.current_passage = Some("intro".to_string());

        let graph = PassageGraph {
            nodes: vec![
                PassageNode {
                    id: "intro".to_string(),
                    tags: vec!["opening".to_string(), "safe".to_string()],
                    body: vec![],
                    scripts: vec![],
                    hooks: vec![],
                },
            ],
            edges: vec![],
        };

        let e = Expression::Call {
            name: "has_tag".into(),
            args: vec![Expression::Literal(Value::String("opening".into()))],
        };
        assert_eq!(
            e.eval(&EvalContext::with_graph(&state, &graph)).unwrap(),
            Value::Bool(true)
        );

        let e2 = Expression::Call {
            name: "has_tag".into(),
            args: vec![Expression::Literal(Value::String("danger".into()))],
        };
        assert_eq!(
            e2.eval(&EvalContext::with_graph(&state, &graph)).unwrap(),
            Value::Bool(false)
        );
    }
}

impl PartialEq for Expression {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Expression::Literal(a), Expression::Literal(b)) => a == b,
            (Expression::StateVar(a), Expression::StateVar(b)) => a == b,
            (Expression::Not(a), Expression::Not(b)) => a == b,
            (Expression::And(a1, a2), Expression::And(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Or(a1, a2), Expression::Or(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Eq(a1, a2), Expression::Eq(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::NotEq(a1, a2), Expression::NotEq(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Lt(a1, a2), Expression::Lt(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Lte(a1, a2), Expression::Lte(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Gt(a1, a2), Expression::Gt(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Gte(a1, a2), Expression::Gte(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Add(a1, a2), Expression::Add(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Sub(a1, a2), Expression::Sub(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Mul(a1, a2), Expression::Mul(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Div(a1, a2), Expression::Div(b1, b2)) => a1 == b1 && a2 == b2,
            (Expression::Call { name: n1, args: a1 }, Expression::Call { name: n2, args: a2 }) => {
                n1 == n2 && a1 == a2
            }
            _ => false,
        }
    }
}
