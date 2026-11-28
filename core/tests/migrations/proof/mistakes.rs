// Steps that are wrong on purpose (core#268, step 1.4; proof.md, "Test-only
// formats" and "The gate"): the gate is tested by seeing it reject a bad
// step. One test for each kind of mistake proof.md names, and each says
// what rejects it: the square, which sees a wrong, an invented or a
// needless value, or the engine, which holds a step to its witness and to
// the unknowns it declares. Each honest step passes the same checks
// (`squares.rs`), so what fails here fails because of the one mistake.
//
//   a wrong value for a field N could know      the square: Wrong
//   a value for a field N could not know        the square: Invented
//   an unknown for a field N could know         the square: Needless; and the
//                                               engine too, when the path is
//                                               not even on the step's list
//   a field dropped                             the engine's witness
//   a clock, a hash map's order                 determinism (`laws.rs`)
//   a step that is not total                    totality (`laws.rs`)
//
// The witness cannot see the first three, by design: a step whose `down`
// agrees with its `up` passes it whatever `up` computed. The tests say so.

use comline_core::package::build::cas::migrate::{path_matches, MigrateError};
use proptest::prelude::*;

use super::formats::*;
use super::histories::history;
use super::square::*;
use super::steps::*;
use super::strategies;

/// The failure of the square on the history `grows` (every kind, a float of
/// each width, edits of every field).
fn rejected(from: u32, to: u32, first: Defect, second: Defect) -> Failure {
    match disk_square(&history("grows"), from, to, first, second) {
        Ok(()) => {
            panic!("the gate accepted a step that is wrong on purpose: {first:?}, {second:?}")
        }
        Err(failure) => failure,
    }
}

fn the_witness_holds(first: Defect, second: Defect) {
    for source in sources() {
        for number in [T0, T1] {
            assert!(
                witness(&fresh(number, &source), first, second).unwrap(),
                "{first:?}, {second:?}: the witness is what this mistake gets past"
            );
        }
    }
}

/// The sources of the history `grows`.
fn sources() -> Vec<Source> {
    history("grows")
        .events
        .into_iter()
        .filter_map(|event| match event {
            super::histories::Event::Build(source) => Some(source),
            _ => None,
        })
        .collect()
}

// ---------- the first step ----------

#[test]
fn a_wrong_value_for_a_field_the_old_format_knew_is_a_failure_of_the_square() {
    let failure = rejected(T0, T1, Defect::WrongValue, Defect::None);
    let Failure::Wrong { path, .. } = &failure else {
        panic!("{failure:?}")
    };
    assert!(
        path.starts_with("/scalars/") && path.ends_with("/size"),
        "{path}"
    );
    the_witness_holds(Defect::WrongValue, Defect::None);
}

#[test]
fn a_value_invented_for_a_field_the_old_format_never_recorded_is_a_failure_of_the_square() {
    // The step declares the width unknown and writes one. The engine holds a
    // step to the unknowns it writes being declared, not to the declared
    // ones being written, so only the square sees it.
    let failure = rejected(T0, T1, Defect::Invents, Defect::None);
    let Failure::Invented { path } = &failure else {
        panic!("{failure:?}")
    };
    assert!(path.ends_with("/kind/bits"), "{path}");
    the_witness_holds(Defect::Invents, Defect::None);
}

#[test]
fn an_unknown_for_a_field_the_old_format_knew_is_a_failure_of_the_square() {
    // What has no width has nothing unknown about it. The path is covered by
    // the step's own pattern, so the engine lets it through; `forget` leaves
    // the field alone, because the fresh value has no width there.
    let failure = rejected(T0, T1, Defect::Needless, Defect::None);
    let Failure::Needless { path } = &failure else {
        panic!("{failure:?}")
    };
    assert!(path_matches("/scalars/*/kind/bits", path), "{path}");
    the_witness_holds(Defect::Needless, Defect::None);
}

#[test]
fn a_field_dropped_is_a_failure_of_the_engines_witness() {
    let failure = rejected(T0, T1, Defect::Drops, Defect::None);
    match failure.engine_error() {
        Some(MigrateError::Witness { step, .. }) => assert_eq!(step, "80-81"),
        other => panic!("{other:?} in {failure:?}"),
    }
}

// ---------- the second step ----------

#[test]
fn a_wrong_value_in_the_second_step_is_a_failure_of_the_square() {
    let failure = rejected(T1, T2, Defect::None, Defect::WrongValue);
    let Failure::Wrong { path, .. } = &failure else {
        panic!("{failure:?}")
    };
    assert_eq!(path, "/total_size");
}

