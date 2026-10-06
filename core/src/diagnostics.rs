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

    /// Like [`with_span`](Self::with_span), but for a caller that only
    /// sometimes has one (e.g. a declaration kind whose `FrozenUnit`
    /// carries no span) - `None` leaves the diagnostic spanless rather
    /// than forcing every call site to branch.
    pub fn maybe_span(self, span: Option<(usize, usize)>) -> Self {
        match span {
            Some(span) => self.with_span(span),
            None => self,
        }
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
        None => {
            let mut d = Diagnostic::new("syntax error: unrecognized or incomplete syntax");
            if let Some(span) = errors.first().map(deepest_span) {
                d = d.with_span(span);
            }
            d
        }
    }
}

/// Follow a `FailedNode` chain down to its innermost node's own span - the
/// last-resort location when no leaf names an actual token (an empty
/// `FailedNode(vec![])`, emitted for an error node with no children and no
/// text - the shape of a file cut off mid-construct, hitting EOF before
/// completing it).
fn deepest_span(err: &rust_sitter::errors::ParseError) -> (usize, usize) {
    match &err.reason {
        rust_sitter::errors::ParseErrorReason::FailedNode(nested) => {
            nested.first().map(deepest_span).unwrap_or((err.start, err.end))
        }
        _ => (err.start, err.end),
    }
}

/// Scan `source` for a `{` or `[` with no matching close by EOF, skipping
/// string literals and `//`/`/* */` comments the same way both grammars'
/// lexers do. An unclosed bracket makes every downstream rust-sitter parse
/// error unreliable noise - once the parser can't find a matching close, it
/// often gives up and wraps the *entire remaining document* in one
/// `FailedNode` with no finer-grained position to offer
/// ([`from_parse_errors`] has nothing to drill into in that case). This is
/// usually the actual, fixable mistake, found independently of whatever
/// (often whole-file-spanning) error the parser's own recovery produced.
pub fn find_unclosed_bracket(source: &str) -> Option<Diagnostic> {
    let bytes = source.as_bytes();
    let mut stack: Vec<(usize, u8)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'{' | b'[' => stack.push((i, bytes[i])),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
        i += 1;
    }
    // The outermost unclosed bracket - the one to fix first.
    let (pos, ch) = *stack.first()?;
    let close = if ch == b'{' { '}' } else { ']' };
    Some(
        Diagnostic::new(format!("unclosed `{}`", ch as char))
            .with_context("opened here")
            .with_span((pos, pos + 1))
            .with_help(format!("add a matching `{close}` to close this block")),
    )
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
    fn an_empty_failed_node_still_gets_a_span_from_its_innermost_node() {
        // The shape `rust_sitter::errors::collect_parsing_errors` produces
        // for a file cut off mid-construct (e.g. a dangling `=` with no
        // value): a `FailedNode` nested a couple levels deep, bottoming out
        // in an empty vec at the point parsing actually gave up. No leaf
        // names a token, so `most_informative` finds nothing - but the
        // innermost node's own span must still survive into the
        // `Diagnostic`, not get discarded.
        let errors = vec![ParseError {
            reason: ParseErrorReason::FailedNode(vec![ParseError {
                reason: ParseErrorReason::FailedNode(vec![]),
                start: 30,
                end: 30,
            }]),
            start: 18,
            end: 30,
        }];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "syntax error: unrecognized or incomplete syntax");
        assert_eq!(d.span, Some((30, 30)));
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

    #[test]
    fn balanced_source_has_no_unclosed_bracket() {
        assert_eq!(find_unclosed_bracket("congregation test\nx = {\n  y = 1\n}\n"), None);
    }

    #[test]
    fn an_unclosed_brace_is_found_at_its_own_position() {
        let source = "congregation test\ndeps = {\n";
        let d = find_unclosed_bracket(source).expect("the `{` on line 2 never closes");
        assert_eq!(d.message, "unclosed `{`");
        let (start, end) = d.span.expect("a span pointing at the `{`");
        assert_eq!(&source[start..end], "{");
        assert_eq!(d.help.as_deref(), Some("add a matching `}` to close this block"));
    }

    #[test]
    fn a_brace_inside_a_string_literal_is_not_a_real_bracket() {
        assert_eq!(find_unclosed_bracket(r#"congregation test\nx = "foo { bar"\n"#), None);
    }

    #[test]
    fn a_brace_inside_a_line_comment_is_not_a_real_bracket() {
        assert_eq!(find_unclosed_bracket("congregation test\n// { oops\nx = 1\n"), None);
    }

    #[test]
    fn a_brace_inside_a_block_comment_is_not_a_real_bracket() {
        let source = "congregation test\n/* unfinished {\n   still going\n*/\nx = 1\n";
        assert_eq!(find_unclosed_bracket(source), None);
    }

    #[test]
    fn nested_unclosed_brackets_report_the_outermost_one() {
        let source = "congregation test\na = {\n  b = [\n";
        let d = find_unclosed_bracket(source).expect("both `{` and `[` are unclosed");
        assert_eq!(d.message, "unclosed `{`");
        let (start, _) = d.span.unwrap();
        assert_eq!(&source[start..start + 1], "{");
    }
}
