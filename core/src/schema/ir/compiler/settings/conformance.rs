//! Checks a frozen `settings` dictionary against the key registry: every
//! key recognised, every value the type its key declares.
//!
//! Two holes this closes, both of which used to let a mistake build clean:
//!
//! - An unrecognised key was simply ignored. `annotations.framng.alowed =
//!   true` compiled with no complaint and no effect, because nothing ever
//!   enumerated the keys - `resolution` checked only dotted-key shape
//!   consistency and `mode`'s value.
//! - A wrongly-typed value was caught only if some declaration happened to
//!   exercise it. `validators.allowed = Tru` in a schema referencing no
//!   validators was accepted, since `enforcement` is the only thing that
//!   looked and it only looks at keys a declaration reaches.
//!
//! # Why unknown keys are only an error inside known groups
//!
//! `settings` stays an **open namespace at the top level**: a key that
//! never enters a group this registry owns is left alone, so a package can
//! carry a forward-looking or experimental key without core rejecting it
//! (core's own fixtures do exactly that). Once a key enters
//! `annotations.`/`validators.`/`diagnostics.`, or is a reserved word,
//! core defines the whole shape and a deviation is a typo rather than an
//! extension.
//!
//! That boundary is also what keeps this pass safe to make a hard error.
//! `settings::unknown-key` is Error severity, and `severity` refuses to
//! soften an Error-severity code, so there is deliberately no
//! `diagnostics."settings::unknown-key".level = "allow"` escape hatch - a
//! false positive would be an unfixable build break. Restricting the check
//! to shapes core fully defines, plus
//! `keys::tests::all_shapes_are_pairwise_disjoint` guaranteeing each key
//! has at most one parse, is what makes that acceptable.

use crate::config_value;
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::settings::keys::{self, DeclPath, Shape};
use crate::settings::value::{SettingsDict, SettingsSpans, SettingsValue};
use crate::utils::text::closest;

const UNKNOWN_KEY: DiagnosticCode = DiagnosticCode::error("settings::unknown-key");

