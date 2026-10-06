//! Human-readable docs for the settings-key shapes
//! `schema::ir::compiler::settings::enforcement` actually interprets. That
//! module is the authority on the resolution logic (candidate-key order,
//! decl-path scoping) - this module mirrors its candidate shapes for
//! editor hover and completion, nothing more. If `enforcement`'s candidate
//! lists ever change, this catalog needs to change with them.

/// One recognized settings-key shape, for display and completion.
/// `key_pattern` is the canonical dotted form (`<name>` standing in for a
/// user-chosen annotation/validator name) — completion inserts it
/// directly (as a snippet where `<name>` is a tab-stop); hover shows it
/// alongside `summary`/`description`.
pub struct FieldDoc {
    pub key_pattern: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
}

/// A dotted settings key matched against a recognized shape, independent
/// of whatever prefix (decl-path) precedes it.
pub enum Match<'a> {
    Mode,
    AnnotationAllowed { name: &'a str },
    ValidatorAllowed { name: &'a str },
    ValidatorAllowedCoarse,
}

/// Suffix match over a dotted key's segments. Mirrors exactly the
/// candidate-key shapes `enforcement::check_annotation`/`check_validator_ref`
/// build (most-specific decl-scoped key down to the global fallback) and
/// `resolution`'s reserved `mode` key - this function doesn't care about
/// scoping, only the trailing shape.
///
/// Known, accepted limitation: a validator or annotation literally *named*
/// `"validators"` or `"annotations"` can collide with the sibling keyword
/// one level up (e.g. a validator named `validators` makes
/// `validators.validators.allowed` ambiguous with "coarse + irrelevant
/// extra segment"). This can't be resolved by a prefix-agnostic suffix
/// matcher. `enforcement` itself is immune, since it only ever constructs
/// candidate keys forward from a known name, never reverse-parses one.
pub fn match_path<'a>(segments: &[&'a str]) -> Option<Match<'a>> {
    let n = segments.len();
    if n == 0 {
        return None;
    }
    if segments[n - 1] == "mode" {
        return Some(Match::Mode);
    }
    if segments[n - 1] != "allowed" || n < 2 {
        return None;
    }
    if segments[n - 2] == "validators" {
        return Some(Match::ValidatorAllowedCoarse);
    }
    if n < 3 {
        return None;
    }
    match segments[n - 3] {
        "validators" => Some(Match::ValidatorAllowed { name: segments[n - 2] }),
        "annotations" => Some(Match::AnnotationAllowed { name: segments[n - 2] }),
        _ => None,
    }
}

/// The full prose for one matched shape.
pub fn doc_for(m: &Match) -> FieldDoc {
    match m {
        Match::Mode => FieldDoc {
            key_pattern: "mode",
            summary: "merge behavior for this subtree",
            description: "Reserved key. Its value must be the bare keyword `replace` - \
                when present, this subtree replaces the corresponding subtree from the \
                layer below instead of merging into it. Absent (the default) means merge.",
        },
        Match::AnnotationAllowed { .. } => FieldDoc {
            key_pattern: "annotations.<name>.allowed",
            summary: "whether this annotation is permitted here",
            description: "Whether the named annotation is permitted on declarations this \
                key's scope covers. `true` allows it, `false` forbids it (a hard build \
                error naming the annotation and this key). No key at any level means \
                allowed by default.",
        },
        Match::ValidatorAllowed { .. } => FieldDoc {
            key_pattern: "validators.<name>.allowed",
            summary: "whether this validator is permitted here",
            description: "Whether the named validator is permitted on declarations this \
                key's scope covers. `true` allows it, `false` forbids it (a hard build \
                error naming the validator and this key). No key at any level means \
                allowed by default.",
        },
        Match::ValidatorAllowedCoarse => FieldDoc {
            key_pattern: "validators.allowed",
            summary: "whether any validator is permitted here",
            description: "Whether validators in general are permitted on declarations \
                this key's scope covers, when no more specific `validators.<name>.allowed` \
                key matches first. `true` allows, `false` forbids (a hard build error \
                naming the validator and this key).",
        },
    }
}

