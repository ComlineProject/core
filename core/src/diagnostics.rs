//! The crate's single diagnostic type and its one rendering path. Parse
//! errors and validation errors both become one of these, rendered the
//! same way via `ariadne` — instead of each pipeline stage inventing its
//! own `Debug`-dump or ad hoc string.

use std::ops::Range;

use ariadne::{Label, Report, ReportKind, Source};

type SpanId = (String, Range<usize>);

/// How urgently a diagnostic should be treated - one-to-one with LSP's four
/// `DiagnosticSeverity` tiers, so mapping to the editor never needs a
/// translation table. Every diagnostic today is `Error`; the other three
/// exist as infrastructure for later work (configurable lint-style
/// severity, deprecation diagnostics) - nothing currently constructs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A diagnostic's stable identity: a namespaced, kebab-case code
/// (`"settings::malformed-value"`, `"validation::unknown-type"`) paired
/// with its one intrinsic severity. Bundled into a single type on purpose,
/// not two independent fields on `Diagnostic` - a given code's severity is
/// a fact about what it means, not a per-call-site choice, so it should be
/// structurally impossible to construct the same code as `Error` in one
/// place and `Warning` in another. Each module that raises diagnostics
/// declares its own small set of `const DiagnosticCode`s near the logic
/// that raises them (see `settings::enforcement`, `validation::validator`,
/// …) rather than one central registry every new diagnostic has to join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticCode {
    pub id: &'static str,
    pub severity: Severity,
}

impl DiagnosticCode {
    pub const fn error(id: &'static str) -> Self {
        Self { id, severity: Severity::Error }
    }

    pub const fn warning(id: &'static str) -> Self {
        Self { id, severity: Severity::Warning }
    }

    pub const fn info(id: &'static str) -> Self {
        Self { id, severity: Severity::Info }
    }

    pub const fn hint(id: &'static str) -> Self {
        Self { id, severity: Severity::Hint }
    }
}

/// A human-facing problem with a schema or package config: a stable code,
/// a message, an optional short label for its span (`"unexpected here"`,
/// `"Struct 'User'"`), an optional byte range into the source, and an
/// optional suggestion (`"did you mean `str`?"`).
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub context: String,
    pub span: Option<(usize, usize)>,
    pub help: Option<String>,
    /// A second, related span with its own short label - e.g. "this is
    /// where the bad value was actually written" (`span`) plus "applied
    /// here" (`secondary`), for an error whose root cause and trigger site
    /// are different places. Most diagnostics have none.
    pub secondary: Option<((usize, usize), String)>,
    /// A project-configured severity for this one diagnostic (settings-
    /// driven `diagnostics.<code>.level`), distinct from `code.severity` -
    /// deliberately not stored *in* `code`: a `DiagnosticCode`'s severity
    /// is its one intrinsic, non-reconfigurable meaning (see
    /// `DiagnosticCode`'s own doc comment), while this is a downstream,
    /// explicit policy choice layered on top. `None` (the default) means
    /// "use `code.severity`, unchanged" - see [`Self::effective_severity`].
    pub severity_override: Option<Severity>,
}

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code, message: message.into(), context: String::new(), span: None, help: None,
            secondary: None, severity_override: None,
        }
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

    /// A second, related span - rendered as its own label alongside the
    /// primary one. `label` is short, like `span`'s own context (e.g.
    /// "struct 'S'", the usage site that triggered this diagnostic).
    pub fn with_secondary_span(mut self, span: (usize, usize), label: impl Into<String>) -> Self {
        self.secondary = Some((span, label.into()));
        self
    }

    /// Like [`with_secondary_span`](Self::with_secondary_span), but for a
    /// caller whose related site only sometimes has a span - the mirror of
    /// [`maybe_span`](Self::maybe_span). A duplicate declaration is the
    /// case in hand: both the offending declaration and the one it collides
    /// with may be a kind that carries no span of its own.
    pub fn maybe_secondary(self, span: Option<(usize, usize)>, label: impl Into<String>) -> Self {
        match span {
            Some(span) => self.with_secondary_span(span, label),
            None => self,
        }
    }

    /// A settings-configured severity for this one diagnostic - see
    /// [`Self::severity_override`]'s own doc comment for why this doesn't
    /// just mutate `code`.
    pub fn with_severity_override(mut self, severity: Severity) -> Self {
        self.severity_override = Some(severity);
        self
    }

    /// The severity to actually treat this diagnostic as: the settings
    /// override when one was applied, `code.severity` (its unchanged
    /// intrinsic default) otherwise. Every consumer that branches on
    /// severity (`render`, the LSP's `DiagnosticSeverity` mapping) should
    /// call this, not `code.severity` directly.
    pub fn effective_severity(&self) -> Severity {
        self.severity_override.unwrap_or(self.code.severity)
    }
}

