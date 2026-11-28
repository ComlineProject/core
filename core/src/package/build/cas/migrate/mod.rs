// The IR migration engine and what `comline store` does with it (core#268,
// step 1.3; the design is `docs/proposals/ir-migrations`).
//
// A store is migrated from the format it is stamped with to the one this
// comline writes, a step at a time, in memory: the container is read (every
// object checked against its hash), each version is decoded under its format,
// run through the chain of steps, and encoded under the target. Nothing is
// written until the whole plan exists and its checks that need no disk have
// passed. The new store is then written beside the old one
// (`.comline.migrating/`), verified, and swapped in: the old one is renamed to
// `.comline.format-N.bak/` and stays until `comline store prune`.
//
// The rule behind every function here: **nothing is deleted or changed
// without the user doing so.** Core is a library: it never decides to migrate
// or reset. Every function that changes a store on disk takes a [`Consent`],
// which the caller passes only because the user ran the command, or answered
// yes to the exact question it showed. The CLI's other commands stop with a
// [`StoreError`](super::object_store::StoreError) that names the command, and
// on a terminal may show its [`offer`](super::object_store::StoreError::offer).
//
// Layout:
// - `model`, `format`: what a step works on (`Version<P>`), the traits that
//   describe a format and a step, and the registry of both. Format 3's own
//   codec is `v3`. Test-only formats (step 1.4) implement the same traits.
// - `container`: reading a store with no format knowledge.
// - `plan`: the in-memory migration and its checks (`--check` stops here).
// - `journal`, `disk`: the on-disk state, and the only file that moves or
//   removes anything (see `core_deletes_and_moves_no_store`).
// - `run`: the procedure and its recovery. `verify`: the checks on a store
//   directory. `housekeeping`: status, backups, reset, prune.

mod container;
mod disk;
mod error;
mod format;
mod housekeeping;
mod journal;
mod model;
mod plan;
mod run;
mod v3;
mod verify;

use std::path::{Path, PathBuf};

use eyre::Result;

pub use container::{CommitNode, Container};
pub use error::{Leftover, MigrateError};
pub use format::{Format, MemObjects, Objects, PayloadBound, Registry, Sink, Step};
pub use housekeeping::{backups, Backup, Pruned, ResetPlan, ResetReport, StoreStatus, VersionInfo};
pub use journal::{Journal, JournalState};
pub use model::{path_matches, Erased, ReleaseRecord, UnknownDecl, Version, VersionId};
pub use plan::{Plan, PlannedVersion, UnknownLine};
pub use run::{Migrated, Reading, Report};
pub use v3::{Content, Node, Payload as V3Payload, V3};
pub use verify::VerifyReport;

/// The store being migrated: written beside `.comline/`, on the same
/// filesystem, so the renames of the swap are atomic. Its existence is the
/// lock.
pub fn migrating_dir(project_root: &Path) -> PathBuf {
    project_root.join(".comline.migrating")
}

/// The staging directory of a `comline store reset`.
pub fn resetting_dir(project_root: &Path) -> PathBuf {
    project_root.join(".comline.resetting")
}

/// A migration's journal, in the store directory it is writing (or, after the
/// swap and until it is finished, in `.comline/`).
pub fn journal_path(store_dir: &Path) -> PathBuf {
    store_dir.join("JOURNAL")
}

/// Why `.comline.migrating/` failed verification, kept beside what was
/// written so the user can look at it.
pub(crate) const FAILED_FILE: &str = "FAILED";

/// `.comline/MIGRATIONS`: one line per migration, the same on every machine
/// (no time, no author, no core version), so Git never conflicts on it.
pub const RECEIPT_FILE: &str = "MIGRATIONS";

/// The user's say-so. Every function that moves, writes or removes a store
/// takes one, so that no path to a change in a store exists without the
/// caller stating how the user asked for it. There is no `Default`, and no
/// flag or environment variable that makes a command pass one on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// The user ran the command that does this: `comline store migrate`,
    /// `comline store reset` (`--force` where there is no terminal to ask on)
    /// or `comline store prune`.
    Command,
    /// A command that found the store stopped, showed the user exactly what
    /// it would run (the question of a
    /// [`StoreError::offer`](super::object_store::StoreError::offer), or a
    /// reset's [`ResetPlan::describe`]) and the user said yes.
    Confirmed,
}

