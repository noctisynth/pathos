
//! Pathos parser — multi-format story file parser.
//!
//! Supports `.pathos` (native format), `.toml`, `.json`, `.yaml`, and `.twee` formats.
//! Each format is implemented as a `FormatParser` and auto-dispatched by file extension.

pub mod format;
pub mod inline;
pub mod toml_parser;
pub mod pathos_parser;

pub use format::{Diagnostic, FormatParser, FormatRegistry, ParseOutput, Severity, SourceSpan};
pub use inline::parse_inline;
pub use toml_parser::TomlParser;
pub use pathos_parser::PathosParser;

/// Parse a story file by auto-detecting the format from the file extension.
pub fn parse_file(path: &str, source: &str) -> ParseOutput {
    let registry = FormatRegistry::default();
    registry.parse(path, source)
}

#[cfg(test)]
mod tests {
    // Integration tests are in tests/ directory
}