const UNEXPECTED_TOKEN: DiagnosticCode = DiagnosticCode::error("parse::unexpected-token");
const MISSING_TOKEN: DiagnosticCode = DiagnosticCode::error("parse::missing-token");
const SYNTAX_ERROR: DiagnosticCode = DiagnosticCode::error("parse::syntax-error");
const UNCLOSED_BRACKET: DiagnosticCode = DiagnosticCode::error("parse::unclosed-bracket");
const MISSING_RETURN_TYPE: DiagnosticCode = DiagnosticCode::error("parse::missing-return-type");

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
        Some((code, message, context, span, help)) => {
            let mut d = Diagnostic::new(code, message).with_context(context).with_span(span);
            if let Some(help) = help {
                d = d.with_help(help);
            }
            d
        }
        None => {
            let mut d =
                Diagnostic::new(SYNTAX_ERROR, "syntax error: unrecognized or incomplete syntax");
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

/// The bytes of `source` that are code, with their offsets: string
/// literals and `//`/`/* */` comments are skipped the same way both
/// grammars' lexers do. A string's opening `"` is kept, so a literal still
/// counts as *something* where it appears; its contents and closing `"`
/// are not.
fn code_bytes(source: &str) -> Vec<(usize, u8)> {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                out.push((i, b'"'));
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
            b => out.push((i, b)),
        }
        i += 1;
    }
    out
}

