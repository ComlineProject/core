// `comline store status`, the backups a migration or a reset leaves, `reset`
// and `prune`. Every one that changes anything takes the user's `Consent`;
// `status` and the backup listing only read.

use std::fs;
use std::path::{Path, PathBuf};

use eyre::Result;

use super::super::object_store::{
    ensure_compatible_store_with, read_stamp, Interrupted, Stamp, StoreError, StoreState,
};
use super::super::storage::Hash;
use super::container::Container;
use super::error::MigrateError;
use super::run::interrupted;
use super::{disk, journal_path, migrating_dir, resetting_dir, Consent, Engine, Point};

/// A store set aside by a migration or a reset: `.comline.<what>.bak/`, kept
/// until the user runs `comline store prune`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub dir: PathBuf,
    /// The format the store in it was in, if its name says (`format-<N>`).
    pub format: Option<u32>,
}

/// The name for a backup of a store labelled `label` (`3`, or `unknown`):
/// `.comline.format-<label>.bak`, or with a number after it where a backup of
/// that format is already there. Never a name that is taken.
pub(super) fn backup_name(root: &Path, label: &str) -> String {
    let first = format!(".comline.format-{label}.bak");
    if !root.join(&first).exists() {
        return first;
    }
    (2..)
        .map(|n| format!(".comline.format-{label}-{n}.bak"))
        .find(|name| !root.join(name).exists())
        .expect("an unbounded range has a free name")
}

/// The backups beside `.comline/`, oldest name first.
pub fn backups(root: &Path) -> Result<Vec<Backup>> {
    let mut found = vec![];
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(".comline.") && name.ends_with(".bak")) || !entry.path().is_dir() {
            continue;
        }
        let format = name
            .strip_prefix(".comline.format-")
            .map(|rest| {
                rest.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .and_then(|digits| digits.parse().ok());
        found.push(Backup {
            dir: entry.path(),
            format,
        });
    }
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(found)
}

/// One version, as `comline store status` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub hash: Hash,
    pub version: String,
    /// Whether it is a release rather than a build between releases.
    pub release: bool,
    /// How many unknowns it records; `None` where the store's format can't
    /// be decoded here to count them.
    pub unknowns: Option<usize>,
}

/// What `comline store status` shows.
#[derive(Debug)]
pub struct StoreStatus {
    /// The format this comline writes.
    pub target: u32,
    /// The format the stamp says, where it says one.
    pub stamp: Option<u32>,
    /// Whether a command can use the store as it is, or why not: the error
    /// that names `comline store migrate` or `reset`.
    pub classification: Result<StoreState, StoreError>,
    /// The versions, oldest first, or why the history can't be listed (a
    /// damaged store names the file).
    pub versions: Result<Vec<VersionInfo>, String>,
    pub backups: Vec<Backup>,
}

impl StoreStatus {
    /// A migration is needed, or has to be finished.
    pub fn needs_migration(&self) -> bool {
        matches!(
            self.classification,
            Err(StoreError::NeedsMigration { .. })
                | Err(StoreError::Interrupted {
                    what: Interrupted::Migration
                })
        )
    }
}

/// What `comline store reset` would do, to show the user before they confirm:
/// the exact action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetPlan {
    pub store: PathBuf,
    pub backup: PathBuf,
    /// The format the store is in, if its stamp says.
    pub format: Option<u32>,
    /// There is a `deps-cache/` in it, which stays in the new `.comline/`.
    pub keeps_deps_cache: bool,
}

impl ResetPlan {
    /// The question for the user to confirm: it names the store, the backup
    /// and the format, and says what is deleted (nothing).
    pub fn describe(&self) -> String {
        let format = self.format.map_or_else(
            || "of an unknown IR format".to_string(),
            |n| format!("in IR format {n}"),
        );
        format!(
            "Move `{}` ({format}) aside to `{}`, and start a new version history at the next \
             build. Nothing is deleted: the backup keeps every version until `comline store \
             prune`{}.",
            self.store.display(),
            self.backup.display(),
            if self.keeps_deps_cache {
                ", and `deps-cache/` stays where it is"
            } else {
                ""
            }
        )
    }
}

/// What a reset did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetReport {
    /// Where the old store is now; `None` if this run only finished a reset
    /// whose first moves were made earlier.
    pub backup: Option<PathBuf>,
    pub resumed: bool,
}

/// What a prune removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pruned {
    pub removed: Vec<Backup>,
}

impl Engine {
    /// `comline store status`: read-only, and never an error for a store it
    /// can't use: that is what it reports.
    pub fn status(&self, root: &Path) -> Result<StoreStatus> {
        let store = root.join(".comline");
        let stamp = match read_stamp(root) {
            Stamp::Format(n) => Some(n),
            _ => None,
        };
        let versions = if store.join("refs").exists() || store.join("objects").exists() {
            match Container::open(&store) {
                Err(error) => Err(format!("{error:#}")),
                Ok(container) => {
                    let format = stamp.and_then(|n| self.registry.format(n).ok());
                    Ok(container
                        .commits
                        .iter()
                        .map(|node| VersionInfo {
                            hash: node.hash,
                            version: node.commit.version.clone(),
                            release: node.commit.is_release(),
                            unknowns: format.and_then(|format| {
                                let payload =
                                    format.decode(&container.store, &node.commit.tree).ok()?;
                                format.unknowns(&payload).ok().map(|found| found.len())
                            }),
                        })
                        .collect())
                }
            }
        } else {
            Ok(vec![])
        };
        Ok(StoreStatus {
            target: self.registry.target(),
            stamp,
            classification: ensure_compatible_store_with(root, &self.registry),
            versions,
            backups: backups(root)?,
        })
    }

