// The engine on test-only formats (core#268, step 1.3): what a real step
// would drive, with the smallest step that does. `T0` (format 90) holds one
// number per version; `T1` (format 91) widens it and adds a label that format
// 90 never recorded, an unknown the step declares. Nothing here is a format a
// release writes. Step 1.4's proof harness builds on the same hooks with more
// formats and more steps; what is covered here is the engine's mechanics:
// plan, journal, verification, swap, backup, resume after a crash at every
// point, reset and prune.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use comline_core::package::build::cas::migrate::{
    backups, Check, Consent, Container, Engine, Format, Leftover, MigrateError, MigrateOptions,
    Migrated, Objects, Point, Registry, Sink, Step, UnknownDecl, Version,
};
use comline_core::package::build::cas::object_store::{
    ensure_compatible_store_with, Interrupted, StoreError, StoreState,
};
use comline_core::package::build::cas::objects::{Commit, EntryMode, Release, Tree};
use comline_core::package::build::cas::reconcile::{parse_heads, Heads};
use comline_core::package::build::cas::storage::{from_cbor, to_cbor, Hash};
use comline_core::package::build::cas::{
    update_ref, ObjectStore, VersionBump, REF_HEAD_MAIN_FILENAME,
};
use comline_ir_formats::v3::types::UnknownRecord;
use eyre::eyre;
use tempfile::TempDir;

use super::support::{files_under, snapshot};

const T0: u32 = 90;
const T1: u32 = 91;

// ---------- the test-only formats ----------

#[derive(Clone, Debug, PartialEq)]
struct P0 {
    value: u32,
}

struct Format0;

impl Format for Format0 {
    type Payload = P0;

    fn number(&self) -> u32 {
        T0
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> eyre::Result<P0> {
        let tree = objects.tree(root)?;
        let entry = tree
            .find_entry("value")
            .ok_or_else(|| eyre!("no `value` entry"))?;
        Ok(P0 {
            value: from_cbor(&objects.blob(&entry.hash)?)?,
        })
    }

    fn encode(&self, payload: &P0, sink: &mut dyn Sink) -> eyre::Result<Hash> {
        let mut tree = Tree::new();
        tree.add_entry(
            EntryMode::Blob,
            "value".into(),
            sink.blob(to_cbor(&payload.value)?)?,
        );
        sink.tree(&tree)
    }

    fn unknowns(&self, _: &P0) -> Vec<UnknownRecord> {
        vec![]
    }
}

#[derive(Clone, Debug, PartialEq)]
struct P1 {
    value: u64,
    /// `None` is unknown: format 90 recorded no labels.
    label: Option<String>,
}

struct Format1;

impl Format for Format1 {
    type Payload = P1;

