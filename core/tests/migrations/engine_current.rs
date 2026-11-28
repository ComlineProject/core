// The engine on the format this comline writes (core#268, step 1.3). There is
// no step out of format 3, so the plan of a format-3 store is empty; what is
// tested here is what can be: that nothing is done to a current store, that
// a newer one and one with no path are refused and never touched, that
// `verify` and `status` read a real store, that a store that lost its stamp
// can be stamped by the user, and `reset` and `prune` on real stores.

use std::fs;
use std::path::Path;

use comline_core::package::build::build;
use comline_core::package::build::cas::migrate::{
    backups, Check, Consent, Engine, Leftover, MigrateError, MigrateOptions, Migrated,
};
use comline_core::package::build::cas::object_store::{
    ensure_compatible_store, Interrupted, StoreError, StoreState, CURRENT_IR_FORMAT_VERSION,
};
use tempfile::TempDir;

use super::support::snapshot;

fn package() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.idp"), "congregation test\nspecification_version = 1\nsettings = {\n    bounds = {\n        string = {\n            max = 256\n        }\n        array = {\n            max = 1024\n        }\n    }\n}\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/ping.ids"),
        "struct Foo {\n    a: u32\n}\n",
    )
    .unwrap();
    dir
}

/// A package built twice, with a fetched dependency in `deps-cache/`.
fn built_package() -> TempDir {
    let dir = package();
    build(dir.path()).unwrap();
    fs::write(
        dir.path().join("src/ping.ids"),
        "struct Foo {\n    a: u32\n}\nstruct Bar {\n    b: u32\n}\n",
    )
    .unwrap();
    build(dir.path()).unwrap();
    let cached = dir.path().join(".comline/deps-cache/dep-1234");
    fs::create_dir_all(cached.join("src")).unwrap();
    fs::write(cached.join("config.idp"), "a fetched dependency").unwrap();
    dir
}

fn stamp(dir: &Path) -> std::path::PathBuf {
    dir.join(".comline/FORMAT_VERSION")
}

fn options() -> MigrateOptions {
    MigrateOptions::default()
}

fn store_error(error: &eyre::Report) -> &StoreError {
    error
        .downcast_ref::<StoreError>()
        .unwrap_or_else(|| panic!("not a StoreError: {error:#}"))
}

fn migrate_error(error: &eyre::Report) -> &MigrateError {
    error
        .downcast_ref::<MigrateError>()
        .unwrap_or_else(|| panic!("not a MigrateError: {error:#}"))
}