/// Check every leaf in `dict`. `label` describes where the dictionary came
/// from, for the diagnostic's context (`"the unnamed settings block"`,
/// `"the package's 'settings'"`). `fallback_span` is used for a leaf whose
/// own origin `spans` doesn't record - `.idp` settings aren't span-tracked
/// yet.
///
/// Collects every problem rather than stopping at the first.
pub fn check_settings_conformance(
    dict: &SettingsDict,
    spans: &SettingsSpans,
    label: &str,
    fallback_span: Option<(usize, usize)>,
) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut path = Vec::new();
    walk(dict, &mut path, spans, label, fallback_span, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn walk(
    dict: &SettingsDict,
    path: &mut Vec<String>,
    spans: &SettingsSpans,
    label: &str,
    fallback_span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    for (key, value) in &dict.0 {
        path.push(key.clone());

        if let SettingsValue::Dict(sub) = value {
            walk(sub, path, spans, label, fallback_span, errors);
        } else {
            check_leaf(path, value, spans, label, fallback_span, errors);
        }

        path.pop();
    }
}

fn check_leaf(
    path: &[String],
    value: &SettingsValue,
    spans: &SettingsSpans,
    label: &str,
    fallback_span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    let segments: Vec<&str> = path.iter().map(String::as_str).collect();
    let dotted = path.join(".");

    // Outside the groups core owns, `settings` is an open namespace.
    if !keys::enters_known_group(&segments) {
        return;
    }

    let span = spans.get(&dotted).or(fallback_span);

    let Some(matched) = keys::match_path(&segments) else {
        let mut d = Diagnostic::new(
            UNKNOWN_KEY,
            format!("unrecognised settings key '{dotted}'"),
        )
        .with_context(format!("in {label}"))
        .maybe_span(span);
        d = match suggestion(&segments) {
            Some(s) => d.with_help(format!(
                "did you mean '{s}'? as written, this key has no effect"
            )),
            None => d.with_help(
                "this key has no effect - check the recognised shapes for this group",
            ),
        };
        errors.push(d);
        return;
    };

    if let Err(mismatch) = config_value::check(matched.value_type(), value.form()) {
        // The code comes from the shape, so this pass and `severity` can
        // never invent different codes for one mistake. A reserved word
        // also carries its own advice.
        let help = match matched.shape {
            Shape::Reserved(r) => format!(
                "'{}' only accepts {} - remove it to merge (the default) instead",
                r.word, mismatch.expected
            ),
            Shape::Path(_) => format!("'{dotted}' accepts {}", mismatch.expected),
        };
        errors.push(
            Diagnostic::new(matched.malformed_code(), mismatch.message(&dotted))
                .with_context(format!("in {label}"))
                .with_help(help)
                .maybe_span(span),
        );
    }
}

/// The nearest recognised key to a written one, for "did you mean?".
///
/// Candidates are built by reading the written key's *own* segments into
/// each shape's holes, so the suggestion differs from the input only where
/// the author actually went wrong - `annotations.framng.alowed` suggests
/// `annotations.framng.allowed`, pointing at the misspelled leaf rather
/// than replacing the name they chose deliberately.
fn suggestion(segments: &[&str]) -> Option<String> {
    let written = segments.join(".");
    let (decl, rest) = DeclPath::split_prefix(segments);
    let mut candidates = Vec::new();

    for shape in keys::SHAPES {
        match shape {
            Shape::Reserved(r) => {
                let mut with_word: Vec<&str> = segments[..segments.len() - 1].to_vec();
                with_word.push(r.word);
                candidates.push(with_word.join("."));
            }
            Shape::Path(p) => {
                let scope = decl.filter(|_| p.scoping != keys::Scoping::GlobalOnly);
                // Whatever sits where this shape's name segment would go.
                let name = rest.get(1).copied().unwrap_or("name");
                candidates.push(p.key(scope, Some(name)));
                if p.coarse {
                    candidates.push(p.key(scope, None));
                }
            }
        }
    }

    closest(&written, candidates.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::desugar::desugar_lenient;

    fn dict(entries: &[(&str, SettingsValue)]) -> SettingsDict {
        let owned: Vec<(Vec<String>, SettingsValue)> = entries
            .iter()
            .map(|(k, v)| (k.split('.').map(str::to_string).collect(), v.clone()))
            .collect();
        desugar_lenient(&owned)
    }

    fn check(entries: &[(&str, SettingsValue)]) -> Result<(), Vec<Diagnostic>> {
        check_settings_conformance(
            &dict(entries),
            &SettingsSpans::new(),
            "the unnamed settings block",
            None,
        )
    }

    fn b(v: bool) -> SettingsValue {
        SettingsValue::Bool(v)
    }

    #[test]
    fn every_recognised_key_passes() {
        check(&[
            ("annotations.framing.allowed", b(true)),
            ("validators.allowed", b(false)),
            ("validators.StringBounds.allowed", b(true)),
            ("struct.field.validators.allowed", b(true)),
            ("protocol.function.annotations.timeout_ms.allowed", b(true)),
            ("diagnostics.validation::unknown-type.level", SettingsValue::Str("warn".into())),
            ("mode", SettingsValue::Identifier("replace".into())),
            ("validators.mode", SettingsValue::Identifier("replace".into())),
        ])
        .expect("all of these are recognised shapes");
    }

    #[test]
    fn a_misspelled_leaf_inside_a_known_group_is_an_error() {
        let errors = check(&[("annotations.framing.alowed", b(true))]).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code.id, "settings::unknown-key");
        assert!(errors[0].message.contains("annotations.framing.alowed"));
    }

    #[test]
    fn the_suggestion_corrects_the_typo_without_replacing_the_authors_name() {
        let errors = check(&[("annotations.framng.alowed", b(true))]).unwrap_err();
        let help = errors[0].help.as_deref().unwrap();
        assert!(
            help.contains("annotations.framng.allowed"),
            "should fix the leaf and keep the chosen name: {help}"
        );
    }

    #[test]
    fn a_misspelled_group_is_an_error_when_it_is_still_a_known_group_elsewhere() {
        // `validators.alowed` enters a known group, so the shape applies.
        let errors = check(&[("validators.alowed", b(true))]).unwrap_err();
        assert_eq!(errors[0].code.id, "settings::unknown-key");
        assert!(errors[0].help.as_deref().unwrap().contains("validators.allowed"));
    }

    #[test]
    fn an_unknown_top_level_key_is_left_alone() {
        // `settings` stays an open namespace outside the groups core owns -
        // core's own test fixtures rely on this.
        check(&[
            ("forbid_indexing", b(true)),
            ("max_depth", SettingsValue::Integer(8)),
            ("label", SettingsValue::Str("core".into())),
            ("some.custom.thing", b(true)),
        ])
        .expect("an open-namespace key must not be rejected");
    }

    #[test]
    fn a_wrongly_typed_value_is_caught_even_though_nothing_uses_the_key() {
        // The hole this closes: `enforcement` only ever looked at keys a
        // declaration actually reached.
        let errors = check(&[(
            "validators.allowed",
            SettingsValue::Identifier("Tru".into()),
        )])
        .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code.id, "settings::malformed-value");
        assert!(errors[0].message.contains("must be a boolean"), "{}", errors[0].message);
        assert!(errors[0].message.contains("got Tru"), "{}", errors[0].message);
    }

    #[test]
    fn a_bad_diagnostic_level_is_caught_at_freeze_time() {
        let errors = check(&[(
            "diagnostics.validation::unknown-type.level",
            SettingsValue::Str("nope".into()),
        )])
        .unwrap_err();
        // The shape owns its code, so this matches what `severity` reports
        // for the same key rather than a generic malformed-value.
        assert_eq!(errors[0].code.id, "settings::invalid-diagnostic-level");
        assert!(errors[0].message.contains("nope"), "{}", errors[0].message);
    }

    #[test]
    fn a_quoted_mode_is_rejected_because_mode_wants_a_bare_keyword() {
        let errors = check(&[("mode", SettingsValue::Str("replace".into()))]).unwrap_err();
        // A reserved word keeps its own code and its own advice, rather
        // than falling into the generic malformed-value bucket.
        assert_eq!(errors[0].code.id, "settings::invalid-mode");
        assert!(
            errors[0].help.as_deref().unwrap().contains("remove it to merge"),
            "{:?}",
            errors[0].help
        );
    }

    #[test]
    fn a_mode_under_an_unverifiable_name_is_still_accepted() {
        // `annotations.framng` is a real name-level position - the
        // registry cannot know whether `framng` names a real annotation,
        // and deliberately does not try. Only the shape around a
        // user-chosen name is checked.
        let replace = SettingsValue::Identifier("replace".into());
        check(&[("annotations.framng.mode", replace)])
            .expect("a name hole is open-ended by design");
    }

    #[test]
    fn a_mode_where_no_dictionary_can_exist_is_an_unknown_key() {
        // `validators.allowed` is itself a complete key, so it is a leaf -
        // nothing nests under it, `mode` included.
        let replace = SettingsValue::Identifier("replace".into());
        let errors = check(&[("validators.allowed.mode", replace)]).unwrap_err();
        assert_eq!(errors[0].code.id, "settings::unknown-key");
    }

    #[test]
    fn every_problem_is_collected_not_just_the_first() {
        let errors = check(&[
            ("annotations.a.alowed", b(true)),
            ("validators.alowed", b(true)),
            ("validators.allowed", SettingsValue::Integer(3)),
        ])
        .unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:#?}");
    }

    #[test]
    fn a_leaf_origin_span_is_reported_when_known() {
        let mut spans = SettingsSpans::new();
        spans.0.insert("validators.alowed".to_string(), (12, 29));
        let errors = check_settings_conformance(
            &dict(&[("validators.alowed", b(true))]),
            &spans,
            "the unnamed settings block",
            None,
        )
        .unwrap_err();
        assert_eq!(errors[0].span, Some((12, 29)));
    }

    #[test]
    fn the_fallback_span_is_used_when_a_leafs_own_origin_is_unknown() {
        // `.idp` settings have no span tracking yet, so the whole
        // assignment's span is the best available pointer.
        let errors = check_settings_conformance(
            &dict(&[("validators.alowed", b(true))]),
            &SettingsSpans::new(),
            "the package's 'settings'",
            Some((3, 7)),
        )
        .unwrap_err();
        assert_eq!(errors[0].span, Some((3, 7)));
    }

    #[test]
    fn the_built_in_default_passes_its_own_pass() {
        check_settings_conformance(
            &crate::settings::defaults::built_in(),
            &SettingsSpans::new(),
            "Comline's built-in default",
            None,
        )
        .expect("core's own default policy must satisfy core's own registry");
    }
}