    fn number(&self) -> u32 {
        T1
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> eyre::Result<P1> {
        let tree = objects.tree(root)?;
        let read = |name: &str| -> eyre::Result<Vec<u8>> {
            let entry = tree
                .find_entry(name)
                .ok_or_else(|| eyre!("no `{name}` entry"))?;
            objects.blob(&entry.hash)
        };
        Ok(P1 {
            value: from_cbor(&read("value")?)?,
            label: from_cbor(&read("label")?)?,
        })
    }

    fn encode(&self, payload: &P1, sink: &mut dyn Sink) -> eyre::Result<Hash> {
        let mut tree = Tree::new();
        tree.add_entry(
            EntryMode::Blob,
            "value".into(),
            sink.blob(to_cbor(&payload.value)?)?,
        );
        tree.add_entry(
            EntryMode::Blob,
            "label".into(),
            sink.blob(to_cbor(&payload.label)?)?,
        );
        sink.tree(&tree)
    }

    fn unknowns(&self, payload: &P1) -> Vec<UnknownRecord> {
        match payload.label {
            Some(_) => vec![],
            None => vec![UnknownRecord {
                path: "/label".into(),
                step: format!("{T0}-{T1}"),
                reason: "format 90 recorded no labels".into(),
                original: None,
            }],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    /// `up` loses the value: the witness must catch it.
    Forgetful,
    /// Writes an unknown it doesn't declare.
    Undeclared,
}

struct Widen(Mode);

impl Step for Widen {
    type From = P0;
    type To = P1;

    fn from_format(&self) -> u32 {
        T0
    }

    fn unknown(&self) -> Vec<UnknownDecl> {
        match self.0 {
            Mode::Undeclared => vec![UnknownDecl::new("/elsewhere", "declared, not written")],
            _ => vec![UnknownDecl::new("/label", "format 90 recorded no labels")],
        }
    }

    fn up(&self, version: Version<P0>) -> eyre::Result<Version<P1>> {
        let value = if self.0 == Mode::Forgetful {
            0
        } else {
            u64::from(version.payload.value)
        };
        Ok(version.with_payload(P1 { value, label: None }))
    }

    fn down(&self, version: Version<P1>) -> eyre::Result<Version<P0>> {
        let value = u32::try_from(version.payload.value)?;
        Ok(version.with_payload(P0 { value }))
    }
}

fn registry(mode: Mode) -> Registry {
    Registry::new(T1)
        .with_format(Format0)
        .with_format(Format1)
        .with_step(Widen(mode))
}

fn engine() -> Engine {
    Engine::new(registry(Mode::Honest))
}

fn options() -> MigrateOptions {
    MigrateOptions::default()
}

// ---------- a store in format 90 ----------

/// A project with a format-90 store of three versions (the last a release that
/// counts from the first) and a fetched dependency in `deps-cache/`. Nothing
/// in it depends on a clock or a user, so every run writes the same bytes.
fn t0_project() -> (TempDir, Vec<Hash>) {
    let dir = TempDir::new().unwrap();
    let commits = t0_store(dir.path(), &[5, 6, 7]);
    let cached = dir.path().join(".comline/deps-cache/dep-1234");
    fs::create_dir_all(&cached).unwrap();
    fs::write(cached.join("config.idp"), "a fetched dependency").unwrap();
    (dir, commits)
}

fn t0_commit(store: &ObjectStore, value: u32, index: usize, parents: Vec<Hash>) -> Commit {
    let blob = comline_core::package::build::cas::objects::Blob::new(to_cbor(&value).unwrap());
    let blob = store.write(&blob.to_bytes().unwrap()).unwrap();
    let mut tree = Tree::new();
    tree.add_entry(EntryMode::Blob, "value".into(), blob);
    let tree = store.write(&tree.to_bytes().unwrap()).unwrap();
    Commit::new(
        tree,
        parents,
        "tester".into(),
        1_700_000_000 + index as i64,
        format!("build {index}"),
        format!("0.1.0-dev.{}", index + 1),
    )
}

fn t0_store(root: &Path, values: &[u32]) -> Vec<Hash> {
    let store = ObjectStore::new(root);
    store.init().unwrap();
    fs::write(root.join(".comline/FORMAT_VERSION"), T0.to_string()).unwrap();
    let mut commits: Vec<Hash> = vec![];
    for (index, value) in values.iter().enumerate() {
        let mut commit = t0_commit(
            &store,
            *value,
            index,
            commits.last().copied().into_iter().collect(),
        );
        if index == 2 {
            commit.version = "0.1.0".into();
            commit.release = Some(Release {
                computed: VersionBump::Patch,
                applied: VersionBump::Minor,
                reason: Some("raised".into()),
                covered: false,
                stable: false,
                since: Some(commits[0]),
            });
        }
        commits.push(store.write(&commit.to_bytes().unwrap()).unwrap());
    }
    update_ref(root, REF_HEAD_MAIN_FILENAME, commits.last().unwrap()).unwrap();
    commits
}

fn stamp(root: &Path) -> String {
    fs::read_to_string(root.join(".comline/FORMAT_VERSION")).unwrap()
}

fn migrate_error(error: &eyre::Report) -> &MigrateError {
    error
        .downcast_ref::<MigrateError>()
        .unwrap_or_else(|| panic!("not a MigrateError: {error:#}"))
}

// ---------- the plan ----------

#[test]
fn the_plan_runs_the_step_over_every_version_and_writes_nothing() {
    let (dir, commits) = t0_project();
    let before = snapshot(dir.path());
    let engine = engine();

    let check = engine.check(dir.path(), &options());
    assert_eq!(check.exit_code(), Check::EXIT_WOULD_MIGRATE);
    let Check::WouldMigrate(plan) = check else {
        panic!("{check:?}")
    };

    assert_eq!((plan.from, plan.to, plan.assumed), (T0, T1, false));
    assert_eq!(plan.steps, [(T0, T1)]);
    let names: Vec<&str> = plan.versions.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(names, ["0.1.0-dev.1", "0.1.0-dev.2", "0.1.0"]);
    assert_eq!(
        plan.versions.iter().map(|v| v.old).collect::<Vec<_>>(),
        commits,
        "oldest first, parents before children"
    );
    assert!(
        plan.versions.iter().all(|v| v.old != v.new),
        "each commit's hash moves"
    );
    // Each version has the one unknown the step declares, with its reason.
    assert_eq!(plan.unknowns.len(), 3);
    assert!(plan
        .unknowns
        .iter()
        .all(|u| u.record.path == "/label" && u.record.reason.contains("no labels")));
    // The value blobs encode the same in both formats (CBOR integers have no
    // width), so they are kept, not rewritten.
    assert!(plan.objects_unchanged >= 3, "{plan}");
    assert!(plan.objects_rewritten > 0);
    let shown = plan.to_string();
    assert!(shown.contains("would migrate 90 -> 91"), "{shown}");
    assert!(shown.contains("nothing deleted"), "{shown}");
    assert!(shown.contains(".comline.format-90.bak"), "{shown}");

    assert_eq!(
        snapshot(dir.path()),
        before,
        "--check writes nothing: no lock, no directory"
    );
}

#[test]
fn a_command_that_finds_the_store_stops_naming_migrate_and_offers_it() {
    let (dir, _) = t0_project();
    let registry = registry(Mode::Honest);

    let error = ensure_compatible_store_with(dir.path(), &registry).unwrap_err();
    assert_eq!(error, StoreError::NeedsMigration { found: T0 });
    assert!(
        error.to_string().contains("`comline store migrate`"),
        "{error}"
    );
    assert!(error.to_string().contains("--check"), "{error}");
    assert_eq!(error.command(), Some("comline store migrate"));
    let offer = error.offer().unwrap();
    assert_eq!(offer.command, "comline store migrate");
    assert!(
        offer.question.contains("IR format 90"),
        "{}",
        offer.question
    );
}

#[test]
fn a_step_that_loses_something_is_caught_by_its_witness_and_nothing_is_written() {
    let (dir, _) = t0_project();
    let before = snapshot(dir.path());
    let mut engine = Engine::new(registry(Mode::Forgetful));

    let Check::Refused(error) = engine.check(dir.path(), &options()) else {
        panic!()
    };
    assert!(
        matches!(migrate_error(&error), MigrateError::Witness { .. }),
        "{error:#}"
    );
    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::Witness { .. }),
        "{error:#}"
    );
    assert!(error.to_string().contains("90-91"), "{error:#}");

    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn an_unknown_the_step_does_not_declare_fails_the_plan() {
    let (dir, _) = t0_project();
    let before = snapshot(dir.path());
    let mut engine = Engine::new(registry(Mode::Undeclared));

    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    match migrate_error(&error) {
        MigrateError::UndeclaredUnknown { path, .. } => assert_eq!(path, "/label"),
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_store_in_a_format_the_registry_cannot_decode_is_refused() {
    let (dir, _) = t0_project();
    // Registry has the step but not the format to decode with.
    let registry = Registry::new(T1)
        .with_format(Format1)
        .with_step(Widen(Mode::Honest));
    let before = snapshot(dir.path());
    let mut engine = Engine::new(registry);

    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert_eq!(
        ensure_compatible_store_with(dir.path(), engine.registry()),
        Err(StoreError::Older { found: T0 }),
        "no chain it can read from there"
    );
    assert!(error.downcast_ref::<StoreError>().is_some(), "{error:#}");
    assert_eq!(snapshot(dir.path()), before);
}

// ---------- the migration ----------

#[test]
fn migrate_writes_beside_verifies_swaps_and_keeps_the_old_store() {
    let (dir, commits) = t0_project();
    let backup = dir.path().join(".comline.format-90.bak");
    let old_objects = snapshot(&dir.path().join(".comline/objects"));
    let mut engine = engine();

    let migrated = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    let Migrated::Done(report) = migrated else {
        panic!("{migrated:?}")
    };

    assert_eq!((report.from, report.to, report.versions), (T0, T1, 3));
    assert!(!report.resumed);
    assert_eq!(report.backup, backup);
    assert_eq!(
        report.map.iter().map(|(old, _)| *old).collect::<Vec<_>>(),
        commits
    );

    // The old store is whole in the backup: its stamp, refs and every object.
    assert_eq!(
        fs::read_to_string(backup.join("FORMAT_VERSION")).unwrap(),
        "90"
    );
    for (path, bytes) in &old_objects {
        assert_eq!(
            &snapshot(&backup.join("objects"))
                .get(path)
                .cloned()
                .flatten(),
            bytes,
            "{path:?}"
        );
    }
    assert!(!backup.join("deps-cache").exists());

    // The new store is in place and stamped, with the cache across.
    assert_eq!(stamp(dir.path()), "91");
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
    assert_eq!(
        fs::read_to_string(dir.path().join(".comline/MIGRATIONS")).unwrap(),
        "90 -> 91\n"
    );
    assert!(!dir.path().join(".comline.migrating").exists());
    assert!(!dir.path().join(".comline/JOURNAL").exists());
    assert_eq!(
        ensure_compatible_store_with(dir.path(), engine.registry()),
        Ok(StoreState::Current)
    );
    assert_eq!(engine.verify(dir.path()).unwrap().versions, 3);

    // The history is the same history: versions, commit fields, parents and
    // the release's `since`, through the old-to-new map.
    let old = Container::open(&backup).unwrap();
    let new = Container::open(&dir.path().join(".comline")).unwrap();
    assert_eq!(old.commits.len(), 3);
    assert_eq!(new.commits.len(), 3);
    for (index, (o, n)) in old.commits.iter().zip(&new.commits).enumerate() {
        assert_eq!(report.map[index], (o.hash, n.hash));
        assert_eq!(o.commit.version, n.commit.version);
        assert_eq!(o.commit.author, n.commit.author);
        assert_eq!(o.commit.timestamp, n.commit.timestamp);
        assert_eq!(o.commit.message, n.commit.message);
    }
    assert_eq!(new.commits[1].commit.parents, vec![new.commits[0].hash]);
    let release = new.commits[2].commit.release.as_ref().unwrap();
    assert_eq!(release.since, Some(new.commits[0].hash));
    assert_eq!(release.reason.as_deref(), Some("raised"));
    assert_eq!(release.applied, VersionBump::Minor);
    let head = fs::read_to_string(dir.path().join(".comline/refs/heads/main")).unwrap();
    assert_eq!(head, new.commits[2].hash.to_hex());

    // The payloads decode in the new format, and the unknown is the step's.
    let status = engine.status(dir.path()).unwrap();
    let versions = status.versions.unwrap();
    assert!(versions.iter().all(|v| v.unknowns == Some(1)));
    assert!(versions[2].release);
    assert_eq!(status.backups.len(), 1);
    assert_eq!(status.backups[0].format, Some(T0));

    // Doing it again does nothing.
    let after = snapshot(dir.path());
    assert_eq!(
        engine
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap(),
        Migrated::NothingToDo
    );
    assert_eq!(snapshot(dir.path()), after);
}

#[test]
fn migrating_the_same_store_twice_gives_the_same_files() {
    let (a, _) = t0_project();
    let (b, _) = t0_project();
    engine()
        .migrate(a.path(), &options(), Consent::Command)
        .unwrap();
    engine()
        .migrate(b.path(), &options(), Consent::Command)
        .unwrap();
    assert_eq!(snapshot(a.path()), snapshot(b.path()));
}

#[test]
fn a_history_that_a_git_merge_left_with_two_heads_keeps_both() {
    let dir = TempDir::new().unwrap();
    let commits = t0_store(dir.path(), &[5, 6]);
    let store = ObjectStore::new(dir.path());
    // A sibling of the second version, built on the first.
    let sibling = t0_commit(&store, 9, 1, vec![commits[0]]);
    let sibling = store.write(&sibling.to_bytes().unwrap()).unwrap();
    fs::write(
        dir.path().join(".comline/refs/heads/main"),
        format!(
            "<<<<<<< ours\n{}\n=======\n{sibling}\n>>>>>>> theirs\n",
            commits[1]
        ),
    )
    .unwrap();

    let mut engine = engine();
    let Migrated::Done(report) = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap()
    else {
        panic!()
    };

    assert_eq!(
        report.versions, 3,
        "the commit only the other side has is carried too"
    );
    let text = fs::read_to_string(dir.path().join(".comline/refs/heads/main")).unwrap();
    let Heads::Two(ours, theirs) = parse_heads(&text).unwrap() else {
        panic!("{text}")
    };
    let map: std::collections::HashMap<Hash, Hash> = report.map.iter().copied().collect();
    assert_eq!((ours, theirs), (map[&commits[1]], map[&sibling]));
    assert_eq!(engine.verify(dir.path()).unwrap().versions, 3);
}

#[test]
fn a_migration_is_read_in_memory_by_a_command_that_only_reads() {
    let (dir, commits) = t0_project();
    let before = snapshot(dir.path());
    let engine = engine();

    let reading = engine.open_for_reading(dir.path()).unwrap();

    assert_eq!(reading.migrated_from, Some(T0));
    let Heads::One(head) = reading.refs[REF_HEAD_MAIN_FILENAME] else {
        panic!()
    };
    assert_ne!(head, commits[2], "the head is the migrated one");
    let commit = Commit::from_bytes(&reading.store.read(&head).unwrap()).unwrap();
    assert_eq!(commit.version, "0.1.0");
    assert_eq!(snapshot(dir.path()), before, "it changes nothing on disk");
}

// ---------- crashes ----------

/// Every point a migration reaches, uninterrupted.
fn points_of_a_migration() -> usize {
    let (dir, _) = t0_project();
    let count = Rc::new(Cell::new(0usize));
    let counter = count.clone();
    let mut engine = Engine::new(registry(Mode::Honest)).on_point(move |_| {
        counter.set(counter.get() + 1);
        Ok(())
    });
    engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    count.get()
}

/// An engine that stops (an error, nothing cleaned up: what a kill leaves)
/// at the `n`th point it reaches.
fn engine_killed_at(n: usize) -> Engine {
    let reached = Rc::new(Cell::new(0usize));
    Engine::new(registry(Mode::Honest)).on_point(move |_| {
        let at = reached.get();
        reached.set(at + 1);
        if at == n {
            Err(eyre!("killed"))
        } else {
            Ok(())
        }
    })
}

/// An engine that stops when it reaches `point`.
fn engine_killed_at_point(point: Point) -> Engine {
    Engine::new(registry(Mode::Honest)).on_point(move |reached| {
        if reached == point {
            Err(eyre!("killed"))
        } else {
            Ok(())
        }
    })
}

#[test]
fn a_migration_killed_at_any_point_is_named_by_other_commands_and_resumes_to_the_same_files() {
    let reference = {
        let (dir, _) = t0_project();
        engine()
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap();
        snapshot(dir.path())
    };
    let total = points_of_a_migration();
    assert!(
        total > 10,
        "{total} points: a write each object, ref, stamp, rename"
    );

    for killed_at in 0..total {
        let (dir, _) = t0_project();
        let error = engine_killed_at(killed_at)
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap_err();
        assert!(
            error.to_string().contains("killed"),
            "{killed_at}: {error:#}"
        );

        // Another command finds it unfinished, names the command, and does
        // nothing: unless the last thing, removing the journal, was done.
        let found = ensure_compatible_store_with(dir.path(), &registry(Mode::Honest));
        if killed_at + 1 == total {
            assert_eq!(found, Ok(StoreState::Current), "{killed_at}");
        } else {
            let error = found.unwrap_err();
            assert!(
                matches!(
                    error,
                    StoreError::Interrupted {
                        what: Interrupted::Migration
                    } | StoreError::NeedsMigration { .. }
                ),
                "{killed_at}: {error}"
            );
            let error = error.to_string();
            assert!(
                error.contains("comline store migrate"),
                "{killed_at}: {error}"
            );
        }
        // Until the swap the old store is where it was.
        if killed_at < total - 4 {
            assert_eq!(stamp(dir.path()), "90", "{killed_at}");
        }

        // Running the command again finishes it, to the same bytes.
        let finished = engine()
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap();
        match finished {
            Migrated::Done(report) => assert!(report.resumed, "{killed_at}"),
            Migrated::NothingToDo => assert_eq!(killed_at + 1, total, "{killed_at}"),
        }
        assert_eq!(
            snapshot(dir.path()),
            reference,
            "killed at point {killed_at}"
        );
    }
}

#[test]
fn a_partial_directory_of_a_store_that_moved_on_is_not_resumed_over() {
    let (dir, commits) = t0_project();
    let error = engine_killed_at(3)
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(error.to_string().contains("killed"));

    // The history moved on meanwhile (here: the ref was moved back).
    update_ref(dir.path(), REF_HEAD_MAIN_FILENAME, &commits[1]).unwrap();
    let before = snapshot(dir.path());
    let mut engine = engine();

    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(
        matches!(
            migrate_error(&error),
            MigrateError::Leftover {
                why: Leftover::Diverged,
                ..
            }
        ),
        "{error:#}"
    );
    assert!(error.to_string().contains("--restart"), "{error:#}");
    assert_eq!(
        snapshot(dir.path()),
        before,
        "nothing was removed or changed"
    );

    // The user says to start over: the partial directory goes, the store is
    // migrated as it is now.
    let restart = MigrateOptions {
        restart: true,
        ..options()
    };
    let Migrated::Done(report) = engine
        .migrate(dir.path(), &restart, Consent::Command)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(report.versions, 2);
    assert!(!report.resumed);
}

/// Every file under `dir/objects` that `other/objects` doesn't have.
fn objects_not_in(dir: &Path, other: &Path) -> Vec<PathBuf> {
    files_under(&dir.join("objects"))
        .into_iter()
        .filter(|relative| !other.join("objects").join(relative).exists())
        .map(|relative| dir.join("objects").join(relative))
        .collect()
}

#[test]
fn a_new_store_that_fails_verification_is_kept_marked_and_the_store_is_untouched() {
    let (dir, _) = t0_project();
    let store = dir.path().join(".comline");
    let migrating = dir.path().join(".comline.migrating");
    // Killed after the stamp, before verification.
    let error = engine_killed_at_point(Point::StampWritten)
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(error.to_string().contains("killed"));
    assert_eq!(stamp(dir.path()), "90");

    // An object of the new store is damaged (one the old store doesn't share:
    // writing through a link would damage it too).
    let victim = objects_not_in(&migrating, &store)
        .into_iter()
        .next()
        .unwrap();
    fs::write(&victim, b"not an object").unwrap();
    let old_before = snapshot(&store);
    let mut engine = engine();

    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::Damaged { .. }),
        "{error:#}"
    );
    assert!(
        migrating.join("FAILED").is_file(),
        "marked, with the reason"
    );
    assert_eq!(snapshot(&store), old_before, "the store is as it was");
    assert_eq!(stamp(dir.path()), "90");

    // Without the user's say-so it is neither resumed nor removed.
    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(
        matches!(
            migrate_error(&error),
            MigrateError::Leftover {
                why: Leftover::Failed(_),
                ..
            }
        ),
        "{error:#}"
    );
    assert!(migrating.is_dir());

    let restart = MigrateOptions {
        restart: true,
        ..options()
    };
    assert!(matches!(
        engine
            .migrate(dir.path(), &restart, Consent::Command)
            .unwrap(),
        Migrated::Done(_)
    ));
    assert_eq!(stamp(dir.path()), "91");
    assert!(!migrating.exists());
}

// ---------- reset and prune over a store with a path ----------

#[test]
fn prune_removes_the_backup_only_when_the_new_store_verifies() {
    let (dir, _) = t0_project();
    let mut engine = engine();
    engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    let backup = dir.path().join(".comline.format-90.bak");
    assert!(backup.is_dir());

    // A damaged new store: the backup is the only whole copy, and stays.
    let objects = dir.path().join(".comline/objects");
    let victim = files_under(&objects).into_iter().next().unwrap();
    let victim = objects.join(victim);
    let saved = fs::read(&victim).unwrap();
    fs::remove_file(&victim).unwrap();
    let error = engine.prune(dir.path(), Consent::Command).unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::PruneRefused { .. }),
        "{error:#}"
    );
    assert!(backup.is_dir());

