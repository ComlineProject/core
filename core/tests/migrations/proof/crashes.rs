// Crash injection (core#268, step 1.4; proof.md, "Test-only formats" and the
// idempotence law): the engine on test-only stores, killed with the hook of
// `Engine::on_point` after every write and rename of a migration, and again
// while it resumes. An `Err` from the hook stops the engine on the spot with
// nothing cleaned up, which is what a kill leaves. What has to hold at each
// point: another command finds the migration and names the command that
// finishes it, the old store is untouched until the swap, and running the
// command again ends in the same files an uninterrupted run makes, which are
// what a fresh build of the new format is (the square).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use comline_core::package::build::cas::migrate::{
    Consent, Engine, MigrateOptions, Migrated, Point,
};
use comline_core::package::build::cas::object_store::{
    ensure_compatible_store_with, Interrupted, StoreError, StoreState,
};
use eyre::eyre;

use super::super::support::snapshot;
use super::formats::*;
use super::histories::{history, project};
use super::square::*;
use super::steps::*;

/// A history that makes releases and two chained steps; one with two heads
/// and one step; one that is only unknowns of the typed kind.
const CASES: [(&str, u32, u32); 3] = [("releases", T0, T2), ("fork", T0, T1), ("grows", T1, T2)];

fn engine(to: u32) -> Engine {
    Engine::new(honest(to))
}

fn options() -> MigrateOptions {
    MigrateOptions::default()
}

/// The engine, stopped when it reaches its `n`th point.
fn killed_at(to: u32, n: usize) -> Engine {
    let reached = Rc::new(Cell::new(0usize));
    engine_with_hook(to, move |_| {
        let at = reached.get();
        reached.set(at + 1);
        if at == n {
            Err(eyre!("killed"))
        } else {
            Ok(())
        }
    })
}

fn engine_with_hook(to: u32, hook: impl FnMut(Point) -> eyre::Result<()> + 'static) -> Engine {
    engine(to).on_point(hook)
}

/// Every point an uninterrupted migration reaches, in order.
fn points(name: &str, from: u32, to: u32) -> Vec<Point> {
    let dir = project(&history(name), from);
    let log: Rc<RefCell<Vec<Point>>> = Rc::default();
    let sink = log.clone();
    let mut counting = engine_with_hook(to, move |point| {
        sink.borrow_mut().push(point);
        Ok(())
    });
    counting
        .migrate(dir.path(), &options(), Consent::Command)
        .unwrap();
    let reached = log.borrow().clone();
    reached
}

#[test]
fn a_migration_reaches_every_point_of_the_procedure_in_order() {
    let reached = points("releases", T0, T2);
    assert!(reached.contains(&Point::ObjectWritten(0)));
    let order = [
        Point::Claimed,
        Point::RefWritten,
        Point::ReceiptWritten,
        Point::StampWritten,
        Point::Verified,
        Point::Ready,
        Point::OldStoreMoved,
        Point::NewStorePlaced,
        Point::CacheMoved,
        Point::JournalRemoved,
    ];
    let at: Vec<usize> = order
        .iter()
        .map(|point| {
            reached
                .iter()
                .position(|reached| reached == point)
                .unwrap_or_else(|| panic!("{point:?} was never reached"))
        })
        .collect();
    assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
    assert_eq!(*reached.last().unwrap(), Point::JournalRemoved);
}

#[test]
fn a_migration_killed_at_every_point_is_named_by_other_commands_and_resumes_to_the_same_files() {
    for (name, from, to) in CASES {
        let history = history(name);
        let reference = {
            let dir = project(&history, from);
            engine(to)
                .migrate(dir.path(), &options(), Consent::Command)
                .unwrap();
            snapshot(dir.path())
        };
        let reached = points(name, from, to);
        let total = reached.len();
        let swap = reached
            .iter()
            .position(|point| *point == Point::OldStoreMoved)
            .unwrap();
        assert!(total > 10, "{name}: {total} points");

        for killed in 0..total {
            let at = format!("{name} ({from} -> {to}), killed at point {killed} of {total}");
            let dir = project(&history, from);
            let original = snapshot(&dir.path().join(".comline"));
            let error = killed_at(to, killed)
                .migrate(dir.path(), &options(), Consent::Command)
                .unwrap_err();
            assert!(error.to_string().contains("killed"), "{at}: {error:#}");

            // Another command finds it unfinished and names the command,
            // unless the last thing, removing the journal, was done.
            let found = ensure_compatible_store_with(dir.path(), &honest(to));
            if killed + 1 == total {
                assert_eq!(found, Ok(StoreState::Current), "{at}");
            } else {
                let error = found.unwrap_err();
                assert!(
                    matches!(
                        error,
                        StoreError::Interrupted {
                            what: Interrupted::Migration
                        } | StoreError::NeedsMigration { .. }
                    ),
                    "{at}: {error}"
                );
                assert!(error.to_string().contains("comline store migrate"), "{at}");
            }
            // Until the swap the old store is exactly as it was.
            if killed < swap {
                assert_eq!(
                    snapshot(&dir.path().join(".comline")),
                    original,
                    "{at}: the store moved before the swap"
                );
            }

            // Running the command again finishes it, to the same bytes.
            match engine(to)
                .migrate(dir.path(), &options(), Consent::Command)
                .unwrap()
            {
                Migrated::Done(report) => assert!(report.resumed, "{at}"),
                Migrated::NothingToDo => assert_eq!(killed + 1, total, "{at}"),
            }
            assert!(
                snapshot(dir.path()) == reference,
                "{at}: not the same files"
            );

            // And what it made is what a fresh build of the new format is.
            let built = project(&history, to);
            if let Err(failure) = compare_stores(
                &dir.path().join(".comline"),
                &built.path().join(".comline"),
                to,
                &declared(from, to, Defect::None, Defect::None),
            ) {
                panic!("{at}: {failure:?}");
            }
        }
    }
}

#[test]
fn a_migration_killed_again_while_it_resumes_still_finishes_to_the_same_files() {
    let (name, from, to) = ("releases", T0, T2);
    let history = history(name);
    let reference = {
        let dir = project(&history, from);
        engine(to)
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap();
        snapshot(dir.path())
    };
    let total = points(name, from, to).len();

    for killed in 0..total {
        let dir = project(&history, from);
        let _ = killed_at(to, killed).migrate(dir.path(), &options(), Consent::Command);
        // The resume is killed at its second point (or finishes, if it has
        // fewer), and the next run finishes what is left.
        let second = killed_at(to, 1).migrate(dir.path(), &options(), Consent::Command);
        if let Err(error) = &second {
            assert!(error.to_string().contains("killed"), "{killed}: {error:#}");
        }
        engine(to)
            .migrate(dir.path(), &options(), Consent::Command)
            .unwrap();
        assert!(
            snapshot(dir.path()) == reference,
            "killed at {killed}, and again: not the same files"
        );
    }
}
