//! Parsing a `.ids` schema safely: the checks that have to run before the
//! syntax tree is turned into Rust types.
//!
//! [`grammar::parse`] parses with tree-sitter, which builds its tree without
//! recursing, then extracts the typed [`Document`] from it, which recurses
//! once per level of the tree. A schema nested deep enough overflows the
//! stack during that extraction and aborts the process: the CLI, the
//! language server, the playground (core#184). [`parse_schema`] measures the
//! tree's depth first, without recursing, and refuses a tree deeper than
//! [`MAX_SYNTAX_DEPTH`] with a diagnostic. It also refuses an integer
//! literal too large to store (core#165), which the grammar saturates.

use rust_sitter::tree_sitter::{Node, Parser};

use super::grammar::{self, Document};
use crate::diagnostics::{self, Diagnostic};

/// The deepest a schema's syntax tree may be. An array (`[]`) adds three
/// levels, a `union(...)` four, so this is about 80 arrays or 60 unions
/// nested in one type. That's far beyond any real schema, below where the
/// stored schema can no longer be read back (about 124 arrays), and where
/// the extraction stays well within a 1 MiB stack (it overflows one past
/// about 370 arrays).
pub const MAX_SYNTAX_DEPTH: usize = 256;

/// Parse `source` as a `.ids` schema, or say what's wrong with it: an
/// integer literal too large to store, nesting deeper than
/// [`MAX_SYNTAX_DEPTH`], or a syntax error. Never panics or overflows the
/// stack, whatever `source` holds.
pub fn parse_schema(source: &str) -> Result<Document, Vec<Diagnostic>> {
    let out_of_range = diagnostics::find_out_of_range_integers(source);
    if !out_of_range.is_empty() {
        return Err(out_of_range);
    }

    let mut parser = Parser::new();
    parser
        .set_language(&grammar::language())
        .expect("the schema grammar's language loads");
    let tree = parser
        .parse(source, None)
        .expect("a parser with a language and no timeout always returns a tree");
    let root = tree.root_node();

    if let Some(node) = deeper_than(root, MAX_SYNTAX_DEPTH) {
        return Err(vec![diagnostics::nesting_too_deep(
            (node.start_byte(), node.end_byte()),
            MAX_SYNTAX_DEPTH,
        )]);
    }

    if root.has_error() {
        let mut errors = vec![];
        rust_sitter::errors::collect_parsing_errors(&root, source.as_bytes(), &mut errors);
        return Err(vec![diagnostics::from_schema_parse_failure(source, &errors)]);
    }

    // What `grammar::parse` does once the tree is known to be safe.
    Ok(<Document as rust_sitter::Extract<_>>::extract(Some(root), source.as_bytes(), 0, None))
}

/// The first node deeper than `limit` levels below `root`, found without
/// recursing.
fn deeper_than(root: Node<'_>, limit: usize) -> Option<Node<'_>> {
    let mut cursor = root.walk();
    let mut depth = 0usize;
    loop {
        if depth > limit {
            return Some(cursor.node());
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return None;
            }
            depth -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrays(levels: usize) -> String {
        format!("struct S {{\n    a: u8{}\n}}\n", "[]".repeat(levels))
    }

    #[test]
    fn a_schema_within_the_limit_parses() {
        let document = parse_schema(&arrays(60)).expect("60 levels of arrays parse");
        assert_eq!(document.0.len(), 1);
    }

    #[test]
    fn nesting_past_the_limit_is_an_error_not_a_stack_overflow() {
        // 10,000 levels overflowed an 8 MiB stack before this check.
        for levels in [100, 10_000] {
            let errors = parse_schema(&arrays(levels)).map(|_| ()).expect_err("too deep");
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].code.id, "parse::nesting-too-deep");
            assert!(errors[0].span.is_some());
        }
        let unions = format!("struct S {{\n    a: {}u8{}\n}}\n", "union(u16 ".repeat(5_000), ")".repeat(5_000));
        let errors = parse_schema(&unions).map(|_| ()).expect_err("too deep");
        assert_eq!(errors[0].code.id, "parse::nesting-too-deep");
    }

    #[test]
    fn the_check_runs_on_a_small_stack() {
        // What a wasm build or an editor's worker thread has.
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                assert!(parse_schema(&arrays(5_000)).is_err());
                assert!(parse_schema(&arrays(80)).is_ok());
            })
            .unwrap()
            .join()
            .expect("no stack overflow on 1 MiB");
    }

    #[test]
    fn long_flat_schemas_are_not_deep() {
        let fields: String = (0..5_000).map(|i| format!("    f{i}: u8\n")).collect();
        assert!(parse_schema(&format!("struct S {{\n{fields}}}\n")).is_ok());
    }

    #[test]
    fn syntax_errors_and_large_integers_still_report() {
        let errors = parse_schema("struct S {\n    / a: u8\n}\n").map(|_| ()).expect_err("a stray `/`");
        assert_eq!(errors[0].code.id, "parse::unexpected-token");
        let errors = parse_schema("const A: u64 = 99999999999999999999\n").map(|_| ()).expect_err("too large");
        assert_eq!(errors[0].code.id, "parse::integer-out-of-range");
    }
}
