//! Language editions (core#102).
//!
//! A package's `config.idp` names the edition its schemas are written in,
//! `specification_version = 1`. A change that would break schemas already
//! written (a keyword renamed or removed, a default changed) ships in a new
//! edition, and a package keeps its old behaviour until it moves to that
//! edition. Packages on different editions still depend on each other: the
//! compiled schema doesn't depend on the edition, so a package's dependents
//! never see which one it's written in.
//!
//! Every package states its edition; a missing or unknown one is an error
//! with a fix ([`check_config`]). What a new edition deprecates or removes is
//! marked on the language's words in [`vocabulary`](crate::schema::idl::vocabulary),
//! and [`deprecations`] reports their uses. `comline fix --edition N` applies
//! the fixes and moves the package.

use std::fmt;

/// An edition of the schema language. Ordered: a later edition compares
/// greater, so a change reads `if edition >= Edition::E2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Edition {
    /// The first edition: the language as it is at 1.0.
    E1 = 1,
}

impl Edition {
    /// Every edition this core knows, oldest first.
    pub const ALL: &'static [Edition] = &[Edition::E1];

    /// The newest edition, which `comline new` writes.
    pub const LATEST: Edition = Edition::E1;

    /// The number written in `config.idp`.
    pub fn number(self) -> u8 {
        self as u8
    }

    /// The edition `config.idp` names, if this core knows it.
    pub fn from_number(number: u8) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.number() == number)
    }

    /// The known editions as text, for a message: `edition 1`, or
    /// `editions 1, 2 and 3`.
    pub fn known() -> String {
        let numbers: Vec<String> = Self::ALL.iter().map(|e| e.number().to_string()).collect();
        match numbers.split_last() {
            Some((last, [])) => format!("edition {last}"),
            Some((last, rest)) => format!("editions {} and {last}", rest.join(", ")),
            None => String::new(),
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.number())
    }
}

#[cfg(feature = "compiler")]
pub use checks::*;

#[cfg(feature = "compiler")]
mod checks {
    use super::Edition;
    use crate::diagnostics::{Diagnostic, DiagnosticCode, Fix};
    use crate::package::config::idl::grammar::{Congregation, Key, Value};
    use crate::schema::idl::vocabulary::{self, Deprecation};

    pub const MISSING: DiagnosticCode = DiagnosticCode::error("edition::missing");
    pub const UNKNOWN: DiagnosticCode = DiagnosticCode::error("edition::unknown");
    /// A word the package's edition removed. Its deprecation's own code is a
    /// warning; this is the same use once it no longer builds.
    pub const REMOVED: DiagnosticCode = DiagnosticCode::error("edition::removed");

    /// The edition `config.idp` declares, or why it can't be used: no
    /// `specification_version`, or a number this core doesn't know. Either
    /// way the diagnostic carries a fix, to add the line or to name the
    /// newest edition.
    pub fn check_config(source: &str, config: &Congregation) -> Result<Edition, Diagnostic> {
        let declared = config.assignments.iter().find(|a| {
            matches!(&a.value.key, Key::Identifier(i) if i.value == "specification_version")
        });
        let Some(assignment) = declared else {
            return Err(missing(source));
        };
        let span = (assignment.span.0, assignment.span.1);
        let written = &source[span.0..span.1];
        let value_span = match written.find('=') {
            Some(eq) => {
                let after = &written[eq + 1..];
                let start = span.0 + eq + 1 + (after.len() - after.trim_start().len());
                (start, span.0 + written.trim_end().len())
            }
            None => span,
        };
        let number = match &assignment.value.value {
            Value::Number(n) => n.value.parse::<u64>().ok(),
            _ => None,
        };
        match number.and_then(|n| u8::try_from(n).ok()).and_then(Edition::from_number) {
            Some(edition) => Ok(edition),
            None => Err(unknown(&source[value_span.0..value_span.1], number, value_span)),
        }
    }

    fn missing(source: &str) -> Diagnostic {
        let latest = Edition::LATEST;
        // At the end of the `congregation <name>` line, where `comline new`
        // puts it: before that line's newline, so the file's own line
        // endings stay as they are.
        let line = congregation_line(source);
        let (at, text) = match line {
            Some((_, end)) => (end, format!("\nspecification_version = {latest}")),
            None => (0, format!("specification_version = {latest}\n")),
        };
        Diagnostic::new(MISSING, "config.idp doesn't say which edition its schemas are written in")
            .with_context("no `specification_version`")
            .maybe_span(line)
            .with_help(format!(
                "add `specification_version = {latest}` (the newest edition; this Comline knows {})",
                Edition::known()
            ))
            .with_fix(Fix::replace(format!("Add `specification_version = {latest}`"), (at, at), text))
    }

