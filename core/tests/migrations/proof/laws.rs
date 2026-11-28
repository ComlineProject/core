// The laws (core#268, step 1.4; proof.md, "Laws"), on the test-only formats.
// For each law, what a step that breaks it looks like is run too, so that a
// law is shown to be a check that can fail.
//
//   Totality              `up` returns a value for every value the format
//                         can express, not only what a build writes
//   Determinism           the same bytes from the same input, on any thread
//                         count, in any order, on any platform (committed
//                         digests, compared on every CI platform)
//   Information preserved `down(up(v)) == v`, over fixtures and random values
//   Idempotence           a migrated store migrates to itself (and an
//                         interrupted one, resumed: `crashes.rs`)
//   Composition           N -> K is N -> M then M -> K, and both are the
//                         square
//   Hashes carried        what the user owns, and a payload with the same
//                         bytes, are the same after
//   Convergence           the migration a read command does in memory is the
//                         one a migration writes
//   Pins                  not here: they need a dependency's pin, and the
//                         first real client (format 4)

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use comline_core::package::build::cas::migrate::{
    Consent, Container, Engine, MemObjects, MigrateError, MigrateOptions, Migrated,
};
use comline_core::package::build::cas::objects::Tree;
use comline_core::package::build::cas::storage::Hash;
use comline_core::package::build::cas::ObjectStore;
use proptest::prelude::*;
use tempfile::TempDir;

use super::super::support::snapshot;
use super::formats::*;
use super::histories::{corpus, history, project, Event, History};
use super::square::*;
use super::squares::PAIRS;
use super::steps::*;
use super::strategies;

fn migrate(dir: &Path, to: u32, first: Defect, second: Defect) -> eyre::Result<Migrated> {
    Engine::new(registry(to, first, second)).migrate(
        dir,
        &MigrateOptions::default(),
        Consent::Command,
    )
}

/// `history` as the core of `from` builds it, migrated by honest steps.
fn migrated(history: &History, from: u32, to: u32) -> TempDir {
    let dir = project(history, from);
    migrate(dir.path(), to, Defect::None, Defect::None).unwrap();
    dir
}

fn all_sources() -> Vec<Source> {
    corpus()
        .into_iter()
        .flat_map(|history| {
            let mut sources: Vec<Source> = history
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Build(source) => Some(source.clone()),
                    _ => None,
                })
                .collect();
            sources.extend(history.fork.clone());
            sources
        })
        .collect()
}

// ---------- totality ----------

