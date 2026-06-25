//! Render abstraction layer for Pathos.
//!
//! Defines the `RenderBackend` trait that all render targets
//! (TUI, Web, GUI) implement. Also provides `MockBackend` for testing.

use pathos_core::RenderCommand;

/// Trait that every render backend (TUI, Web, GUI) must implement.
///
/// Engine pushes `RenderCommand` batches via `render()`, and the backend
/// decides how to display them (terminal, DOM, canvas, etc.).
pub trait RenderBackend {
    /// Render a batch of commands to the output device.
    fn render(&mut self, commands: Vec<RenderCommand>);

    /// Clear the output screen.
    fn clear(&mut self);
}

/// A mock render backend for use in tests.
///
/// Accumulates all rendered text into a `Vec<String>` so tests can
/// inspect what the engine emitted.
#[derive(Debug, Default)]
pub struct MockBackend {
    /// All text lines produced by `render()` calls, in order.
    pub output: Vec<String>,
    /// All choices emitted, in order (most recent first).
    pub choices: Vec<Vec<pathos_core::Choice>>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return all rendered text as a single string joined by newlines.
    pub fn text(&self) -> String {
        self.output.join("\n")
    }

    /// Return the most recent choice list, if any.
    pub fn last_choices(&self) -> Option<&[pathos_core::Choice]> {
        self.choices.last().map(|v| v.as_slice())
    }
}

impl RenderBackend for MockBackend {
    fn render(&mut self, commands: Vec<RenderCommand>) {
        for cmd in commands {
            match cmd {
                RenderCommand::Clear => {
                    self.output.clear();
                }
                RenderCommand::Text(s) => {
                    self.output.push(s);
                }
                RenderCommand::StreamBegin => {
                    self.output.push("[stream begin]".into());
                }
                RenderCommand::StreamToken(t) => {
                    self.output.push(format!("[token: {}]", t));
                }
                RenderCommand::StreamEnd => {
                    self.output.push("[stream end]".into());
                }
                RenderCommand::StreamFailed { fallback } => {
                    self.output.push(fallback);
                }
                RenderCommand::Choice(choices) => {
                    self.choices.push(choices);
                }
                RenderCommand::Input { .. } => {
                    // Input prompts are not stored in MockBackend
                }
                RenderCommand::Separator => {
                    self.output.push("---".into());
                }
            }
        }
    }

    fn clear(&mut self) {
        self.output.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_backend_accumulates_text() {
        let mut backend = MockBackend::new();
        backend.render(vec![
            RenderCommand::Text("Hello".into()),
            RenderCommand::Text("World".into()),
        ]);
        assert_eq!(backend.output, vec!["Hello", "World"]);
    }

    #[test]
    fn mock_backend_clear() {
        let mut backend = MockBackend::new();
        backend.render(vec![RenderCommand::Text("data".into())]);
        backend.clear();
        assert!(backend.output.is_empty());
    }

    #[test]
    fn mock_backend_collects_choices() {
        let mut backend = MockBackend::new();
        backend.render(vec![RenderCommand::Choice(vec![
            pathos_core::Choice {
                label: "Go".into(),
                target: "room".into(),
                enabled: true,
                tooltip: None,
            },
        ])]);
        let choices = backend.last_choices().unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].label, "Go");
    }
}
