//! Module docstrings: the `//!` lines a file starts with.
//!
//! A `///` docstring documents the declaration right after it, so a schema
//! (a module) and a package (`config.idp`) need a form of their own. They use
//! Rust's inner doc comment: `//!` lines at the top of the file.
//!
//! ```text
//! //! Types for talking HTTP.
//! //!
//! //! Plain data types: use them as fields and arguments.
//!
//! /// An HTTP request method.
//! enum HttpMethod { ... }
//! ```
//!
//! To the grammar a `//!` line is an ordinary comment (the `Comment` extra
//! accepts it, `DocLine` only matches `///`), so nothing in a build changes:
//! module docs exist for editors and documentation, which is why this lives
//! next to `vocabulary` and `annotations` rather than in the compiler.

/// The module docs `source` starts with, as markdown, or `None` when it has
/// none. `source` is a schema (`.ids`) or a package manifest (`config.idp`).
///
/// The docs are the `//!` lines before the file's first declaration (or
/// assignment). Blank lines and plain `//` comments (a license header, say)
/// may come before or between them. Anything else ends the header: a `///`
/// docstring belongs to the declaration it precedes, and a `//!` line after
/// code is just a comment.
///
/// Each line loses its `//!` and one following space, like Rust: further
/// indentation stays, so an indented code block survives. Blank `//!` lines
/// separate paragraphs, and the result has no leading or trailing blank lines.
pub fn module_docs(source: &str) -> Option<String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines: Vec<&str> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(doc) = trimmed.strip_prefix("//!") {
            lines.push(doc.strip_prefix(' ').unwrap_or(doc).trim_end());
        } else if trimmed.is_empty() || is_plain_comment(trimmed) {
            continue;
        } else {
            break;
        }
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let first = lines.iter().position(|line| !line.is_empty())?;
    Some(lines[first..].join("\n"))
}

/// A `//` comment that documents nothing: not a `///` docstring line (the
/// grammar's `DocLine`, which also takes `////`), and not a `//!` module doc.
fn is_plain_comment(line: &str) -> bool {
    line.strip_prefix("//").is_some_and(|rest| !rest.starts_with('/') && !rest.starts_with('!'))
}

/// The first non-blank line of `docs`: what a list of modules or declarations
/// shows next to each name.
pub fn summary(docs: &str) -> &str {
    docs.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::config::idl::grammar as config_grammar;
    use crate::schema::idl::grammar::{self, Declaration};

    #[test]
    fn reads_the_header_lines() {
        let source = "//! Types for talking HTTP.\n//!\n//! Plain data types.\n\nstruct A {\n    id: u64\n}\n";
        assert_eq!(module_docs(source).as_deref(), Some("Types for talking HTTP.\n\nPlain data types."));
    }

    #[test]
    fn none_without_a_header() {
        assert_eq!(module_docs(""), None);
        assert_eq!(module_docs("struct A {\n    id: u64\n}\n"), None);
        assert_eq!(module_docs("//!\n//!\nstruct A {}\n"), None, "only blank lines");
    }

    #[test]
    fn only_the_header_counts() {
        // A `///` docstring documents its declaration, not the module.
        assert_eq!(module_docs("/// About A\nstruct A {\n    id: u64\n}\n"), None);
        // `////` is a docstring line to the grammar too.
        assert_eq!(module_docs("//// banner\n//! Docs\n"), None);
        // `//!` after code is an ordinary comment.
        assert_eq!(module_docs("struct A {}\n//! later\n"), None);
        let source = "//! Docs\nstruct A {}\n//! later\n";
        assert_eq!(module_docs(source).as_deref(), Some("Docs"));
    }

    #[test]
    fn plain_comments_and_blank_lines_may_surround_the_header() {
        let source = "// Copyright someone\n\n//! First\n// not documentation\n\n//! Second\n\nuse a::B\n";
        assert_eq!(module_docs(source).as_deref(), Some("First\nSecond"));
    }

    #[test]
    fn keeps_indentation_for_code_blocks_and_drops_one_space() {
        let source = "//! Attach it:\n//!\n//!     @validators = [StringBounds(max_chars = 8)]\n//!no space\n";
        assert_eq!(
            module_docs(source).as_deref(),
            Some("Attach it:\n\n    @validators = [StringBounds(max_chars = 8)]\nno space")
        );
    }

    #[test]
    fn tolerates_crlf_and_a_byte_order_mark() {
        assert_eq!(module_docs("\u{feff}//! Docs\r\n//! More\r\n\r\nstruct A {}\r\n").as_deref(), Some("Docs\nMore"));
    }

    #[test]
    fn works_on_a_package_manifest() {
        let manifest = "//! The std package.\ncongregation std\nspecification_version = 1\n";
        assert_eq!(module_docs(manifest).as_deref(), Some("The std package."));
    }

    #[test]
    fn the_summary_is_the_first_nonblank_line() {
        assert_eq!(summary("\n  First line \nsecond"), "First line");
        assert_eq!(summary(""), "");
    }

    /// The header is a comment to both grammars, and a `///` right after it
    /// still documents the first declaration, not the module.
    #[test]
    fn the_header_is_just_a_comment_to_the_grammars() {
        let source = "//! Module docs\n//! more\n\n/// Thing docs\nstruct Thing {\n    id: u64\n}\n";
        let document = grammar::parse(source).expect("a schema with a module header parses");
        let Declaration::Struct(thing) = &document.0[0].value else { panic!("expected a struct") };
        assert_eq!(thing.docstring().as_deref(), Some("Thing docs"));
        assert_eq!(module_docs(source).as_deref(), Some("Module docs\nmore"));

        config_grammar::parse("//! A package.\n//! More.\ncongregation app\nspecification_version = 1\n")
            .expect("a manifest with a header parses");
    }
}