    fn unknown(written: &str, number: Option<u64>, span: (usize, usize)) -> Diagnostic {
        let latest = Edition::LATEST;
        let newer = number.is_some_and(|n| n > u64::from(latest.number()));
        let help = if newer {
            format!(
                "this Comline knows {}; a newer one may know {written}, or use {latest}",
                Edition::known()
            )
        } else {
            format!("this Comline knows {}", Edition::known())
        };
        Diagnostic::new(
            UNKNOWN,
            format!("`specification_version = {written}` isn't an edition this Comline knows"),
        )
        .with_context("unknown edition")
        .with_span(span)
        .with_help(help)
        .with_fix(Fix::replace(format!("Use edition {latest}"), span, latest.to_string()))
    }

    /// The span of the `congregation <name>` line, without its newline.
    fn congregation_line(source: &str) -> Option<(usize, usize)> {
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let content = line.trim_end_matches(['\n', '\r']);
            if content.trim_start().starts_with("congregation") {
                return Some((offset, offset + content.len()));
            }
            offset += line.len();
        }
        None
    }

    /// Every use in `source` of a word `edition` deprecates or removes,
    /// from the words in `table` (the language's own: [`vocabulary::DEPRECATIONS`]).
    /// A deprecated word is its deprecation's warning; a removed one is an
    /// [`REMOVED`] error. Each carries a fix when the word has a
    /// replacement.
    ///
    /// Only real tokens count: a word inside a comment, a string or a longer
    /// name isn't a use. `source` must parse; a schema that doesn't gets its
    /// parse errors instead.
    pub fn deprecations(source: &str, edition: Edition, table: &[Deprecation]) -> Vec<Diagnostic> {
        let active: Vec<&Deprecation> = table.iter().filter(|d| edition >= d.since).collect();
        if active.is_empty() {
            return Vec::new();
        }
        let mut parser = rust_sitter::tree_sitter::Parser::new();
        parser
            .set_language(&crate::schema::idl::grammar::language())
            .expect("the schema grammar's language loads");
        let Some(tree) = parser.parse(source, None) else {
            return Vec::new();
        };

        let mut found = Vec::new();
        let mut cursor = tree.walk();
        loop {
            let node = cursor.node();
            // A keyword or primitive type is a leaf of its own whose text
            // is the word. Leaves of free text (names, paths, literals,
            // comments, an f-string's text) can hold the same letters
            // without being a use, so they never match.
            let text = &source[node.start_byte()..node.end_byte()];
            if node.child_count() == 0 && !is_free_text(node.kind()) {
                if let Some(d) = active.iter().find(|d| d.word == text) {
                    found.push(report(d, edition, (node.start_byte(), node.end_byte())));
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return found;
                }
            }
        }
    }

