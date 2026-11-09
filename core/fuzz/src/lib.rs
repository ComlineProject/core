//! What every target checks about the diagnostics it gets back.

use comline_core::diagnostics::Diagnostic;

/// Every diagnostic points somewhere inside `source`: a span that's in
/// bounds, ordered, and on character boundaries, so the CLI and the
/// language server can render it. The same for a secondary span.
pub fn check_spans(source: &str, diagnostics: &[Diagnostic]) {
    assert!(!diagnostics.is_empty(), "an error with no diagnostic for {source:?}");
    for d in diagnostics {
        let Some(span) = d.span else {
            panic!("diagnostic {} has no span: {:?} for {source:?}", d.code.id, d.message);
        };
        check_span(source, span, d);
        if let Some((secondary, _)) = d.secondary {
            check_span(source, secondary, d);
        }
    }
}

fn check_span(source: &str, (start, end): (usize, usize), d: &Diagnostic) {
    assert!(
        start <= end && end <= source.len(),
        "diagnostic {} has span {start}..{end} in {} bytes: {:?} for {source:?}",
        d.code.id, source.len(), d.message,
    );
    assert!(
        source.is_char_boundary(start) && source.is_char_boundary(end),
        "diagnostic {} has span {start}..{end} inside a character: {:?} for {source:?}",
        d.code.id, d.message,
    );
}
