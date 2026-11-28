// The square on honest steps (core#268, step 1.4): what the engine's
// migration of a store the core of N built must equal, and what each
// carrier of an unknown does.

use comline_core::package::build::cas::migrate::{
    Check, Consent, Container, Engine, Format, MigrateOptions,
};
use comline_core::package::build::cas::storage::to_cbor;
use proptest::prelude::*;

use super::formats::*;
use super::histories::{corpus, history, project};
use super::square::*;
use super::steps::*;
use super::strategies;

/// Every pair of formats a store can go between: each step alone, and the
/// two composed.
pub const PAIRS: [(u32, u32); 3] = [(T0, T1), (T1, T2), (T0, T2)];

fn options() -> MigrateOptions {
    MigrateOptions::default()
}

#[test]
fn what_the_old_core_built_and_the_engine_migrated_is_what_the_new_core_builds_modulo_unknown() {
    for history in corpus() {
        for (from, to) in PAIRS {
            if let Err(failure) = disk_square(&history, from, to, Defect::None, Defect::None) {
                panic!("{}, {from} -> {to}: {failure:?}", history.name);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn the_square_commutes_for_every_source(source in strategies::source()) {
        for (from, to) in PAIRS {
            let result = pure_square(&source, from, to, Defect::None, Defect::None);
            prop_assert!(result.is_ok(), "{from} -> {to}: {result:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn the_square_commutes_for_every_history(history in strategies::history()) {
        for (from, to) in PAIRS {
            let result = disk_square(&history, from, to, Defect::None, Defect::None);
            prop_assert!(result.is_ok(), "{from} -> {to}: {result:?}");
        }
    }
}

// ---------- the lossy step: an unknown with no room in the new type ----------

#[test]
fn a_lossy_step_keeps_what_the_old_format_recorded_and_says_what_it_did_not() {
    let history = history("grows");
    let dir = project(&history, T0);
    Engine::new(honest(T1))
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    let old = Container::open(&dir.path().join(".comline.format-80.bak")).unwrap();
    let new = Container::open(&dir.path().join(".comline")).unwrap();

    let mut entries = 0;
    for (before, after) in old.commits.iter().zip(&new.commits) {
        let was = Format0.decode(&old.store, &before.commit.tree).unwrap();
        let now = Format1.decode(&new.store, &after.commit.tree).unwrap();
        let floats: Vec<usize> = was
            .scalars
            .iter()
            .enumerate()
            .filter(|(_, scalar)| scalar.kind == Kind0::Float)
            .map(|(index, _)| index)
            .collect();

        // One entry per float, at its width, and none for anything else.
        let paths: Vec<&str> = now.unknown.iter().map(|r| r.path.as_str()).collect();
        let expected: Vec<String> = floats
            .iter()
            .map(|index| format!("/scalars/{index}/kind/bits"))
            .collect();
        assert_eq!(paths, expected, "version {}", before.commit.version);

        for (record, index) in now.unknown.iter().zip(&floats) {
            assert_eq!(record.step, "80-81");
            assert!(record.reason.contains("no width"), "{}", record.reason);
            // What format 80 recorded, kept as the old bytes.
            assert_eq!(
                record.original,
                Some(to_cbor(&was.scalars[*index].kind).unwrap())
            );
            // And a placeholder in the field, which the entry marks as one.
            assert_eq!(
                now.scalars[*index].kind.bits,
                Some(WIDTH_PLACEHOLDER),
                "the placeholder"
            );
        }
        for (index, scalar) in now.scalars.iter().enumerate() {
            if !floats.contains(&index) {
                assert_eq!(scalar.kind.bits, None, "what has no width has none");
            }
        }
        entries += now.unknown.len();
    }
    assert!(entries > 0, "the history has floats: not vacuous");

    // The status lists them, and a store with none lists none.
    let status = Engine::new(honest(T1)).status(dir.path()).unwrap();
    let listed: usize = status
        .versions
        .unwrap()
        .iter()
        .map(|v| v.unknowns.unwrap())
        .sum();
    assert_eq!(listed, entries);
}

#[test]
fn without_its_entry_a_placeholder_would_read_as_a_float_of_64_bits() {
    // A float that T0 recorded and a float of 64 bits that T1 recorded have
    // the same scalar. Only the entry says that the first one's width is
    // not known: forget is what makes the square tell them apart, and the
    // square compared against a fresh build that was not forgotten fails.
    let source = source_with(SourceKind::Float64);
    let migrated = migrate_pure(&fresh(T0, &source), T1, Defect::None, Defect::None).unwrap();
    let built = fresh(T1, &source);
    let (AnyPayload::T1(m1), AnyPayload::T1(b1)) = (&migrated, &built) else {
        panic!()
    };
    assert_eq!(m1.scalars, b1.scalars, "the same scalar, bytes and all");
    assert_eq!(m1.unknown.len(), 1, "and one entry");
    assert!(b1.unknown.is_empty());

    assert!(matches!(
        compare(&migrated.value(), &built.value()),
        Err(Failure::Needless { .. })
    ));
    pure_square(&source, T0, T1, Defect::None, Defect::None).unwrap();
}

fn source_with(kind: SourceKind) -> Source {
    super::histories::source(
        "One",
        "ana",
        vec![super::histories::scalar("width", 8, kind, &["t"])],
    )
}

// ---------- a typed unknown: a field every reader has to handle ----------

#[test]
fn a_typed_unknown_is_a_field_the_reader_has_to_match_and_the_plan_lists_it() {
    let history = history("grows");
    let dir = project(&history, T1);
    let engine = Engine::new(honest(T2));

    let Check::WouldMigrate(plan) = engine.check(dir.path(), &options()) else {
        panic!("a store in format 81 has a step to 82")
    };
    assert_eq!(plan.steps, [(T1, T2)]);
    assert_eq!(plan.unknowns.len(), plan.versions.len(), "one per version");
    for line in &plan.unknowns {
        assert_eq!(line.record.path, "/owner");
        assert_eq!(line.record.step, "81-82");
        assert!(line.record.reason.contains("no owner"));
        assert_eq!(line.record.original, None, "N recorded nothing there");
    }

    let mut engine = engine;
    engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    let new = Container::open(&dir.path().join(".comline")).unwrap();
    for node in &new.commits {
        let payload = Format2.decode(&new.store, &node.commit.tree).unwrap();
        // The type forces the match: there is no `Deref`, no default.
        match &payload.owner {
            Known::Unknown { step, .. } => assert_eq!(step, "81-82"),
            Known::Known(owner) => panic!("an owner was invented: {owner}"),
        }
        assert!(
            payload.unknown.is_empty(),
            "a typed unknown is not an entry of the `unknown` blob"
        );
        assert!(
            matches!(payload.total_size, Known::Known(_)),
            "what 81 could compute is known"
        );
        assert_eq!(Format2.unknowns(&payload).len(), 1);
    }
}

#[test]
fn a_chain_lists_every_step_s_unknowns_and_a_fresh_build_has_none() {
    let history = history("grows");
    let dir = project(&history, T0);
    let engine = Engine::new(honest(T2));
    let Check::WouldMigrate(plan) = engine.check(dir.path(), &options()) else {
        panic!()
    };
    assert_eq!(plan.steps, [(T0, T1), (T1, T2)]);
    let owners = plan
        .unknowns
        .iter()
        .filter(|line| line.record.path == "/owner")
        .count();
    let widths = plan
        .unknowns
        .iter()
        .filter(|line| line.record.path.ends_with("/kind/bits"))
        .count();
    assert_eq!(owners, plan.versions.len());
    assert!(widths > 0);
    assert_eq!(owners + widths, plan.unknowns.len(), "and nothing else");

    // What the core of the newest format builds has nothing unknown.
    for scalar_source in corpus().iter().flat_map(|h| h.events.iter()) {
        if let super::histories::Event::Build(source) = scalar_source {
            let AnyPayload::T2(built) = fresh(T2, source) else {
                panic!()
            };
            assert!(Format2.unknowns(&built).is_empty());
        }
    }
}
