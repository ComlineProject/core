//! The crate's single diagnostic type and its one rendering path. Parse
//! errors and validation errors both become one of these, rendered the
//! same way via `ariadne` — instead of each pipeline stage inventing its
//! own `Debug`-dump or ad hoc string.

use std::ops::Range;

use ariadne::{Label, Report, ReportKind, Source};

type SpanId = (String, Range<usize>);

/// A human-facing problem with a schema or package config: a message, an
/// optional short label for its span (`"unexpected here"`, `"Struct
/// 'User'"`), an optional byte range into the source, and an optional
/// suggestion (`"did you mean `str`?"`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diagnostic {
    pub message: String,
    pub context: String,
    pub span: Option<(usize, usize)>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), context: String::new(), span: None, help: None }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    pub fn with_span(mut self, span: (usize, usize)) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// Collapse a grammar's raw `Vec<ParseError>` (both the `.ids` schema
/// grammar and the `.idp` package-config grammar share this exact
/// rust-sitter type) into the single most-informative `Diagnostic`. A
/// parse failure's entries - both the top-level vec and each nested
/// `FailedNode` - are alternative branches of the *same* underlying
/// failure, not independent errors, so this walks them depth-first and
/// keeps the first leaf that actually names a token, the same rule
/// `language-server`'s own (separately maintained) copy of this search
/// uses.
pub fn from_parse_errors(errors: &[rust_sitter::errors::ParseError]) -> Diagnostic {
    match most_informative(errors) {
        Some((message, context, span, help)) => {
            let mut d = Diagnostic::new(message).with_context(context).with_span(span);
            if let Some(help) = help {
                d = d.with_help(help);
            }
            d
        }
        None => Diagnostic::new("syntax error: unrecognized or incomplete syntax"),
    }
}