    // Repaired, it goes, and only it.
    fs::write(&victim, saved).unwrap();
    let pruned = engine.prune(dir.path(), Consent::Command).unwrap();
    assert_eq!(pruned.removed.len(), 1);
    assert!(!backup.exists());
    assert!(backups(dir.path()).unwrap().is_empty());
    assert_eq!(engine.verify(dir.path()).unwrap().versions, 3);
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
}

#[test]
fn prune_refuses_while_a_migration_is_unfinished() {
    let (dir, _) = t0_project();
    let mut engine = engine();
    engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    fs::create_dir(dir.path().join(".comline.migrating")).unwrap();

    let error = engine.prune(dir.path(), Consent::Command).unwrap_err();

    assert!(
        matches!(migrate_error(&error), MigrateError::PruneRefused { .. }),
        "{error:#}"
    );
    assert!(dir.path().join(".comline.format-90.bak").is_dir());
}

fn points_of_a_reset() -> usize {
    let (dir, _) = t0_project();
    let count = Rc::new(Cell::new(0usize));
    let counter = count.clone();
    let mut engine = Engine::new(registry(Mode::Honest)).on_point(move |_| {
        counter.set(counter.get() + 1);
        Ok(())
    });
    engine.reset(dir.path(), Consent::Command).unwrap();
    count.get()
}

#[test]
fn a_reset_killed_at_any_point_is_finished_by_running_it_again() {
    let reference = {
        let (dir, _) = t0_project();
        engine().reset(dir.path(), Consent::Command).unwrap();
        snapshot(dir.path())
    };
    assert!(reference.contains_key(&PathBuf::from(".comline.format-90.bak/FORMAT_VERSION")));
    assert!(reference.contains_key(&PathBuf::from(".comline/deps-cache/dep-1234/config.idp")));
    assert!(!reference.contains_key(&PathBuf::from(".comline/FORMAT_VERSION")));
    let total = points_of_a_reset();
    assert_eq!(total, 4);

    for killed_at in 0..total {
        let (dir, _) = t0_project();
        let reached = Rc::new(Cell::new(0usize));
        let mut killed = Engine::new(registry(Mode::Honest)).on_point(move |_| {
            let at = reached.get();
            reached.set(at + 1);
            if at == killed_at {
                Err(eyre!("killed"))
            } else {
                Ok(())
            }
        });
        let error = killed.reset(dir.path(), Consent::Command).unwrap_err();
        assert!(
            error.to_string().contains("killed"),
            "{killed_at}: {error:#}"
        );

        let found = ensure_compatible_store_with(dir.path(), &registry(Mode::Honest));
        if killed_at + 1 < total {
            assert_eq!(
                found,
                Err(StoreError::Interrupted {
                    what: Interrupted::Reset
                }),
                "{killed_at}"
            );
        }

        match engine().reset(dir.path(), Consent::Command) {
            Ok(report) => assert!(report.resumed, "{killed_at}"),
            // Killed after the last rename: it was done.
            Err(error) => {
                assert_eq!(killed_at + 1, total, "{killed_at}");
                assert!(matches!(
                    migrate_error(&error),
                    MigrateError::NothingToReset
                ));
            }
        }
        assert_eq!(
            snapshot(dir.path()),
            reference,
            "killed at point {killed_at}"
        );
    }
}