/// The recognized shapes, as a flat listing - for "what's configurable
/// here" hover on a settings block/group itself, independent of any one
/// written key.
pub fn block_summary() -> &'static [FieldDoc] {
    &[
        FieldDoc {
            key_pattern: "mode",
            summary: "merge behavior for this subtree",
            description: "Reserved key. Its value must be the bare keyword `replace` - \
                when present, this subtree replaces the corresponding subtree from the \
                layer below instead of merging into it. Absent (the default) means merge.",
        },
        FieldDoc {
            key_pattern: "annotations.<name>.allowed",
            summary: "whether an annotation is permitted",
            description: "Whether the named annotation is permitted on declarations this \
                scope covers. `true` allows it, `false` forbids it. No key means allowed \
                by default.",
        },
        FieldDoc {
            key_pattern: "validators.<name>.allowed",
            summary: "whether a specific validator is permitted",
            description: "Whether the named validator is permitted on declarations this \
                scope covers. `true` allows it, `false` forbids it. No key means allowed \
                by default.",
        },
        FieldDoc {
            key_pattern: "validators.allowed",
            summary: "whether validators in general are permitted",
            description: "Whether validators in general are permitted on declarations \
                this scope covers, when no more specific `validators.<name>.allowed` key \
                matches first. `true` allows, `false` forbids.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(m: &Match) -> &'static str {
        match m {
            Match::Mode => "mode",
            Match::AnnotationAllowed { .. } => "annotation_allowed",
            Match::ValidatorAllowed { .. } => "validator_allowed",
            Match::ValidatorAllowedCoarse => "validator_allowed_coarse",
        }
    }

    #[test]
    fn matches_bare_mode() {
        let m = match_path(&["mode"]).unwrap();
        assert_eq!(names(&m), "mode");
    }

    #[test]
    fn matches_nested_mode() {
        let m = match_path(&["struct", "mode"]).unwrap();
        assert_eq!(names(&m), "mode");
    }

    #[test]
    fn matches_decl_scoped_annotation_allowed() {
        let m = match_path(&["struct", "annotations", "foo", "allowed"]).unwrap();
        assert_eq!(names(&m), "annotation_allowed");
        match m {
            Match::AnnotationAllowed { name } => assert_eq!(name, "foo"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn matches_global_annotation_allowed() {
        let m = match_path(&["annotations", "foo", "allowed"]).unwrap();
        match m {
            Match::AnnotationAllowed { name } => assert_eq!(name, "foo"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn matches_decl_scoped_named_validator_allowed() {
        let m = match_path(&["protocol", "function", "validators", "Bar", "allowed"]).unwrap();
        match m {
            Match::ValidatorAllowed { name } => assert_eq!(name, "Bar"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn matches_coarse_validators_allowed() {
        let m = match_path(&["validators", "allowed"]).unwrap();
        assert_eq!(names(&m), "validator_allowed_coarse");
    }

    #[test]
    fn matches_decl_scoped_coarse_validators_allowed() {
        let m = match_path(&["struct", "field", "validators", "allowed"]).unwrap();
        assert_eq!(names(&m), "validator_allowed_coarse");
    }

    #[test]
    fn no_match_for_unrelated_key() {
        assert!(match_path(&["struct", "field"]).is_none());
    }

    #[test]
    fn no_coarse_form_for_annotations() {
        // Unlike validators, enforcement.rs never builds a bare
        // "annotations.allowed" candidate - this asymmetry is real, not
        // a missing pattern.
        assert!(match_path(&["annotations", "allowed"]).is_none());
    }

    #[test]
    fn no_match_for_empty_path() {
        assert!(match_path(&[]).is_none());
    }
}
