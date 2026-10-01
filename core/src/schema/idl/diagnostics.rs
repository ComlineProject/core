// Schema diagnostics - beautiful error reporting
// Converts parse errors to helpful, color-coded messages

use codespan_reporting::diagnostic::{Diagnostic as CsDiagnostic, Label, Severity};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};

/// Position in source code
#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub line: usize,      // 1-indexed
    pub column: usize,    // 1-indexed  
    pub byte_offset: usize,
}

/// Source map for converting byte offsets to line/column positions
pub struct SourceMap {
    source: String,
    line_starts: Vec<usize>,  // Byte offset of each line start
}

impl SourceMap {
    /// Create a new source map from source code
    pub fn new(source: String) -> Self {
        let line_starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { source, line_starts }
    }
    
    /// Convert byte offset to line/column position
    pub fn lookup(&self, byte_offset: usize) -> Position {
        // Binary search to find line
        let line = match self.line_starts.binary_search(&byte_offset) {
            Ok(exact) => exact,
            Err(insert_pos) => insert_pos.saturating_sub(1),
        };
        
        let line_start = self.line_starts[line];
        let column = byte_offset.saturating_sub(line_start);
        
        Position {
            line: line + 1,  // 1-indexed
            column: column + 1,  // 1-indexed
            byte_offset,
        }
    }
    
    /// Get line content for display
    pub fn get_line(&self, line: usize) -> &str {
        if line == 0 || line > self.line_starts.len() {
            return "";
        }
        
        let start = self.line_starts[line - 1];  // Convert to 0-indexed
        let end = if line < self.line_starts.len() {
            self.line_starts[line].saturating_sub(1)  // Exclude \n
        } else {
            self.source.len()
        };
        
        &self.source[start..end]
    }
}

/// Pretty print a parse error with colors and context
pub fn print_parse_error(
    error: &rust_sitter::errors::ParseError,
    source: &str,
    filename: &str,
) {
    let source_map = SourceMap::new(source.to_string());
    let start_pos = source_map.lookup(error.start);
    let end_pos = source_map.lookup(error.end);
    
    // Determine error message and help text
    let (message, help) = match &error.reason {
        rust_sitter::errors::ParseErrorReason::UnexpectedToken(token) => {
            (format!("unexpected token `{}`", token), get_suggestion(token))
        }
        rust_sitter::errors::ParseErrorReason::MissingToken(expected) => {
            (format!("missing required token `{}`", expected), None)
        }
        rust_sitter::errors::ParseErrorReason::FailedNode(nested) => first_informative(nested)
            .unwrap_or_else(|| ("syntax error: unrecognized or incomplete syntax".to_string(), None)),
    };
    
    // Create diagnostic
    let mut files = SimpleFiles::new();
    let file_id = files.add(filename, source);
    
    // If the error spans multiple lines, show just the end position for clarity
    let (span_start, span_end, label_message) = if start_pos.line != end_pos.line {
        // Multi-line error - just highlight the end position
        (end_pos.byte_offset, end_pos.byte_offset + 1, "error here")
    } else {
        // Single-line error - show the full span
        (error.start, error.end, "unexpected here")
    };
    
    let mut diagnostic = CsDiagnostic::error()
        .with_message(&message)
        .with_labels(vec![
            Label::primary(file_id, span_start..span_end)
                .with_message(label_message),
        ]);
    
    if let Some(help_msg) = help {
        diagnostic = diagnostic.with_notes(vec![help_msg]);
    }
    
    // Add context-specific notes
    if message.contains("string") || message.contains("int") || message.contains("float") {
        diagnostic = diagnostic.with_notes(vec![
            "note: valid primitive types: u8, u16, u32, u64, s8, s16, s32, s64, f32, f64, bool, str".to_string()
        ]);
    }
    
    // Pretty print with colors
    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = codespan_reporting::term::Config::default();
    
    let _ = term::emit(&mut writer.lock(), &config, &files, &diagnostic);
}