#[test]
fn a_current_store_has_nothing_to_migrate_and_nothing_is_touched() {
    let dir = built_package();
    let before = snapshot(dir.path());
    let mut engine = Engine::current();

    let check = engine.check(dir.path(), &options());
    assert!(matches!(check, Check::NothingToDo), "{check:?}");
    assert_eq!(check.exit_code(), Check::EXIT_NOTHING_TO_DO);
    assert!(engine.plan(dir.path(), &options()).unwrap().is_none());

    let migrated = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    assert_eq!(migrated, Migrated::NothingToDo);

    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_package_with_no_store_has_nothing_to_migrate() {
    let dir = package();
    let engine = Engine::current();
    assert!(matches!(
        engine.check(dir.path(), &options()),
        Check::NothingToDo
    ));
    assert!(!dir.path().join(".comline").exists());
}

#[test]
fn verify_reads_a_real_store_and_status_lists_it() {
    let dir = built_package();
    let engine = Engine::current();

    let report = engine.verify(dir.path()).unwrap();
    assert_eq!(report.format, CURRENT_IR_FORMAT_VERSION);
    assert_eq!(report.versions, 2);
    assert!(report.objects > 2);
    assert!(
        report.not_canonical.is_empty(),
        "a store the current core built is what the current encoder writes: {:?}",
        report.not_canonical
    );

    let status = engine.status(dir.path()).unwrap();
    assert_eq!(status.target, CURRENT_IR_FORMAT_VERSION);
    assert_eq!(status.stamp, Some(CURRENT_IR_FORMAT_VERSION));
    assert_eq!(status.classification, Ok(StoreState::Current));
    assert!(!status.needs_migration());
    let versions = status.versions.unwrap();
    let names: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(names, ["0.1.0-dev.1", "0.1.0-dev.2"]);
    assert!(versions.iter().all(|v| v.unknowns == Some(0)));
    assert!(status.backups.is_empty());
}

#[test]
fn verify_names_the_file_of_a_damaged_store() {
    let dir = built_package();
    let engine = Engine::current();
    let objects = dir.path().join(".comline/objects");
    let victim = fs::read_dir(&objects)
        .unwrap()
        .flat_map(|d| fs::read_dir(d.unwrap().path()).unwrap())
        .map(|f| f.unwrap().path())
        .next()
        .unwrap();
    fs::remove_file(&victim).unwrap();
    let before = snapshot(dir.path());

    let error = engine.verify(dir.path()).unwrap_err();

    assert!(
        matches!(migrate_error(&error), MigrateError::Damaged { .. }),
        "{error:#}"
    );
    assert!(error.to_string().contains("is missing"), "{error:#}");
    assert!(
        error
            .to_string()
            .contains(&victim.file_name().unwrap().to_string_lossy().to_string()),
        "{error:#}"
    );
    assert_eq!(snapshot(dir.path()), before);

    let status = engine.status(dir.path()).unwrap();
    assert!(status.versions.is_err());
}

#[test]
fn a_newer_store_is_refused_by_every_operation_and_never_touched() {
    let dir = built_package();
    let newer = CURRENT_IR_FORMAT_VERSION + 1;
    fs::write(stamp(dir.path()), newer.to_string()).unwrap();
    let before = snapshot(dir.path());
    let mut engine = Engine::current();

    let check = engine.check(dir.path(), &options());
    assert_eq!(check.exit_code(), Check::EXIT_REFUSED);
    match check {
        Check::Refused(error) => {
            assert_eq!(store_error(&error), &StoreError::Newer { found: newer })
        }
        other => panic!("{other:?}"),
    }
    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert_eq!(store_error(&error), &StoreError::Newer { found: newer });

    let error = engine.reset(dir.path(), Consent::Command).unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::ResetRefused { .. }),
        "{error:#}"
    );
    assert!(engine.reset_plan(dir.path()).is_err());
    assert!(engine.verify(dir.path()).is_err());

    // `--assume-format` is for a store that lost its stamp, not for this.
    let assumed = MigrateOptions {
        assume_format: Some(CURRENT_IR_FORMAT_VERSION),
        ..options()
    };
    let error = engine
        .migrate(dir.path(), &assumed, Consent::Command)
        .unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::Assume { .. }),
        "{error:#}"
    );

    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn an_older_store_with_no_path_is_refused_naming_reset_and_never_touched() {
    let dir = built_package();
    fs::write(stamp(dir.path()), "2").unwrap();
    let before = snapshot(dir.path());
    let mut engine = Engine::current();

    let error = engine.plan(dir.path(), &options()).unwrap_err();
    assert_eq!(store_error(&error), &StoreError::Older { found: 2 });
    assert!(
        error.to_string().contains("`comline store reset`"),
        "{error}"
    );
    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert_eq!(store_error(&error), &StoreError::Older { found: 2 });

    // It is never offered to a terminal: a reset is the user's own command.
    let older = StoreError::Older { found: 2 };
    assert_eq!(older.offer(), None);
    assert_eq!(older.command(), Some("comline store reset"));

    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_store_that_lost_its_stamp_is_stamped_only_by_the_user_naming_a_format() {
    let dir = built_package();
    fs::remove_file(stamp(dir.path())).unwrap();
    let before = snapshot(dir.path());
    let mut engine = Engine::current();

    // Without the option it is refused, naming both ways on.
    let error = engine.plan(dir.path(), &options()).unwrap_err();
    assert_eq!(store_error(&error), &StoreError::Unstamped);
    assert!(error.to_string().contains("--assume-format"), "{error}");

    // `--check` with it shows what would happen, and writes nothing.
    let assumed = MigrateOptions {
        assume_format: Some(CURRENT_IR_FORMAT_VERSION),
        ..options()
    };
    let check = engine.check(dir.path(), &assumed);
    assert_eq!(check.exit_code(), Check::EXIT_WOULD_MIGRATE);
    let Check::WouldMigrate(plan) = check else {
        panic!()
    };
    assert_eq!((plan.from, plan.to, plan.assumed), (3, 3, true));
    assert!(plan.steps.is_empty());
    assert_eq!(plan.versions.len(), 2);
    assert!(plan.unknowns.is_empty());
    assert_eq!(plan.objects_rewritten, 0, "nothing in the objects changes");
    assert!(plan.to_string().contains("nothing deleted"), "{plan}");
    assert_eq!(snapshot(dir.path()), before);

    // Naming a format the objects don't decode as stops it.
    let wrong = MigrateOptions {
        assume_format: Some(CURRENT_IR_FORMAT_VERSION + 1),
        ..options()
    };
    let error = engine
        .migrate(dir.path(), &wrong, Consent::Command)
        .unwrap_err();
    assert_eq!(
        store_error(&error),
        &StoreError::Newer {
            found: CURRENT_IR_FORMAT_VERSION + 1
        }
    );

    let migrated = engine
        .migrate(dir.path(), &assumed, Consent::Command)
        .unwrap();
    let Migrated::Done(report) = migrated else {
        panic!("{migrated:?}")
    };
    assert_eq!((report.from, report.to, report.versions), (3, 3, 2));
    assert!(!report.resumed);
    assert!(
        report.map.iter().all(|(old, new)| old == new),
        "same bytes, same hashes"
    );
    assert!(report.backup.ends_with(".comline.format-3.bak"));
    assert!(report.backup.is_dir());

    // The new store is stamped and verifies; the cache is in it, not in the
    // backup; the backup is the old store as it was, stamp and all.
    assert_eq!(
        fs::read_to_string(stamp(dir.path())).unwrap(),
        CURRENT_IR_FORMAT_VERSION.to_string()
    );
    assert_eq!(engine.verify(dir.path()).unwrap().versions, 2);
    assert_eq!(ensure_compatible_store(dir.path()), Ok(StoreState::Current));
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
    assert!(!report.backup.join("deps-cache").exists());
    assert!(!report.backup.join("FORMAT_VERSION").exists());
    assert!(report.backup.join("refs/heads/main").is_file());
    assert!(!dir.path().join(".comline.migrating").exists());
    assert!(!dir.path().join(".comline/JOURNAL").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join(".comline/MIGRATIONS")).unwrap(),
        "3 -> 3\n"
    );

    // Running it again is a no-op: the stamp is current.
    let after = snapshot(dir.path());
    let error = engine
        .migrate(dir.path(), &assumed, Consent::Command)
        .unwrap_err();
    assert!(
        error.to_string().contains("stamped"),
        "the option is for an unstamped store: {error}"
    );
    assert_eq!(
        engine
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap(),
        Migrated::NothingToDo
    );
    assert_eq!(snapshot(dir.path()), after);
}