#[test]
fn an_owner_invented_is_a_failure_of_the_square() {
    let failure = rejected(T1, T2, Defect::None, Defect::Invents);
    let Failure::Invented { path } = &failure else {
        panic!("{failure:?}")
    };
    assert_eq!(path, "/owner");
}

#[test]
fn a_typed_unknown_for_what_the_old_format_could_compute_is_refused_by_the_engine_and_the_square() {
    // It is not on the step's list, so the engine refuses it...
    let failure = rejected(T1, T2, Defect::None, Defect::Needless);
    match failure.engine_error() {
        Some(MigrateError::UndeclaredUnknown { step, path, .. }) => {
            assert_eq!((step.as_str(), path.as_str()), ("81-82", "/total_size"));
        }
        other => panic!("{other:?} in {failure:?}"),
    }
    // ...and the square would have, had the list covered it.
    let source = sources().remove(0);
    let seen = pure_square(&source, T1, T2, Defect::None, Defect::Needless);
    assert!(
        matches!(&seen, Err(Failure::Needless { path }) if path == "/total_size"),
        "{seen:?}"
    );
}

#[test]
fn a_field_dropped_in_the_second_step_is_a_failure_of_the_engines_witness() {
    let failure = rejected(T1, T2, Defect::None, Defect::Drops);
    match failure.engine_error() {
        Some(MigrateError::Witness { step, .. }) => assert_eq!(step, "81-82"),
        other => panic!("{other:?} in {failure:?}"),
    }
}

// ---------- in a chain ----------

#[test]
fn a_mistake_in_either_step_of_a_chain_is_seen_through_both() {
    let cases: [(Defect, Defect, fn(&Failure) -> bool); 4] = [
        (
            Defect::WrongValue,
            Defect::None,
            |f| matches!(f, Failure::Wrong { path, .. } if path.ends_with("/size") || path.ends_with("/bytes")),
        ),
        (
            Defect::Invents,
            Defect::None,
            |f| matches!(f, Failure::Invented { path } if path.ends_with("/kind/bits")),
        ),
        (
            Defect::None,
            Defect::WrongValue,
            |f| matches!(f, Failure::Wrong { path, .. } if path == "/total_size"),
        ),
        (
            Defect::None,
            Defect::Invents,
            |f| matches!(f, Failure::Invented { path } if path == "/owner"),
        ),
    ];
    for (first, second, expected) in cases {
        let failure = rejected(T0, T2, first, second);
        assert!(expected(&failure), "{first:?}, {second:?}: {failure:?}");
    }
    // A mistake the witness sees is seen by the engine in the step it is in.
    for (first, second, step) in [
        (Defect::Drops, Defect::None, "80-81"),
        (Defect::None, Defect::Drops, "81-82"),
    ] {
        let failure = rejected(T0, T2, first, second);
        assert!(
            matches!(failure.engine_error(), Some(MigrateError::Witness { step: s, .. }) if s == step),
            "{failure:?}"
        );
    }
}

// ---------- the square is exact: it sees a mistake wherever it can show ----------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Each mistake is a failure on every source it can show itself on,
    /// and the square is silent on the others: it rejects what is wrong and
    /// nothing else.
    #[test]
    fn each_mistake_fails_on_exactly_the_sources_it_shows_on(source in strategies::source()) {
        let floats = source.scalars.iter().any(|s| matches!(s.kind, SourceKind::Float32 | SourceKind::Float64));
        let others = source.scalars.iter().any(|s| matches!(s.kind, SourceKind::Int | SourceKind::Text));
        let sized = !source.scalars.is_empty();

        let first = |defect| pure_square(&source, T0, T1, defect, Defect::None);
        prop_assert_eq!(matches!(first(Defect::WrongValue), Err(Failure::Wrong { .. })), sized);
        prop_assert_eq!(matches!(first(Defect::Invents), Err(Failure::Invented { .. })), floats);
        prop_assert_eq!(matches!(first(Defect::Needless), Err(Failure::Needless { .. })), others);
        prop_assert!(first(Defect::None).is_ok());

        let second = |defect| pure_square(&source, T1, T2, Defect::None, defect);
        prop_assert!(matches!(second(Defect::WrongValue), Err(Failure::Wrong { .. })), "second step: wrong");
        prop_assert!(matches!(second(Defect::Invents), Err(Failure::Invented { .. })), "second step: invented");
        prop_assert!(matches!(second(Defect::Needless), Err(Failure::Needless { .. })), "second step: needless");
        prop_assert!(second(Defect::None).is_ok());
    }
}