/// Depth-first search for the first leaf in a `FailedNode`'s nested errors
/// that actually names a token — mirrors the equivalent fix in
/// `language-server/src/analysis/diagnostics.rs`'s `format_error_message`;
/// `nested.first()` alone missed anything past the first entry, a
/// `MissingToken` first entry, or nesting more than one `FailedNode` deep.
fn first_informative(
    nested: &[rust_sitter::errors::ParseError],
) -> Option<(String, Option<String>)> {
    use rust_sitter::errors::ParseErrorReason;

    for err in nested {
        match &err.reason {
            ParseErrorReason::UnexpectedToken(token) => {
                return Some((format!("unexpected token `{}`", token), get_suggestion(token)));
            }
            ParseErrorReason::MissingToken(expected) => {
                return Some((format!("missing required token `{}`", expected), None));
            }
            ParseErrorReason::FailedNode(inner) => {
                if let Some(found) = first_informative(inner) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Get helpful suggestion for common mistakes
fn get_suggestion(token: &str) -> Option<String> {
    match token {
        "string" => Some("help: did you mean `str`?".to_string()),
        "int" => Some("help: use sized integer types like `u32`, `s32`, `u64`, etc.".to_string()),
        "float" => Some("help: use `f32` or `f64`".to_string()),
        "array" => Some("help: use array syntax like `Type[]` or `Type[N]`".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_source_map_lookup() {
        let source = "line 1\nline 2\nline 3".to_string();
        let map = SourceMap::new(source);
        
        // First character
        let pos = map.lookup(0);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 1);
        
        // Start of line 2
        let pos = map.lookup(7);  // After "line 1\n"
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 1);
    }
    
    #[test]
    fn test_get_line() {
        let source = "line 1\nline 2\nline 3".to_string();
        let map = SourceMap::new(source);
        
        assert_eq!(map.get_line(1), "line 1");
        assert_eq!(map.get_line(2), "line 2");
        assert_eq!(map.get_line(3), "line 3");
    }

    // `first_informative` cases — mirrors the equivalent test suite in
    // `language-server/src/analysis/diagnostics.rs::format_error_message`.
    // `print_parse_error` itself only prints to stderr via `term::emit`
    // rather than returning its computed message, so these exercise the
    // recursion directly with constructed `ParseError` values instead of
    // capturing output.

    #[test]
    fn finds_an_unexpected_token_one_level_in() {
        use rust_sitter::errors::{ParseError, ParseErrorReason};

        let nested = vec![ParseError {
            reason: ParseErrorReason::UnexpectedToken("???".to_string()),
            start: 5,
            end: 8,
        }];
        assert_eq!(
            first_informative(&nested),
            Some(("unexpected token `???`".to_string(), None))
        );
    }

    #[test]
    fn finds_a_missing_token_one_level_in() {
        use rust_sitter::errors::{ParseError, ParseErrorReason};

        let nested = vec![ParseError {
            reason: ParseErrorReason::MissingToken(":".to_string()),
            start: 5,
            end: 5,
        }];
        assert_eq!(
            first_informative(&nested),
            Some(("missing required token `:`".to_string(), None))
        );
    }

    #[test]
    fn recurses_through_a_doubly_nested_failed_node() {
        use rust_sitter::errors::{ParseError, ParseErrorReason};

        let nested = vec![ParseError {
            reason: ParseErrorReason::FailedNode(vec![ParseError {
                reason: ParseErrorReason::UnexpectedToken("string".to_string()),
                start: 7,
                end: 13,
            }]),
            start: 5,
            end: 13,
        }];
        assert_eq!(
            first_informative(&nested),
            Some(("unexpected token `string`".to_string(), Some("help: did you mean `str`?".to_string())))
        );
    }

    #[test]
    fn an_empty_nested_vec_yields_nothing() {
        assert_eq!(first_informative(&[]), None);
    }
}