#[test]
fn a_stamp_git_left_in_conflict_between_two_formats_is_refused_by_name() {
    let dir = built_package();
    fs::write(
        stamp(dir.path()),
        "<<<<<<< HEAD\n3\n=======\n2\n>>>>>>> other\n",
    )
    .unwrap();
    let before = snapshot(dir.path());

    let found = ensure_compatible_store(dir.path());
    assert_eq!(found, Err(StoreError::MergedFormats { ours: 3, theirs: 2 }));
    let message = found.unwrap_err().to_string();
    assert!(message.contains("migrate both sides first"), "{message}");

    let mut engine = Engine::current();
    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert_eq!(
        store_error(&error),
        &StoreError::MergedFormats { ours: 3, theirs: 2 }
    );
    assert!(engine.reset(dir.path(), Consent::Command).is_err());
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn an_interrupted_migration_or_reset_is_named_by_every_other_command() {
    let dir = built_package();
    let engine = Engine::current();

    fs::create_dir(dir.path().join(".comline.migrating")).unwrap();
    let error = ensure_compatible_store(dir.path()).unwrap_err();
    assert_eq!(
        error,
        StoreError::Interrupted {
            what: Interrupted::Migration
        }
    );
    assert!(
        error.to_string().contains("`comline store migrate`"),
        "{error}"
    );
    let offer = error.offer().unwrap();
    assert_eq!(offer.command, "comline store migrate");
    assert!(
        offer.question.contains("comline store migrate"),
        "{}",
        offer.question
    );
    assert!(engine.plan(dir.path(), &options()).is_err());
    fs::remove_dir(dir.path().join(".comline.migrating")).unwrap();

    fs::create_dir(dir.path().join(".comline.resetting")).unwrap();
    let error = ensure_compatible_store(dir.path()).unwrap_err();
    assert_eq!(
        error,
        StoreError::Interrupted {
            what: Interrupted::Reset
        }
    );
    assert!(
        error.to_string().contains("`comline store reset`"),
        "{error}"
    );
    assert_eq!(error.offer(), None, "a reset is never offered inline");
    fs::remove_dir(dir.path().join(".comline.resetting")).unwrap();

    assert_eq!(ensure_compatible_store(dir.path()), Ok(StoreState::Current));
}

#[test]
fn a_migratable_store_is_offered_the_migration_and_never_a_reset() {
    let offer = StoreError::NeedsMigration { found: 2 }.offer().unwrap();
    assert_eq!(offer.command, "comline store migrate");
    assert!(offer.question.contains("IR format 2"), "{}", offer.question);
    assert!(
        offer.question.contains(".comline.format-2.bak"),
        "{}",
        offer.question
    );
    for error in [
        StoreError::Older { found: 1 },
        StoreError::Newer { found: 9 },
        StoreError::Unstamped,
        StoreError::InvalidStamp { text: "x".into() },
        StoreError::UnreadableStamp {
            reason: "denied".into(),
        },
        StoreError::MergedFormats { ours: 2, theirs: 3 },
        StoreError::Interrupted {
            what: Interrupted::Reset,
        },
    ] {
        assert_eq!(error.offer(), None, "{error}");
    }
}

#[test]
fn reset_moves_the_store_aside_keeps_the_cache_and_deletes_nothing() {
    let dir = built_package();
    fs::write(stamp(dir.path()), "2").unwrap();
    let mut engine = Engine::current();
    let objects_before = snapshot(&dir.path().join(".comline/objects"));

    let plan = engine.reset_plan(dir.path()).unwrap();
    assert_eq!(plan.format, Some(2));
    assert!(plan.backup.ends_with(".comline.format-2.bak"));
    assert!(plan.keeps_deps_cache);
    let question = plan.describe();
    assert!(question.contains(".comline.format-2.bak"), "{question}");
    assert!(question.contains("IR format 2"), "{question}");
    assert!(question.contains("Nothing is deleted"), "{question}");
    assert!(!plan.backup.exists(), "planning moves nothing");

    let report = engine.reset(dir.path(), Consent::Command).unwrap();
    assert_eq!(report.backup.as_deref(), Some(plan.backup.as_path()));

    // Every object, ref and the stamp are in the backup, as they were.
    assert_eq!(snapshot(&plan.backup.join("objects")), objects_before);
    assert_eq!(
        fs::read_to_string(plan.backup.join("FORMAT_VERSION")).unwrap(),
        "2"
    );
    assert!(plan.backup.join("refs/heads/main").is_file());
    // The cache is in the new `.comline/`, which has no history in it.
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
    assert!(!plan.backup.join("deps-cache").exists());
    assert!(!stamp(dir.path()).exists());
    assert!(!dir.path().join(".comline.resetting").exists());
    assert_eq!(ensure_compatible_store(dir.path()), Ok(StoreState::Fresh));
    assert_eq!(backups(dir.path()).unwrap().len(), 1);
    assert_eq!(backups(dir.path()).unwrap()[0].format, Some(2));

    // Nothing is left to reset, and prune has nothing that replaced them.
    let error = engine.reset(dir.path(), Consent::Command).unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::NothingToReset),
        "{error:#}"
    );
    let error = engine.prune(dir.path(), Consent::Command).unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::PruneRefused { .. }),
        "{error:#}"
    );
    assert!(plan.backup.is_dir());

    // The next build starts a new history. Only then may the backup go.
    let built = build(dir.path()).unwrap();
    assert_eq!(built.current_version, "0.1.0-dev.1");
    assert_eq!(built.previous_version, None);
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());

    let pruned = engine.prune(dir.path(), Consent::Command).unwrap();
    assert_eq!(pruned.removed.len(), 1);
    assert!(!plan.backup.exists());
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
    assert!(engine
        .prune(dir.path(), Consent::Command)
        .unwrap()
        .removed
        .is_empty());
}