/// Scan `source` for a `{` or `[` with no matching close by EOF, skipping
/// string literals and comments (see [`code_bytes`]). An unclosed bracket
/// makes every downstream rust-sitter parse
/// error unreliable noise - once the parser can't find a matching close, it
/// often gives up and wraps the *entire remaining document* in one
/// `FailedNode` with no finer-grained position to offer
/// ([`from_parse_errors`] has nothing to drill into in that case). This is
/// usually the actual, fixable mistake, found independently of whatever
/// (often whole-file-spanning) error the parser's own recovery produced.
pub fn find_unclosed_bracket(source: &str) -> Option<Diagnostic> {
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (pos, b) in code_bytes(source) {
        match b {
            b'{' | b'[' => stack.push((pos, b)),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    // The outermost unclosed bracket - the one to fix first.
    let (pos, ch) = *stack.first()?;
    let close = if ch == b'{' { '}' } else { ']' };
    Some(
        Diagnostic::new(UNCLOSED_BRACKET, format!("unclosed `{}`", ch as char))
            .with_context("opened here")
            .with_span((pos, pos + 1))
            .with_help(format!("add a matching `{close}` to close this block")),
    )
}

/// Every `->` in a `.ids` source with no type after it - the next code
/// byte is a `!`, `;` or `}`, or there is none. The parser itself only
/// reports an empty `FailedNode` over the `->` (or, when the `->` ends a
/// block, over the whole file), which [`from_parse_errors`] can only turn
/// into the generic "unrecognized or incomplete syntax". A function
/// written up to its return type and no further is common enough while
/// typing to deserve its own name.
pub fn find_missing_return_types(source: &str) -> Vec<Diagnostic> {
    let code = code_bytes(source);
    let mut out = Vec::new();
    for (k, window) in code.windows(2).enumerate() {
        let [(pos, b'-'), (next, b'>')] = *window else { continue };
        if next != pos + 1 {
            continue;
        }
        let after = code[k + 2..].iter().map(|&(_, b)| b).find(|b| !b.is_ascii_whitespace());
        if matches!(after, None | Some(b'!' | b';' | b'}')) {
            out.push(
                Diagnostic::new(MISSING_RETURN_TYPE, "missing return type after `->`")
                    .with_context("expected a type after this")
                    .with_span((pos, pos + 2))
                    .with_help(
                        "write the type it returns, `()` for an empty reply, \
                         or remove the `->` for a one-way call",
                    ),
            );
        }
    }
    out
}

/// The one diagnostic for a `.ids` schema that failed to parse: an
/// unclosed bracket first (it makes everything after it noise), then a
/// missing return type, then the parser's own most informative error.
pub fn from_schema_parse_failure(
    source: &str,
    errors: &[rust_sitter::errors::ParseError],
) -> Diagnostic {
    find_unclosed_bracket(source)
        .or_else(|| find_missing_return_types(source).into_iter().next())
        .unwrap_or_else(|| from_parse_errors(errors))
}

#[allow(clippy::type_complexity)]
fn most_informative(
    errors: &[rust_sitter::errors::ParseError],
) -> Option<(DiagnosticCode, String, &'static str, (usize, usize), Option<String>)> {
    use rust_sitter::errors::ParseErrorReason;

    for err in errors {
        match &err.reason {
            ParseErrorReason::UnexpectedToken(token) => {
                return Some((
                    UNEXPECTED_TOKEN,
                    format!("unexpected token `{token}`"),
                    "unexpected here",
                    (err.start, err.end),
                    get_suggestion(token),
                ));
            }
            ParseErrorReason::MissingToken(expected) => {
                return Some((
                    MISSING_TOKEN,
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
    let flat = || {
        let prefixed = format!("[{}] {}", d.code.id, d.message);
        match d.context.is_empty() {
            true => prefixed,
            false => format!("{prefixed}\n  {}", d.context),
        }
    };

    let Some(span) = d.span else {
        return flat();
    };

    let label = if d.context.is_empty() { "here" } else { d.context.as_str() };
    let byte_range: Range<usize> = span.0..span.1;
    let kind = match d.effective_severity() {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Info | Severity::Hint => ReportKind::Advice,
    };

    let mut builder = Report::<SpanId>::build(kind, filename.to_string(), span.0)
        .with_code(d.code.id)
        .with_message(&d.message)
        .with_label(Label::new((filename.to_string(), byte_range)).with_message(label));

    if let Some((sec_span, sec_label)) = &d.secondary {
        let sec_range: Range<usize> = sec_span.0..sec_span.1;
        builder = builder
            .with_label(Label::new((filename.to_string(), sec_range)).with_message(sec_label));
    }

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
        assert_eq!(d.code.id, "parse::unexpected-token");
    }

    #[test]
    fn finds_a_missing_token_one_level_in() {
        let errors =
            vec![ParseError { reason: ParseErrorReason::MissingToken(":".to_string()), start: 5, end: 5 }];
        let d = from_parse_errors(&errors);
        assert_eq!(d.message, "missing required token `:`");
        assert_eq!(d.span, Some((5, 5)));
        assert_eq!(d.code.id, "parse::missing-token");
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

    const TEST_CODE: DiagnosticCode = DiagnosticCode::error("test::example");

    #[test]
    fn render_with_a_span_produces_a_source_snippet() {
        let d = Diagnostic::new(UNEXPECTED_TOKEN, "unexpected token `/`")
            .with_context("unexpected here")
            .with_span((8, 9));
        let rendered = render(&d, "main.ids", "struct Foo {\n  / bar: str\n}\n");
        assert!(rendered.contains("main.ids"), "{rendered}");
        assert!(rendered.contains("unexpected token"), "{rendered}");
        assert!(rendered.contains("parse::unexpected-token"), "code should appear: {rendered}");
        // No raw Rust struct syntax ever leaks into the rendered text.
        assert!(!rendered.contains("ParseError {"), "{rendered}");
        assert!(!rendered.contains("FailedNode("), "{rendered}");
    }

    #[test]
    fn render_with_a_secondary_span_shows_both_source_snippets() {
        let d = Diagnostic::new(TEST_CODE, "'x.allowed' must be a boolean (`true` or `false`), got Tru")
            .with_context("'x.allowed' set here")
            .with_span((8, 9))
            .with_secondary_span((20, 26), "struct 'S'");
        let source = "settings { x.allowed = Tru }\nstruct S { @x message: str }\n";
        let rendered = render(&d, "main.ids", source);
        assert!(rendered.contains("main.ids"), "{rendered}");
        assert!(rendered.contains("must be a boolean"), "{rendered}");
        assert!(rendered.contains("struct 'S'"), "secondary label should appear: {rendered}");
    }

    #[test]
    fn render_without_a_span_falls_back_to_a_flat_message() {
        let d = Diagnostic::new(TEST_CODE, "something went wrong").with_context("Struct 'User'");
        assert_eq!(
            render(&d, "main.ids", "irrelevant"),
            "[test::example] something went wrong\n  Struct 'User'"
        );
    }

    #[test]
    fn render_without_a_span_or_context_is_just_the_message() {
        let d = Diagnostic::new(TEST_CODE, "something went wrong");
        assert_eq!(render(&d, "main.ids", "irrelevant"), "[test::example] something went wrong");
    }

    #[test]
    fn render_maps_severity_to_the_right_ariadne_report_kind() {
        let source = "struct Foo {\n  / bar: str\n}\n";
        let at = |severity_ctor: fn(&'static str) -> DiagnosticCode, expect: &str| {
            let code = severity_ctor("test::example");
            let d = Diagnostic::new(code, "message").with_span((0, 1));
            let rendered = render(&d, "main.ids", source);
            assert!(rendered.contains(expect), "expected {expect:?} in: {rendered}");
        };
        at(DiagnosticCode::error, "Error");
        at(DiagnosticCode::warning, "Warning");
        at(DiagnosticCode::info, "Advice");
        at(DiagnosticCode::hint, "Advice");
    }

    #[test]
    fn effective_severity_defaults_to_the_codes_own_severity() {
        let d = Diagnostic::new(DiagnosticCode::warning("test::example"), "msg");
        assert_eq!(d.effective_severity(), Severity::Warning);
    }

    #[test]
    fn effective_severity_uses_the_override_when_set() {
        let d = Diagnostic::new(DiagnosticCode::warning("test::example"), "msg")
            .with_severity_override(Severity::Error);
        assert_eq!(d.effective_severity(), Severity::Error);
        // The code's own intrinsic severity is untouched by the override.
        assert_eq!(d.code.severity, Severity::Warning);
    }

    #[test]
    fn render_uses_the_overridden_severity_not_the_codes_own() {
        let source = "struct Foo {\n  / bar: str\n}\n";
        let d = Diagnostic::new(DiagnosticCode::warning("test::example"), "message")
            .with_span((0, 1))
            .with_severity_override(Severity::Error);
        let rendered = render(&d, "main.ids", source);
        assert!(rendered.contains("Error"), "expected an Error-kind render: {rendered}");
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
        assert_eq!(d.code.id, "parse::unclosed-bracket");
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

    #[test]
    fn an_arrow_followed_by_a_bang_is_a_missing_return_type() {
        let source = "protocol P {\n    function greet(a: u8) -> ! E;\n}\n";
        let found = find_missing_return_types(source);
        assert_eq!(found.len(), 1);
        let d = &found[0];
        assert_eq!(d.code.id, "parse::missing-return-type");
        assert_eq!(d.message, "missing return type after `->`");
        let (start, end) = d.span.unwrap();
        assert_eq!(&source[start..end], "->");
    }

    #[test]
    fn an_arrow_followed_by_a_semicolon_brace_or_eof_is_a_missing_return_type() {
        for source in [
            "protocol P {\n    function f() -> ;\n}\n",
            "protocol P {\n    function f() ->\n}\n",
            "protocol P {\n    function f() ->",
            "protocol P {\n    function f() -> /* soon */ ;\n}\n",
        ] {
            assert_eq!(find_missing_return_types(source).len(), 1, "{source:?}");
        }
    }

    #[test]
    fn an_arrow_with_a_type_after_it_is_fine() {
        for source in [
            "protocol P {\n    function f() -> u8 ! E;\n}\n",
            "protocol P {\n    function f() -> ();\n}\n",
            "protocol P {\n    function f() -> std::http::Response;\n}\n",
            "protocol P {\n    function f() -> u8[];\n}\n",
            "protocol P {\n    function f();\n}\n",
        ] {
            assert!(find_missing_return_types(source).is_empty(), "{source:?}");
        }
    }

    #[test]
    fn an_arrow_inside_a_comment_or_string_is_not_checked() {
        let source = "// f() -> ;\nconst S: str = \"-> ;\"\n/* -> } */\n";
        assert!(find_missing_return_types(source).is_empty());
    }

    #[test]
    fn a_schema_parse_failure_names_the_missing_return_type() {
        let source = "protocol P {\n    function greet(a: u8) -> ! E;\n}\n";
        let errors = crate::schema::idl::grammar::parse(source).unwrap_err();
        let d = from_schema_parse_failure(source, &errors);
        assert_eq!(d.code.id, "parse::missing-return-type");
    }
}
