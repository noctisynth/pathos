use std::collections::HashMap;
use crate::error::CoreResult;
use crate::passage::{HookDeclaration, PassageScript};
use crate::scripts::ScriptSignal;
use crate::scripts::ScriptEngine;
use crate::state::StoryState;

/// Registry of all hooks, indexed by event name.
///
/// Hooks are registered at graph build time from `@hook:` directives
/// found in passages.
#[derive(Debug, Default)]
pub struct HookRegistry {
    /// event_name → list of hook scripts (run in registration order)
    handlers: HashMap<String, Vec<RegisteredHook>>,
}

#[derive(Debug, Clone)]
struct RegisteredHook {
    script: PassageScript,
}

impl HookRegistry {
    /// Register a hook declared in a passage.
    pub fn register(&mut self, event: &str, script: PassageScript) {
        self.handlers
            .entry(event.to_string())
            .or_default()
            .push(RegisteredHook { script });
    }

    /// Register hooks from a `HookDeclaration` list.
    pub fn register_all(&mut self, hooks: &[HookDeclaration]) {
        for h in hooks {
            self.register(&h.event, h.script.clone());
        }
    }

    /// Trigger all handlers registered for `event`.
    ///
    /// Returns the first navigation signal (if any), or the first error.
    /// On navigation, subsequent handlers are skipped — the lifecycle
    /// must yield control to the new passage immediately.
    pub fn trigger(
        &self,
        event: &str,
        state: &mut StoryState,
        script: &ScriptEngine,
    ) -> CoreResult<ScriptSignal> {
        let Some(handlers) = self.handlers.get(event) else {
            return Ok(ScriptSignal::None);
        };
        for h in handlers {
            let (_value, signal) = script.eval(state, &h.script.code, &h.script.lang)?;
            if signal.is_navigation() {
                return Ok(signal);
            }
        }
        Ok(ScriptSignal::None)
    }

    /// Return the number of registered handlers for `event`.
    pub fn count(&self, event: &str) -> usize {
        self.handlers.get(event).map_or(0, |v| v.len())
    }
}
