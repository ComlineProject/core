// Drift guards for `schema::idl::vocabulary` — the canonical primitive/
// keyword list — against the grammar it describes.

use std::collections::BTreeSet;

use comline_core::schema::idl::grammar::{self, Declaration, Type};
use comline_core::schema::idl::size::{self, SizeLookup, SizeTarget, WireSize};
use comline_core::schema::idl::vocabulary;

#[cfg(test)]
mod vocabulary_tests {
    use super::*;

    /// Structural drift guard: every `leaf(text = "...")` identifier-shaped
    /// token in the grammar source must be listed in the vocabulary, and
    /// vice versa. Catches additions (a new keyword nobody added to the
    /// table) as well as removals/renames — a parse-based test alone can't
    /// catch an addition, since an unlisted word just silently stays
    /// unclassified.
    #[test]
    fn every_grammar_leaf_text_token_is_in_the_vocabulary() {
        let source = include_str!("../../src/schema/idl/grammar.rs");
        let re = regex::Regex::new(r#"leaf\s*\(\s*text\s*=\s*"([A-Za-z0-9_]+)""#).unwrap();
        let grammar_tokens: BTreeSet<&str> =
            re.captures_iter(source).map(|c| c.get(1).unwrap().as_str()).collect();

        let vocab_tokens: BTreeSet<&str> = vocabulary::all_word_tokens().collect();

        assert_eq!(
            grammar_tokens, vocab_tokens,
            "grammar.rs's identifier-shaped leaf(text=...) tokens and \
             vocabulary::all_word_tokens() have drifted apart"
        );
    }

    fn first_field_type(code: &str) -> Type {
        let doc = grammar::parse(code).unwrap_or_else(|e| panic!("{code:?} failed to parse: {e:?}"));
        let Declaration::Struct(s) = &*doc.0[0] else {
            panic!("expected a struct declaration, got {:?}", doc.0[0])
        };
        s.fields()[0].field_type().clone()
    }

    /// Semantic drift guard: a listed primitive must parse as its own
    /// `Type` variant, not fall through to `Type::Named` (an arbitrary
    /// identifier). This is the actual regression check for the bug that
    /// started this module: `i32` "parses" today too, just as a bare name.
    #[test]
    fn every_primitive_parses_as_a_real_primitive_variant() {
        for p in vocabulary::PRIMITIVES {
            let ty = first_field_type(&format!("struct S {{ f: {} }}", p.name));
            assert!(
                !matches!(ty, Type::Named(_)),
                "{} parsed as Type::Named — not recognised as a real primitive",
                p.name
            );
        }
    }

    /// The inverse: these look like primitives but are not Comline syntax,
    /// so they must fall through to `Type::Named`.
    #[test]
    fn non_primitives_fall_through_to_named() {
        for fake in ["i8", "i16", "i32", "i64", "int", "float"] {
            let ty = first_field_type(&format!("struct S {{ f: {fake} }}"));
            assert!(
                matches!(ty, Type::Named(_)),
                "{fake} unexpectedly parsed as a real primitive variant"
            );
        }
    }

    /// Every keyword's `example` is a real fixture, not documentation
    /// prose: it must actually parse, and must actually contain the
    /// keyword as a whole word (not just claim to demonstrate it).
    #[test]
    fn every_keyword_example_parses_and_demonstrates_the_keyword() {
        for k in vocabulary::KEYWORDS {
            grammar::parse(k.example)
                .unwrap_or_else(|e| panic!("{}'s example failed to parse: {e:?}", k.text));
            let is_whole_word = k
                .example
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == k.text);
            assert!(is_whole_word, "{}'s own example doesn't contain it as a whole word", k.text);
        }
    }

    struct NoLookup;
    impl SizeLookup for NoLookup {
        fn resolve(&self, _bare_name: &str) -> Option<SizeTarget<'_>> {
            None
        }
    }

    /// Binds `vocabulary` and `size` without refactoring either: a
    /// primitive's declared bit width must agree with what the wire-size
    /// estimator actually computes for it.
    #[test]
    fn primitive_bits_agree_with_size_of_type() {
        for p in vocabulary::PRIMITIVES {
            let ty = first_field_type(&format!("struct S {{ f: {} }}", p.name));
            let got = size::size_of_type(&ty, &NoLookup);
            match p.bits {
                Some(bits) => {
                    assert_eq!(got, WireSize::Fixed((bits / 8) as u32), "{}", p.name)
                }
                None => assert_eq!(got, WireSize::Variable, "{}", p.name),
            }
        }
    }

    #[test]
    fn primitives_list_has_no_duplicates() {
        let mut seen = BTreeSet::new();
        for p in vocabulary::PRIMITIVES {
            assert!(seen.insert(p.name), "duplicate primitive: {}", p.name);
        }
    }

    #[test]
    fn keywords_list_has_no_duplicates() {
        let mut seen = BTreeSet::new();
        for k in vocabulary::KEYWORDS {
            assert!(seen.insert(k.text), "duplicate keyword: {}", k.text);
        }
    }

    #[test]
    fn lookup_functions_work() {
        assert!(vocabulary::primitive("s32").is_some());
        assert!(vocabulary::primitive("i32").is_none());
        assert!(vocabulary::keyword("struct").is_some());
        assert!(vocabulary::keyword("nope").is_none());
    }
}
