// What can stop a migration, a reset or a prune. Like `StoreError`, these
// travel in an `eyre::Report` and are recovered with `downcast_ref`, and in
// every case the message says what was found, what was changed (nothing, or
// what is left on disk) and what to do.

use std::fmt;
use std::path::PathBuf;

use crate::package::build::cas::object_store::MIGRATE_COMMAND as MIGRATE;

const RESTART_FLAG: &str = "--restart";

/// What is left of an earlier attempt, when it stops this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leftover {
    /// `.comline.migrating/` holds something that isn't a migration's
    /// journal: it isn't ours to resume or remove.
    NotAJournal,
    /// The journal can't be read.
    UnreadableJournal(String),
    /// An earlier attempt wrote a store that failed verification, and kept it
    /// (`FAILED` says why).
    Failed(String),
    /// The store has changed since the attempt began (its heads are not the
    /// journal's), so what was written is of an older store.
    Diverged,
    /// The attempt is of a migration to another format than this comline
    /// writes, or the paths on disk are in a state no crash leaves.
    Inconsistent(String),
}

/// Why the engine stopped, other than a store it refuses on its stamp
/// (that is [`StoreError`](crate::package::build::cas::object_store::StoreError)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    /// A file or object the history needs is missing or damaged. Names the
    /// file ([core#182](https://github.com/ComlineProject/core/issues/182)).
    Damaged { file: PathBuf, reason: String },
    /// An earlier attempt is still on disk.
    Leftover { dir: PathBuf, why: Leftover },
    /// `--assume-format` can't be used here.
    Assume { reason: String },
    /// The registry has no format of that number to decode or encode with.
    NoFormat { number: u32 },
    /// The registry has no step out of that format.
    NoStep { from: u32 },
    /// A version doesn't decode under the format the store is (or is assumed
    /// to be) in.
    Undecodable {
        version: String,
        format: u32,
        reason: String,
    },
    /// A step failed on a version: it is total for every value its format can
    /// express, so this is a bug in the step or a value outside its format.
    StepFailed {
        step: String,
        version: String,
        reason: String,
    },
    /// `down(up(v)) != v`: the step lost something the version recorded.
    Witness { step: String, version: String },
    /// A step wrote an unknown it did not declare.
    UndeclaredUnknown {
        step: String,
        version: String,
        path: String,
    },
    /// The new store failed one of the checks that run before a swap.
    Verification { check: &'static str, reason: String },
    /// There is no store to reset.
    NothingToReset,
    /// A store that can't be reset: a newer one, or one a merge left in two
    /// formats, is never touched.
    ResetRefused { reason: String },
    /// `prune` removes a backup only when the store that replaced it passes
    /// `verify`.
    PruneRefused { reason: String },
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged { file, reason } => write!(
                f,
                "`{}` {reason}. The version history can't be read as it stands, and nothing was \
                 changed. Restore the file (from Git, if `.comline/` is committed, or from a \
                 backup), or start a new history with `comline store reset`, which keeps this \
                 one aside.",
                file.display()
            ),
            Self::Leftover { dir, why } => {
                let dir = dir.display();
                match why {
                    Leftover::NotAJournal => write!(
                        f,
                        "`{dir}` exists and is not a migration this comline made (it has no \
                         journal). Nothing was changed. Look at it, and remove it yourself if \
                         it is not needed."
                    ),
                    Leftover::UnreadableJournal(reason) => write!(
                        f,
                        "the journal in `{dir}` can't be read ({reason}). Nothing was changed. \
                         `{MIGRATE} {RESTART_FLAG}` removes the partial directory and migrates \
                         again; the store itself is untouched."
                    ),
                    Leftover::Failed(reason) => write!(
                        f,
                        "an earlier migration wrote `{dir}` and it failed verification: \
                         {reason}. The store was not touched and the directory is kept for you \
                         to look at. `{MIGRATE} {RESTART_FLAG}` removes it and migrates again."
                    ),
                    Leftover::Diverged => write!(
                        f,
                        "`{dir}` was written for an older state of the store: its history has \
                         changed since (a build, a merge). Nothing was changed. `{MIGRATE} \
                         {RESTART_FLAG}` removes the partial directory and migrates again."
                    ),
                    Leftover::Inconsistent(reason) => write!(
                        f,
                        "`{dir}` and the store are in a state a crash does not leave: {reason}. \
                         Nothing was changed. Look at them, and remove the partial directory \
                         yourself if it is not needed."
                    ),
                }
            }
            Self::Assume { reason } => write!(f, "`--assume-format` can't be used: {reason}."),
            Self::NoFormat { number } => write!(
                f,
                "this comline has no reader for IR format {number}, so a store in it can't be \
                 migrated. Nothing was changed."
            ),
            Self::NoStep { from } => write!(
                f,
                "this comline has no migration step out of IR format {from}. Nothing was changed."
            ),
            Self::Undecodable {
                version,
                format,
                reason,
            } => write!(
                f,
                "version {version} does not decode as IR format {format}: {reason}. Nothing was \
                 changed. If the stamp is wrong or was assumed, the store is not in that format."
            ),
            Self::StepFailed {
                step,
                version,
                reason,
            } => write!(
                f,
                "the migration step {step} failed on version {version}: {reason}. Nothing was \
                 changed."
            ),
            Self::Witness { step, version } => write!(
                f,
                "the migration step {step} would lose something version {version} recorded \
                 (undoing it does not give the version back). Nothing was changed."
            ),
            Self::UndeclaredUnknown {
                step,
                version,
                path,
            } => write!(
                f,
                "the migration step {step} wrote an unknown at {path} in version {version} that \
                 it does not declare. Nothing was changed."
            ),
            Self::Verification { check, reason } => write!(
                f,
                "the new store failed verification ({check}): {reason}. The store was not \
                 touched."
            ),
            Self::NothingToReset => write!(
                f,
                "there is no version history in `.comline/` to reset. Nothing was changed."
            ),
            Self::ResetRefused { reason } => {
                write!(
                    f,
                    "`comline store reset` refuses: {reason}. Nothing was changed."
                )
            }
            Self::PruneRefused { reason } => {
                write!(
                    f,
                    "`comline store prune` refuses: {reason}. Nothing was removed."
                )
            }
        }
    }
}

impl std::error::Error for MigrateError {}
