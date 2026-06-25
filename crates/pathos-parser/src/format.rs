use pathos_core::{PassageGraph, StoryConfig};

/// The result of parsing a story file.
#[derive(Debug)]
pub struct ParseOutput {
    /// Story metadata extracted from the file.
    pub config: StoryConfig,
    /// The passage graph built from the file.
    pub graph: PassageGraph,
    /// Warnings and errors collected during parsing.
    pub diagnostics: Vec<Diagnostic>,
}

/// A diagnostic message produced during parsing.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// Optional source location for error reporting.
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A position range in the source text.
#[derive(Debug, Clone, Copy)]
pub struct SourceSpan {
    /// 0-based line number.
    pub line: usize,
    /// 0-based column (byte offset within the line).
    pub column: usize,
    /// Length in bytes.
    pub length: usize,
}

/// Trait for story format parsers.
///
/// Each parser handles one file format (`.pathos`, `.toml`, `.json`, `.yaml`, `.twee`).
pub trait FormatParser: Send + Sync {
    /// File extensions this parser handles (e.g. `&["pathos"]`, `&["toml"]`).
    fn extensions(&self) -> &[&str];

    /// Parse raw source text into a `ParseOutput`.
    fn parse(&self, source: &str) -> ParseOutput;
}

/// A registry of format parsers, keyed by file extension.
///
/// Dispatches `parse()` to the correct parser based on the file extension.
pub struct FormatRegistry {
    parsers: Vec<Box<dyn FormatParser>>,
}

impl FormatRegistry {
    /// Create a registry with the given parsers.
    pub fn new(parsers: Vec<Box<dyn FormatParser>>) -> Self {
        Self { parsers }
    }

    /// Look up a parser by file extension and run it.
    ///
    /// If no parser matches the extension, returns a `ParseOutput` with
    /// an error diagnostic.
    pub fn parse(&self, path: &str, source: &str) -> ParseOutput {
        let ext = path.rsplit('.').next().unwrap_or("");

        for parser in &self.parsers {
            if parser.extensions().contains(&ext) {
                return parser.parse(source);
            }
        }

        ParseOutput {
            config: StoryConfig {
                title: String::new(),
                author: String::new(),
                start: String::new(),
                version: String::new(),
                save_slots: 0,
            },
            graph: PassageGraph::default(),
            diagnostics: vec![Diagnostic {
                severity: Severity::Error,
                message: format!("unsupported file format: .{ext}"),
                span: None,
            }],
        }
    }
}

impl Default for FormatRegistry {
    fn default() -> Self {
        Self::new(vec![
            Box::new(super::TomlParser),
            Box::new(super::PathosParser),
        ])
    }
}