/// The grammar's leaves that hold free text rather than a fixed word, by
    /// the struct rust_sitter names their kind after (`Identifier_text`).
    fn is_free_text(kind: &str) -> bool {
        const FREE_TEXT: &[&str] = &[
            "Identifier", "ScopedIdentifier", "ScopedPath", "InterpolatedText", "StringLiteral",
            "IntegerLiteral", "Comment", "Docstring", "Whitespace",
        ];
        FREE_TEXT.contains(&kind.split('_').next().unwrap_or(kind))
    }

    fn report(d: &Deprecation, edition: Edition, span: (usize, usize)) -> Diagnostic {
        let removed = d.removed.is_some_and(|r| edition >= r);
        let (code, message) = if removed {
            (REMOVED, format!("`{}` was removed in edition {}", d.word, d.removed.unwrap()))
        } else {
            let when = match d.removed {
                Some(r) => format!(", and edition {r} removes it"),
                None => String::new(),
            };
            (d.code, format!("`{}` is deprecated since edition {}{when}", d.word, d.since))
        };
        let mut diagnostic = Diagnostic::new(code, message)
            .with_context(if removed { "removed" } else { "deprecated" })
            .with_span(span)
            .with_help(d.note);
        if let Some(replacement) = d.replacement {
            diagnostic = diagnostic.with_fix(Fix::replace(
                format!("Replace `{}` with `{replacement}`", d.word),
                span,
                replacement,
            ));
        }
        diagnostic
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::diagnostics::Severity;

        fn config(source: &str) -> Congregation {
            crate::package::config::idl::grammar::parse(source).expect("the config parses")
        }

        fn fixed(source: &str, d: &Diagnostic) -> String {
            d.fix.as_ref().expect("a fix").apply(source)
        }

        #[test]
        fn a_known_edition_is_read() {
            let source = "congregation app\nspecification_version = 1\n";
            assert_eq!(check_config(source, &config(source)), Ok(Edition::E1));
        }

        #[test]
        fn a_missing_edition_is_an_error_whose_fix_adds_the_line() {
            let source = "//! An app.\ncongregation app\nid = \"x\"\n";
            let d = check_config(source, &config(source)).unwrap_err();
            assert_eq!(d.code, MISSING);
            assert_eq!(&source[d.span.unwrap().0..d.span.unwrap().1], "congregation app");
            assert_eq!(fixed(source, &d), "//! An app.\ncongregation app\nspecification_version = 1\nid = \"x\"\n");
            let again = fixed(source, &d);
            assert_eq!(check_config(&again, &config(&again)), Ok(Edition::E1));
        }

        #[test]
        fn a_missing_edition_at_the_end_of_a_file_without_a_newline() {
            let source = "congregation app";
            let d = check_config(source, &config(source)).unwrap_err();
            let again = fixed(source, &d);
            assert_eq!(again, "congregation app\nspecification_version = 1");
            assert_eq!(check_config(&again, &config(&again)), Ok(Edition::E1));
        }

        #[test]
        fn an_unknown_edition_points_at_the_number_and_offers_the_newest() {
            for (written, newer) in [("0", false), ("2", true), ("300", true)] {
                let source = format!("congregation app\nspecification_version = {written}\n");
                let d = check_config(&source, &config(&source)).unwrap_err();
                assert_eq!(d.code, UNKNOWN);
                assert_eq!(&source[d.span.unwrap().0..d.span.unwrap().1], written);
                assert_eq!(d.help.as_deref().unwrap().contains("newer one"), newer, "{written}");
                assert_eq!(fixed(&source, &d), "congregation app\nspecification_version = 1\n");
            }
        }

        #[test]
        fn a_value_that_isnt_a_number_is_unknown() {
            let source = "congregation app\nspecification_version = \"one\"\n";
            let d = check_config(source, &config(source)).unwrap_err();
            assert_eq!(d.code, UNKNOWN);
            assert_eq!(&source[d.span.unwrap().0..d.span.unwrap().1], "\"one\"");
        }

        #[test]
        fn known_lists_every_edition() {
            assert_eq!(Edition::known(), "edition 1");
            assert_eq!(Edition::from_number(1), Some(Edition::E1));
            assert_eq!(Edition::from_number(0), None);
            assert_eq!(Edition::LATEST, *Edition::ALL.last().unwrap());
        }

        // Edition 1 deprecates nothing yet, so these use a table of their
        // own, as if a later edition had renamed `u8` (a primitive) and
        // removed `optional` (a keyword). Both editions are written as E1:
        // the comparison is what's under test.
        const RENAMED: DiagnosticCode = DiagnosticCode::warning("deprecation::u8");
        const DROPPED: DiagnosticCode = DiagnosticCode::warning("deprecation::optional");
        const TABLE: &[Deprecation] = &[
            Deprecation { word: "u8", since: Edition::E1, removed: None, replacement: Some("byte"), code: RENAMED, note: "`byte` says what it is" },
            Deprecation { word: "optional", since: Edition::E1, removed: Some(Edition::E1), replacement: None, code: DROPPED, note: "write `T?`" },
        ];

        #[test]
        fn a_deprecated_word_is_a_warning_with_its_replacement() {
            let source = "struct S {\n    a: u8\n    b: u8[max 4]\n    u8_count: u16\n}\n// u8 in a comment\nconst C: string = \"u8\"\nerror E {\n    message = f\"{a}u8\"\n    a: u16\n}\n";
            let found = deprecations(source, Edition::E1, &TABLE[..1]);
            assert_eq!(found.len(), 2, "{found:#?}");
            for d in &found {
                assert_eq!(d.code, RENAMED);
                assert_eq!(d.effective_severity(), Severity::Warning);
                assert_eq!(&source[d.span.unwrap().0..d.span.unwrap().1], "u8");
            }
            assert_eq!(fixed(source, &found[0]), source.replacen("a: u8", "a: byte", 1));
        }

        #[test]
        fn a_removed_word_is_an_error() {
            let source = "struct S {\n    optional a: u16\n}\n";
            let found = deprecations(source, Edition::E1, &TABLE[1..]);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].code, REMOVED);
            assert_eq!(found[0].effective_severity(), Severity::Error);
            assert!(found[0].fix.is_none());
        }

        #[test]
        fn every_deprecated_word_is_a_word_of_the_language() {
            for d in vocabulary::DEPRECATIONS {
                assert!(
                    vocabulary::keyword(d.word).is_some() || vocabulary::primitive(d.word).is_some(),
                    "`{}` isn't a keyword or a primitive type",
                    d.word
                );
                assert!(d.code.id == format!("deprecation::{}", d.word), "{}", d.code.id);
                assert!(d.removed.is_none_or(|r| r > d.since), "`{}` is removed before it's deprecated", d.word);
            }
        }

        #[test]
        fn the_language_deprecates_nothing_yet() {
            let source = "struct S {\n    optional a: u8\n    s: string\n}\n";
            assert!(deprecations(source, Edition::LATEST, vocabulary::DEPRECATIONS).is_empty());
        }
    }
}
