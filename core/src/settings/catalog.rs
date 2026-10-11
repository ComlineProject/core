//! Editor-facing view of the settings-key registry: hover prose and
//! completion listings, derived from [`super::keys::SHAPES`].
//!
//! This module used to be a hand-maintained *mirror* of
//! `schema::ir::compiler::settings::enforcement`'s candidate lists, with
//! its own doc warning that it had to be updated by hand whenever those
//! changed, and its own prefix-agnostic suffix matcher that could not tell
//! a validator named `validators` from the coarse form. [`super::keys`] is
//! now the single authority for both the shapes and the matching; what is
//! left here is the adapter the language server already calls.

use std::sync::LazyLock;

use super::keys;

pub use super::keys::{doc_for, match_path, Match};

/// One recognized settings-key shape, for display and completion.
/// `key_pattern` is the canonical dotted form (`<name>`/`<code>` standing
/// in for a user-chosen name) - completion inserts it as a snippet with
/// the placeholders as tab-stops; hover shows it alongside
/// `summary`/`description`.
pub type FieldDoc = keys::KeyDoc;

static BLOCK_SUMMARY: LazyLock<Vec<FieldDoc>> = LazyLock::new(keys::listing);

/// The recognized shapes, as a flat listing - for "what's configurable
/// here" hover on a settings block/group itself, independent of any one
/// written key.
pub fn block_summary() -> &'static [FieldDoc] {
    &BLOCK_SUMMARY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_value::ValueType;

    #[test]
    fn block_summary_lists_every_shape_in_registry_order() {
        let patterns: Vec<&str> =
            block_summary().iter().map(|d| d.key_pattern.as_str()).collect();
        assert_eq!(
            patterns,
            vec![
                "mode",
                "annotations.<name>.allowed",
                "validators.<name>.allowed",
                "validators.allowed",
                "diagnostics.<code>.level",
            ]
        );
    }

    #[test]
    fn block_summary_is_stable_across_calls() {
        // It is a `LazyLock`, so the language server can hold the slice.
        assert!(std::ptr::eq(block_summary(), block_summary()));
    }

    #[test]
    fn every_listed_entry_carries_prose_and_a_value_type() {
        for doc in block_summary() {
            assert!(!doc.summary.is_empty(), "{}", doc.key_pattern);
            assert!(!doc.description.is_empty(), "{}", doc.key_pattern);
        }
        // The editor used to assume every settings key took a boolean;
        // the listing now says otherwise for the one that doesn't.
        let level = block_summary()
            .iter()
            .find(|d| d.key_pattern == "diagnostics.<code>.level")
            .unwrap();
        assert!(matches!(level.value, ValueType::StrEnum(_)));
    }

    #[test]
    fn hover_on_a_written_key_still_resolves_to_its_prose() {
        let segments = ["annotations", "framing", "allowed"];
        let m = match_path(&segments).unwrap();
        let doc = doc_for(&m);
        assert_eq!(doc.key_pattern, "annotations.<name>.allowed");
        assert!(doc.description.contains("built-in default"));
    }
}