    /// What a reset would do, or why it refuses. Read-only. A store that has
    /// no path to the current format is what a reset is for, but the user may
    /// also reset one that has (to start a new history instead of migrating)
    /// or a current one: the old history is kept either way. A **newer**
    /// store, and one a Git merge left in two formats, are never touched.
    pub fn reset_plan(&self, root: &Path) -> Result<ResetPlan> {
        if migrating_dir(root).exists() || journal_path(&root.join(".comline")).exists() {
            return Err(StoreError::Interrupted {
                what: Interrupted::Migration,
            }
            .into());
        }
        let store = root.join(".comline");
        let (format, label) = match read_stamp(root) {
            Stamp::NoHistory => return Err(MigrateError::NothingToReset.into()),
            Stamp::Format(n) if n > self.registry.target() => {
                return Err(MigrateError::ResetRefused {
                    reason: format!(
                        "the store is in IR format {n}, which a newer comline wrote; it is \
                         never reset or touched by this one"
                    ),
                }
                .into())
            }
            Stamp::Merged(ours, theirs) => {
                return Err(MigrateError::ResetRefused {
                    reason: format!(
                        "a Git merge left `FORMAT_VERSION` between formats {ours} and \
                         {theirs}: migrate both sides first"
                    ),
                }
                .into())
            }
            Stamp::Format(n) => (Some(n), n.to_string()),
            Stamp::Unstamped | Stamp::Unreadable(_) | Stamp::Invalid(_) => {
                (None, "unknown".to_string())
            }
        };
        Ok(ResetPlan {
            backup: root.join(backup_name(root, &label)),
            keeps_deps_cache: store.join("deps-cache").exists(),
            store,
            format,
        })
    }

    /// `comline store reset`: move `.comline/` to a backup and leave a fresh
    /// one, `deps-cache/` kept. Deletes nothing. The user's command (on a
    /// terminal, after they confirmed [`ResetPlan::describe`]; without one,
    /// with `--force`): `consent` says which. Resumable: an interrupted reset
    /// is finished by running it again.
    pub fn reset(&mut self, root: &Path, _consent: Consent) -> Result<ResetReport> {
        let store = root.join(".comline");
        let staging = resetting_dir(root);

        if staging.exists() && !store.exists() {
            // The old store is already aside: only the last rename is left.
            disk::rename(&staging, &store)?;
            self.reach(Point::ResetPlaced)?;
            return Ok(ResetReport {
                backup: None,
                resumed: true,
            });
        }

        let resumed = staging.exists();
        let plan = self.reset_plan(root)?;
        if !resumed {
            disk::create_dir(&staging)?;
        }
        self.reach(Point::ResetStaged)?;

        // The cache goes first, so that it is never in the store that is
        // moved aside: an offline machine can't fetch it again.
        let cache = store.join("deps-cache");
        if cache.exists() {
            disk::rename(&cache, &staging.join("deps-cache"))?;
        }
        self.reach(Point::ResetCacheStaged)?;

        disk::rename(&store, &plan.backup)?;
        self.reach(Point::ResetStoreMoved)?;
        disk::rename(&staging, &store)?;
        self.reach(Point::ResetPlaced)?;
        Ok(ResetReport {
            backup: Some(plan.backup),
            resumed,
        })
    }

    /// `comline store prune`: remove the backups, when the user runs it, and
    /// only if the store that replaced them passes `comline store verify`.
    /// With no store yet (after a reset, before the next build) nothing has
    /// replaced them, and it refuses.
    pub fn prune(&self, root: &Path, _consent: Consent) -> Result<Pruned> {
        let found = backups(root)?;
        if found.is_empty() {
            return Ok(Pruned { removed: vec![] });
        }
        let refuse =
            |reason: String| -> eyre::Report { MigrateError::PruneRefused { reason }.into() };
        if let Some(what) = interrupted(root) {
            return Err(refuse(format!("{}", StoreError::Interrupted { what })));
        }
        match ensure_compatible_store_with(root, &self.registry) {
            Ok(StoreState::Current) => {}
            Ok(StoreState::Fresh) => {
                return Err(refuse(
                    "no store has replaced the backups yet (there is no version history in \
                     `.comline/`); build first"
                        .into(),
                ))
            }
            Err(error) => {
                return Err(refuse(format!(
                    "`.comline/` can't be used as it is: {error}"
                )))
            }
        }
        self.verify(root).map_err(|error| {
            refuse(format!(
                "the store that replaced them does not pass verification: {error:#}"
            ))
        })?;
        for backup in &found {
            disk::remove_tree(&backup.dir)?;
        }
        Ok(Pruned { removed: found })
    }
}
