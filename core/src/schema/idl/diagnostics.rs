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
            let msg = format!("unexpected token `{}`", token);
            let help_text = get_suggestion(token);
            (msg, help_text)
        }
        rust_sitter::errors::ParseErrorReason::FailedNode(nested) => {
            // Try to extract useful info from nested errors
            if let Some(first) = nested.first() {
                if let rust_sitter::errors::ParseErrorReason::UnexpectedToken(token) = &first.reason {
                    let msg = format!("unexpected token `{}`", token);
                    let help_text = get_suggestion(token);
                    (msg, help_text)
                } else {
                    ("parse error".to_string(), None)
                }
            } else {
                ("parse error".to_string(), None)
            }
        }
        _ => ("parse error".to_string(), None),
    };
    
    // Create diagnostic
    let mut files = SimpleFiles::new();
    let file_id = files.add(filename, source);
    
    let mut diagnostic = CsDiagnostic::error()
        .with_message(&message)
        .with_labels(vec![
            Label::primary(file_id, error.start..error.end)
                .with_message("unexpected here"),
        ]);
    
    if let Some(help_msg) = help {
        diagnostic = diagnostic.with_notes(vec![help_msg]);
    }
    
    // Add note about valid types for common mistakes
    if message.contains("string") {
        diagnostic = diagnostic.with_notes(vec![
            "valid primitive types: u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, bool, str".to_string()
        ]);
    }
    
    // Pretty print with colors
    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = codespan_reporting::term::Config::default();
    
    let _ = term::emit(&mut writer.lock(), &config, &files, &diagnostic);
}

/// Get helpful suggestion for common mistakes
fn get_suggestion(token: &str) -> Option<String> {
    match token {
        "string" => Some("help: did you mean `str`?".to_string()),
        "int" => Some("help: use sized integer types like `u32`, `i32`, `u64`, etc.".to_string()),
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
}