/// The format's codec holds the value: it encodes, and decodes to itself.
fn round_trips(payload: &AnyPayload) {
    let mut objects = MemObjects::default();
    let root = encode_any(payload, &mut objects).unwrap();
    assert_eq!(
        &decode_any(payload.number(), &objects, &root).unwrap(),
        payload
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Not only reachable values: any `P0` and any `P1` (a width on an
    /// integer, entries at any path, sizes no build writes).
    #[test]
    fn up_returns_a_value_that_encodes_for_every_value_of_the_format(
        p0 in strategies::p0(),
        p1 in strategies::p1(),
    ) {
        for start in [AnyPayload::T0(p0), AnyPayload::T1(p1)] {
            round_trips(&start);
            let next = up(&start, Defect::None, Defect::None);
            prop_assert!(next.is_ok(), "{:?}", next);
            round_trips(&next.unwrap());
        }
    }

    /// `down(up(v)) == v` over values no build writes.
    #[test]
    fn nothing_is_lost_for_any_value_of_the_format(
        p0 in strategies::p0(),
        p1 in strategies::p1(),
    ) {
        for start in [AnyPayload::T0(p0), AnyPayload::T1(p1)] {
            prop_assert!(witness(&start, Defect::None, Defect::None).unwrap());
        }
    }
}

#[test]
fn nothing_is_lost_for_any_source_of_the_corpus() {
    for source in all_sources() {
        for number in [T0, T1] {
            assert!(witness(&fresh(number, &source), Defect::None, Defect::None).unwrap());
        }
    }
}

#[test]
fn a_step_that_is_not_total_passes_every_fixture_and_fails_a_value_no_build_writes() {
    // On every history of the corpus, whose sizes are small, it is
    // indistinguishable from the honest step.
    for history in corpus() {
        for (from, to) in PAIRS {
            if let Err(failure) = disk_square(&history, from, to, Defect::Partial, Defect::None) {
                panic!("{}, {from} -> {to}: {failure:?}", history.name);
            }
        }
    }
    // On a value the format can express, it fails, and the honest step does
    // not.
    let big = AnyPayload::T0(P0 {
        title: "t".into(),
        scalars: vec![S0 {
            name: "n".into(),
            size: PARTIAL_LIMIT + 1,
            kind: Kind0::Int,
            tags: vec![],
        }],
    });
    assert!(up(&big, Defect::None, Defect::None).is_ok());
    assert!(up(&big, Defect::Partial, Defect::None).is_err());

    // And a store that holds one is refused by the engine, naming the step
    // and the version, with nothing written.
    let mut history = history("empty");
    history.events = vec![Event::Build(super::histories::source(
        "Big",
        "ana",
        vec![super::histories::scalar(
            "huge",
            PARTIAL_LIMIT + 1,
            SourceKind::Int,
            &[],
        )],
    ))];
    let dir = project(&history, T0);
    let before = snapshot(dir.path());
    let error = migrate(dir.path(), T1, Defect::Partial, Defect::None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<MigrateError>(),
            Some(MigrateError::StepFailed { step, .. }) if step == "80-81"
        ),
        "{error:#}"
    );
    assert_eq!(snapshot(dir.path()), before);
    migrate(dir.path(), T1, Defect::None, Defect::None).unwrap();
}

// ---------- determinism ----------

/// Two independent migrations of one store give the same files.
fn deterministic(history: &History, from: u32, to: u32, first: Defect, second: Defect) -> bool {
    let (a, b) = (project(history, from), project(history, from));
    migrate(a.path(), to, first, second).unwrap();
    migrate(b.path(), to, first, second).unwrap();
    snapshot(a.path()) == snapshot(b.path())
}

#[test]
fn the_same_store_migrated_twice_is_the_same_files() {
    for history in corpus() {
        for (from, to) in PAIRS {
            assert!(
                deterministic(&history, from, to, Defect::None, Defect::None),
                "{}, {from} -> {to}",
                history.name
            );
        }
    }
}

#[test]
fn a_step_that_reads_a_clock_or_orders_by_a_hash_map_is_not_deterministic() {
    assert!(!deterministic(
        &history("grows"),
        T1,
        T2,
        Defect::None,
        Defect::Clock
    ));
    // Each hash map has its own random state: twenty tags in two orders.
    assert!(!deterministic(
        &history("tagged"),
        T1,
        T2,
        Defect::None,
        Defect::HashOrder
    ));
}

/// Every input through the chain, the inputs taken in `order` by `threads`
/// threads.
fn chain_all(
    inputs: &[AnyPayload],
    order: &[usize],
    threads: usize,
    first: Defect,
    second: Defect,
) -> Vec<AnyPayload> {
    let results: Mutex<Vec<Option<AnyPayload>>> = Mutex::new(vec![None; inputs.len()]);
    let chunk = order.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for part in order.chunks(chunk) {
            let results = &results;
            scope.spawn(move || {
                for &index in part {
                    let migrated = migrate_pure(&inputs[index], T2, first, second).unwrap();
                    results.lock().unwrap()[index] = Some(migrated);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|payload| payload.expect("every input was migrated"))
        .collect()
}

#[test]
fn the_steps_give_the_same_values_on_any_thread_count_and_in_any_order() {
    let inputs: Vec<AnyPayload> = all_sources().iter().map(|s| fresh(T0, s)).collect();
    let in_order: Vec<usize> = (0..inputs.len()).collect();
    let reversed: Vec<usize> = in_order.iter().rev().copied().collect();
    let reference = chain_all(&inputs, &in_order, 1, Defect::None, Defect::None);
    for threads in [2, 3, 8] {
        assert_eq!(
            chain_all(&inputs, &reversed, threads, Defect::None, Defect::None),
            reference,
            "{threads} thread(s), versions in reverse"
        );
    }
    // The check can fail: with a step that reads a clock it does.
    assert_ne!(
        chain_all(&inputs, &reversed, 4, Defect::None, Defect::Clock),
        chain_all(&inputs, &in_order, 1, Defect::None, Defect::Clock)
    );
}

const HEADER: &str = "\
# The migrated test-only stores, as digests (core#268, step 1.4): for each
# script of the corpus and each pair of formats, every commit's hash before
# and after the migration, oldest first. The same on every platform: the
# format gate's CI runs this on Linux, macOS and Windows against this file.
# These move only with the corpus, the codecs or the steps of the test-only
# formats. Regenerate:
#   COMLINE_UPDATE_GOLDEN=1 cargo test -p comline-core --features deps --test mod migrations::proof::laws
";

fn digests() -> String {
    let mut lines = vec![];
    for history in corpus() {
        for (from, to) in PAIRS {
            let dir = project(&history, from);
            let Migrated::Done(report) =
                migrate(dir.path(), to, Defect::None, Defect::None).unwrap()
            else {
                panic!("a store in format {from} has a step")
            };
            let new = Container::open(&dir.path().join(".comline")).unwrap();
            for ((old_hash, new_hash), node) in report.map.iter().zip(&new.commits) {
                lines.push(format!(
                    "{}  {from}->{to}  {}  {old_hash}  {new_hash}",
                    history.name, node.commit.version
                ));
            }
        }
    }
    lines.join("\n")
}

#[test]
fn the_migrated_stores_hash_to_the_committed_digests_on_every_platform() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/migrations/proof/golden.txt");
    let fresh = format!("{HEADER}{}\n", digests());
    if std::env::var_os("COMLINE_UPDATE_GOLDEN").is_some() {
        fs::write(&path, &fresh).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no digests ({e}); COMLINE_UPDATE_GOLDEN=1 writes them"))
        .replace("\r\n", "\n");
    assert_eq!(
        fresh,
        expected,
        "the migration of a test-only store is not what {} records",
        path.display()
    );
}

// ---------- idempotence ----------

#[test]
fn migrating_a_migrated_store_changes_nothing() {
    for history in corpus() {
        let dir = migrated(&history, T0, T2);
        let before = snapshot(dir.path());
        assert_eq!(
            migrate(dir.path(), T2, Defect::None, Defect::None).unwrap(),
            Migrated::NothingToDo,
            "{}",
            history.name
        );
        assert_eq!(snapshot(dir.path()), before, "{}", history.name);
    }
}

// ---------- composition ----------

#[test]
fn migrating_n_to_k_is_migrating_n_to_m_then_m_to_k_and_both_are_the_square() {
    for history in corpus() {
        let straight = migrated(&history, T0, T2);
        let staged = project(&history, T0);
        migrate(staged.path(), T1, Defect::None, Defect::None).unwrap();
        migrate(staged.path(), T2, Defect::None, Defect::None).unwrap();

        let mut a = snapshot(&straight.path().join(".comline"));
        let mut b = snapshot(&staged.path().join(".comline"));
        // The receipt is the one thing that says how it got there.
        let receipt = PathBuf::from("MIGRATIONS");
        assert_eq!(
            a.remove(&receipt),
            Some(Some(b"80 -> 82\n".to_vec())),
            "{}",
            history.name
        );
        assert_eq!(
            b.remove(&receipt),
            Some(Some(b"80 -> 81\n81 -> 82\n".to_vec())),
            "{}",
            history.name
        );
        assert!(a == b, "{}: two steps are not one chain", history.name);

        let built = project(&history, T2);
        compare_stores(
            &staged.path().join(".comline"),
            &built.path().join(".comline"),
            T2,
            &declared(T0, T2, Defect::None, Defect::None),
        )
        .unwrap_or_else(|failure| panic!("{}: {failure:?}", history.name));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// The same, one version at a time: writing the middle format and
    /// reading it back changes nothing.
    #[test]
    fn composing_through_the_written_middle_is_the_same(source in strategies::source()) {
        let start = fresh(T0, &source);
        let straight = migrate_pure(&start, T2, Defect::None, Defect::None).unwrap();
        let middle = up(&start, Defect::None, Defect::None).unwrap();
        let mut objects = MemObjects::default();
        let root = encode_any(&middle, &mut objects).unwrap();
        let written = decode_any(T1, &objects, &root).unwrap();
        let staged = up(&written, Defect::None, Defect::None).unwrap();
        prop_assert_eq!(straight, staged);
    }
}

// ---------- hashes carried ----------

fn meta_hash(container: &Container, index: usize) -> Hash {
    let tree = container
        .store
        .read(&container.commits[index].commit.tree)
        .unwrap();
    Tree::from_bytes(&tree)
        .unwrap()
        .find_entry("meta")
        .expect("every version has a `meta` entry")
        .hash
}

#[test]
fn a_payload_with_the_same_bytes_keeps_its_hash_and_what_the_user_owns_is_the_same() {
    for history in corpus() {
        let dir = project(&history, T0);
        let Migrated::Done(report) = migrate(dir.path(), T1, Defect::None, Defect::None).unwrap()
        else {
            panic!()
        };
        let old = Container::open(&dir.path().join(".comline.format-80.bak")).unwrap();
        let new = Container::open(&dir.path().join(".comline")).unwrap();

        // Versions, release records, parents and heads: the same history.
        let pairs = same_history(&old, &new).unwrap_or_else(|f| panic!("{}: {f:?}", history.name));
        assert_eq!(pairs.len(), old.commits.len());
        for (i, j) in pairs {
            // The `meta` blob has the same bytes in 80 and 81 (the same
            // type): the same object.
            assert_eq!(meta_hash(&old, i), meta_hash(&new, j), "{}", history.name);
            // The commit itself moved, because its tree did: the map says so.
            assert!(report
                .map
                .contains(&(old.commits[i].hash, new.commits[j].hash)));
            assert_ne!(old.commits[i].hash, new.commits[j].hash);
        }
    }
}

// ---------- convergence ----------

#[test]
fn the_migration_a_read_command_does_in_memory_is_the_one_a_migration_writes() {
    for history in corpus() {
        let dir = project(&history, T0);
        let engine = Engine::new(honest(T2));
        let before = snapshot(dir.path());

        let reading = engine.open_for_reading(dir.path()).unwrap();
        assert_eq!(reading.migrated_from, Some(T0));
        assert_eq!(snapshot(dir.path()), before, "reading writes nothing");

        migrate(dir.path(), T2, Defect::None, Defect::None).unwrap();
        let new = Container::open(&dir.path().join(".comline")).unwrap();
        let written = ObjectStore::new(dir.path());
        assert_eq!(reading.refs, new.refs, "{}", history.name);
        for node in &new.commits {
            for hash in [&node.hash, &node.commit.tree] {
                assert_eq!(
                    reading.store.read(hash).unwrap(),
                    written.read(hash).unwrap(),
                    "{}: object {hash}",
                    history.name
                );
            }
        }
    }
}