#[test]
fn a_second_reset_does_not_overwrite_the_first_backup() {
    let dir = built_package();
    fs::write(stamp(dir.path()), "2").unwrap();
    let mut engine = Engine::current();
    let first = engine
        .reset(dir.path(), Consent::Command)
        .unwrap()
        .backup
        .unwrap();
    let first_before = snapshot(&first);

    build(dir.path()).unwrap();
    fs::write(stamp(dir.path()), "2").unwrap();
    let second = engine
        .reset(dir.path(), Consent::Command)
        .unwrap()
        .backup
        .unwrap();

    assert_ne!(first, second);
    assert!(second.ends_with(".comline.format-2-2.bak"), "{second:?}");
    assert_eq!(snapshot(&first), first_before);
    assert_eq!(backups(dir.path()).unwrap().len(), 2);
}

#[test]
fn prune_refuses_while_the_store_that_replaced_the_backup_is_damaged() {
    let dir = built_package();
    fs::write(stamp(dir.path()), "2").unwrap();
    let mut engine = Engine::current();
    let backup = engine
        .reset(dir.path(), Consent::Command)
        .unwrap()
        .backup
        .unwrap();
    build(dir.path()).unwrap();

    // Remove an object of the new store (removing a link can't reach the
    // backup, whatever it shares).
    let victim = fs::read_dir(dir.path().join(".comline/objects"))
        .unwrap()
        .flat_map(|d| fs::read_dir(d.unwrap().path()).unwrap())
        .map(|f| f.unwrap().path())
        .next()
        .unwrap();
    fs::remove_file(victim).unwrap();

    let error = engine.prune(dir.path(), Consent::Command).unwrap_err();
    assert!(
        matches!(migrate_error(&error), MigrateError::PruneRefused { .. }),
        "{error:#}"
    );
    assert!(error.to_string().contains("verification"), "{error:#}");
    assert!(
        backup.is_dir(),
        "a backup is removed only after verification"
    );
}

#[test]
fn a_leftover_that_is_not_ours_is_not_removed_or_resumed() {
    let dir = built_package();
    let mut engine = Engine::current();
    fs::create_dir(dir.path().join(".comline.migrating")).unwrap();
    fs::write(dir.path().join(".comline.migrating/notes.txt"), "mine").unwrap();
    let before = snapshot(dir.path());

    let error = engine
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap_err();
    assert!(
        matches!(
            migrate_error(&error),
            MigrateError::Leftover {
                why: Leftover::NotAJournal,
                ..
            }
        ),
        "{error:#}"
    );
    let restart = MigrateOptions {
        restart: true,
        ..options()
    };
    assert!(engine
        .migrate(dir.path(), &restart, Consent::Command)
        .is_err());
    assert_eq!(snapshot(dir.path()), before);
}
