use comline_core::utils::codemap::*;

#[test]
fn insert_a_file_into_a_codemap() {
    let mut map = CodeMap::new();
    let filename = "foo.rs";
    let content = "Hello World!";

    assert_eq!(map.files().len(), 0);
    let fm = map.insert_file(filename, content);

    assert_eq!(fm.filename(), filename);
    assert_eq!(fm.contents(), content);
    assert_eq!(map.files().len(), 1);
}

#[test]
fn get_span_for_substring() {
    let mut map = CodeMap::new();
    let src = "Hello World!";
    let fm = map.insert_file("foo.rs", src);

    let start = 2;
    let end = 5;
    let should_be = &src[start..end];

    let span = fm.insert_span(start, end);
    let got = fm.lookup(span).unwrap();
    assert_eq!(got, should_be);
    assert_eq!(fm.range_of(span).unwrap(), start..end);

    let got_from_codemap = map.lookup(span);
    assert_eq!(got_from_codemap, should_be);
}

#[test]
fn spans_for_different_ranges_are_always_unique() {
    let mut map = CodeMap::new();
    let src = "Hello World!";
    let fm = map.insert_file("foo.rs", src);

    let mut spans = Vec::new();

    for start in 0..src.len() {
        for end in start..src.len() {
            let span = fm.insert_span(start, end);
            assert!(!spans.contains(&span),
                    "{:?} already contains {:?} ({}..{})",
                    spans, span, start, end);
            // Span::dummy() is pub(crate), so we use Span(0) if visible, or skip check if strictly internal.
            // Span tuple field is pub.
            assert!(span != Span(0));

            spans.push(span);
        }
    }
}

#[test]
fn spans_for_identical_ranges_are_identical() {
    let mut map = CodeMap::new();
    let src = "Hello World!";
    let fm = map.insert_file("foo.rs", src);

    let start = 0;
    let end = 5;

    let span_1 = fm.insert_span(start, end);
    let span_2 = fm.insert_span(start, end);

    assert_eq!(span_1, span_2);
}

#[test]
fn join_multiple_spans() {
    let mut map = CodeMap::new();
    let src = "Hello World!";
    let fm = map.insert_file("foo.rs", src);

    let span_1 = fm.insert_span(0, 2);
    let span_2 = fm.insert_span(3, 8);

    let joined = fm.merge(span_1, span_2);
    let equivalent_range = fm.range_of(joined).unwrap();

    assert_eq!(equivalent_range.start, 0);
    assert_eq!(equivalent_range.end, 8);
}