#[allow(clippy::type_complexity)]
fn most_informative(
    errors: &[rust_sitter::errors::ParseError],
) -> Option<(String, &'static str, (usize, usize), Option<String>)> {
    use rust_sitter::errors::ParseErrorReason;

    for err in errors {
        match &err.reason {
            ParseErrorReason::UnexpectedToken(token) => {
                return Some((
                    format!("unexpected token `{token}`"),
                    "unexpected here",
                    (err.start, err.end),
                    get_suggestion(token),
                ));
            }
            ParseErrorReason::MissingToken(expected) => {
                return Some((
                    format!("missing required token `{expected}`"),
                    "expected here",
                    (err.start, err.end),
                    None,
                ));
            }
            ParseErrorReason::FailedNode(nested) => {
                if let Some(found) = most_informative(nested) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Suggestions for common mistakes a bare `UnexpectedToken` can't explain
/// on its own - ported from the retired `schema::idl::diagnostics`.
fn get_suggestion(token: &str) -> Option<String> {
    match token {
        "string" => Some("did you mean `str`?".to_string()),
        "int" => Some("use sized integer types like `u32`, `s32`, `u64`, etc.".to_string()),
        "float" => Some("use `f32` or `f64`".to_string()),
        "array" => Some("use array syntax like `Type[]` or `Type[N]`".to_string()),
        _ => None,
    }
}

/// Render one diagnostic as a rich, source-span-aware string via
/// `ariadne`. Falls back to a flat `message\n  context` when there's no
/// span, or if rendering itself fails.
pub fn render(d: &Diagnostic, filename: &str, source: &str) -> String {
    let flat = || match d.context.is_empty() {
        true => d.message.clone(),
        false => format!("{}\n  {}", d.message, d.context),
    };

    let Some(span) = d.span else {
        return flat();
    };

    let label = if d.context.is_empty() { "here" } else { d.context.as_str() };
    let byte_range: Range<usize> = span.0..span.1;

    let mut builder = Report::<SpanId>::build(ReportKind::Error, filename.to_string(), span.0)
        .with_message(&d.message)
        .with_label(Label::new((filename.to_string(), byte_range)).with_message(label));

    if let Some(help) = &d.help {
        builder = builder.with_help(help);
    }

    let report = builder.finish();

    let mut buf = Vec::new();
    match report.write((filename.to_string(), Source::from(source)), &mut buf) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(_) => flat(),
    }
}

/// [`render`] for several diagnostics against the same source, joined with
/// blank lines (e.g. several validation errors found in one schema).
pub fn render_all(ds: &[Diagnostic], filename: &str, source: &str) -> String {
    ds.iter().map(|d| render(d, filename, source)).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_sitter::errors::{ParseError, ParseErrorReason};

    #[test]
    fn finds_an_unexpected_token_one_level_in() {
        let errors = vec![ParseError {
            reason: ParseErrorReason::UnexpectedToken("???".to_string()),
            start: 5,
            end: 8,
        }];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "unexpected token `???`");
        assert_eq!(d.span, Some((5, 8)));
        assert_eq!(d.help, None);
    }

    #[test]
    fn finds_a_missing_token_one_level_in() {
        let errors =
            vec![ParseError { reason: ParseErrorReason::MissingToken(":".to_string()), start: 5, end: 5 }];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "missing required token `:`");
        assert_eq!(d.span, Some((5, 5)));
    }

    #[test]
    fn recurses_through_a_doubly_nested_failed_node_and_keeps_the_leafs_own_span() {
        // The leaf's span (7..13) is what gets kept and rendered, not the
        // outer FailedNode's wider span (5..13) - the underline-accuracy
        // fix over the old codespan-reporting renderer, which always used
        // the outer span regardless of nesting depth.
        let errors = vec![ParseError {
            reason: ParseErrorReason::FailedNode(vec![ParseError {
                reason: ParseErrorReason::UnexpectedToken("string".to_string()),
                start: 7,
                end: 13,
            }]),
            start: 5,
            end: 13,
        }];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "unexpected token `string`");
        assert_eq!(d.span, Some((7, 13)));
        assert_eq!(d.help.as_deref(), Some("did you mean `str`?"));
    }

    #[test]
    fn an_empty_error_list_yields_the_generic_fallback_message() {
        let d = from_parse_errors(&[]);
        assert_eq!(d.message, "syntax error: unrecognized or incomplete syntax");
        assert_eq!(d.span, None);
    }

    #[test]
    fn multiple_top_level_errors_use_the_first_informative_one() {
        // The exact shape of the reported bug: several top-level
        // `ParseError`s from one malformed region. The first one that
        // names a token wins, not all of them concatenated.
        let errors = vec![
            ParseError {
                reason: ParseErrorReason::FailedNode(vec![ParseError {
                    reason: ParseErrorReason::UnexpectedToken("/".to_string()),
                    start: 1761,
                    end: 1762,
                }]),
                start: 1761,
                end: 1763,
            },
            ParseError {
                reason: ParseErrorReason::FailedNode(vec![ParseError {
                    reason: ParseErrorReason::UnexpectedToken("/".to_string()),
                    start: 1797,
                    end: 1798,
                }]),
                start: 1796,
                end: 1798,
            },
        ];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "unexpected token `/`");
        assert_eq!(d.span, Some((1761, 1762)));
    }

    #[test]
    fn render_with_a_span_produces_a_source_snippet() {
        let d = Diagnostic::new("unexpected token `/`")
            .with_context("unexpected here")
            .with_span((8, 9));
        let rendered = render(&d, "main.ids", "struct Foo {\n  / bar: str\n}\n");
        assert!(rendered.contains("main.ids"), "{rendered}");
        assert!(rendered.contains("unexpected token"), "{rendered}");
        // No raw Rust struct syntax ever leaks into the rendered text.
        assert!(!rendered.contains("ParseError {"), "{rendered}");
        assert!(!rendered.contains("FailedNode("), "{rendered}");
    }

    #[test]
    fn render_without_a_span_falls_back_to_a_flat_message() {
        let d = Diagnostic::new("something went wrong").with_context("Struct 'User'");
        assert_eq!(render(&d, "main.ids", "irrelevant"), "something went wrong\n  Struct 'User'");
    }

    #[test]
    fn render_without_a_span_or_context_is_just_the_message() {
        let d = Diagnostic::new("something went wrong");
        assert_eq!(render(&d, "main.ids", "irrelevant"), "something went wrong");
    }
}