/// A place in a migration or a reset where a crash could leave the disk, and
/// where the engine calls the hook set with [`Engine::on_point`]. Returning an
/// error from the hook stops the engine on the spot with nothing cleaned up,
/// which is what a kill leaves: crash tests use it to stop after every write
/// and rename and resume (core#268, step 1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    /// `.comline.migrating/` made and its journal written (`writing`).
    Claimed,
    /// The `n`th object of the new store written (from 0).
    ObjectWritten(usize),
    /// A ref of the new store written.
    RefWritten,
    /// The receipt written.
    ReceiptWritten,
    /// `FORMAT_VERSION` written, last.
    StampWritten,
    /// The new store passed [verification](Engine::verify).
    Verified,
    /// The journal says `ready`.
    Ready,
    /// `.comline/` renamed to its backup.
    OldStoreMoved,
    /// `.comline.migrating/` renamed to `.comline/`.
    NewStorePlaced,
    /// `deps-cache/` moved from the backup into the new store.
    CacheMoved,
    /// The journal removed: the migration is complete.
    JournalRemoved,
    /// A reset: `.comline.resetting/` made.
    ResetStaged,
    /// A reset: `deps-cache/` moved into `.comline.resetting/`.
    ResetCacheStaged,
    /// A reset: `.comline/` renamed to its backup.
    ResetStoreMoved,
    /// A reset: `.comline.resetting/` renamed to `.comline/`.
    ResetPlaced,
}

/// Options of [`Engine::plan`], [`Engine::check`] and [`Engine::migrate`].
#[derive(Debug, Clone, Default)]
pub struct MigrateOptions {
    /// `--assume-format N`: the format of a store that has lost its stamp.
    /// Only accepted for an unstamped store; every object must decode under
    /// it.
    pub assume_format: Option<u32>,
    /// `--restart`: remove a partial `.comline.migrating/` (one that failed
    /// verification, or that the store has moved on from) and migrate again.
    pub restart: bool,
}

/// What `comline store migrate --check` found.
#[derive(Debug)]
pub enum Check {
    /// The store is in the current format (or there is none): nothing to do.
    NothingToDo,
    /// This is what a migration would do.
    WouldMigrate(Box<Plan>),
    /// The migration would be refused, and why. Nothing was written.
    Refused(eyre::Report),
}

impl Check {
    /// The exit codes of `comline store migrate --check`: distinct, so a CI
    /// guard ("the checked-in store is current") and a person before an
    /// upgrade can tell the three apart. (An I/O failure is the CLI's own
    /// error exit, `1`.)
    pub const EXIT_NOTHING_TO_DO: i32 = 0;
    /// See [`Check::exit_code`].
    pub const EXIT_WOULD_MIGRATE: i32 = 2;
    /// See [`Check::exit_code`].
    pub const EXIT_REFUSED: i32 = 3;

    /// `0` nothing to do, `2` would migrate, `3` would be refused.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NothingToDo => Self::EXIT_NOTHING_TO_DO,
            Self::WouldMigrate(_) => Self::EXIT_WOULD_MIGRATE,
            Self::Refused(_) => Self::EXIT_REFUSED,
        }
    }
}

type Hook = Box<dyn FnMut(Point) -> Result<()>>;

/// The engine: a [`Registry`] of formats and steps, and every operation of
/// `comline store` on a project's `.comline/`. [`Engine::current`] is the
/// one a release uses; tests build one over test-only formats.
pub struct Engine {
    pub(crate) registry: Registry,
    hook: Option<Hook>,
}

impl Engine {
    /// The engine for what this comline writes: format
    /// [`CURRENT_IR_FORMAT_VERSION`](super::object_store::CURRENT_IR_FORMAT_VERSION) and the steps that lead to it (none yet:
    /// format 3 is the first, so a format-3 store has an empty plan).
    pub fn current() -> Self {
        Self::new(Registry::current())
    }

    /// An engine over `registry`.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            hook: None,
        }
    }

    /// Call `hook` at every [`Point`] of a migration or reset. An `Err` from
    /// it stops the operation there, as a crash would.
    pub fn on_point(mut self, hook: impl FnMut(Point) -> Result<()> + 'static) -> Self {
        self.hook = Some(Box::new(hook));
        self
    }

    /// The formats and steps this engine has.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn reach(&mut self, point: Point) -> Result<()> {
        match &mut self.hook {
            Some(hook) => hook(point),
            None => Ok(()),
        }
    }
}
