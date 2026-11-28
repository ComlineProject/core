// `comline store migrate`: the procedure of running.md and its recovery.
//
//   plan (in memory; nothing exists on disk yet)
//   1. claim:  mkdir `.comline.migrating/`, write its journal (`writing`)
//   3. write the new store into it: objects, refs, the receipt, and
//      `FORMAT_VERSION` last
//   4. verify it as a store, and against the old one
//   5. swap:   journal `ready`; rename `.comline/` to its backup; rename
//      `.comline.migrating/` to `.comline/`; move `deps-cache/` across;
//      remove the journal
//
// The state is in the paths and the journal alone, so every step is
// resumable, and a resume does what a first run would from where the disk
// is: nothing resumes on its own (the user runs `comline store migrate`
// again), and a store the attempt wasn't made for is never resumed over.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eyre::Result;

use super::super::object_store::{
    ensure_compatible_store_with, read_stamp, Interrupted, Stamp, StoreError, StoreState,
};
use super::super::reconcile::Heads;
use super::super::storage::{compress, Hash};
use super::super::ObjectStore;
use super::container::{read_refs, Container};
use super::error::{Leftover, MigrateError};
use super::housekeeping::backup_name;
use super::journal::{Journal, JournalState};
use super::plan::Plan;
use super::verify::{check_dir, check_map, VerifyReport};
use super::{
    disk, journal_path, migrating_dir, resetting_dir, Check, Consent, Engine, MigrateOptions,
    Point, FAILED_FILE, RECEIPT_FILE,
};

/// What a finished migration did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub from: u32,
    pub to: u32,
    pub versions: usize,
    /// Where the old store is now: kept until `comline store prune`.
    pub backup: PathBuf,
    /// The old and new hash of every commit, oldest first (`--map`).
    pub map: Vec<(Hash, Hash)>,
    /// The migration was interrupted earlier and this run finished it.
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migrated {
    /// The store is in the current format, or there is none.
    NothingToDo,
    Done(Report),
}

/// A store opened to be read, whatever format it is in.
pub struct Reading {
    pub store: ObjectStore,
    /// The refs to read from (the migrated ones, for a store migrated in
    /// memory).
    pub refs: BTreeMap<String, Heads>,
    /// The format it was migrated from in memory, if it was. Nothing on disk
    /// changed either way.
    pub migrated_from: Option<u32>,
}

fn store_dir(root: &Path) -> PathBuf {
    root.join(".comline")
}

fn heads_hex(refs: &BTreeMap<String, Heads>) -> BTreeMap<String, Vec<String>> {
    refs.iter()
        .map(|(name, heads)| {
            let hex = match heads {
                Heads::One(head) => vec![head.to_hex()],
                Heads::Two(ours, theirs) => vec![ours.to_hex(), theirs.to_hex()],
            };
            (name.clone(), hex)
        })
        .collect()
}

fn render(heads: &Heads) -> String {
    match heads {
        Heads::One(head) => head.to_hex(),
        Heads::Two(ours, theirs) => {
            format!("<<<<<<< ours\n{ours}\n=======\n{theirs}\n>>>>>>> theirs\n")
        }
    }
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// A migration or a reset that is still on disk.
pub(super) fn interrupted(root: &Path) -> Option<Interrupted> {
    if resetting_dir(root).exists() {
        Some(Interrupted::Reset)
    } else if migrating_dir(root).exists() || journal_path(&store_dir(root)).exists() {
        Some(Interrupted::Migration)
    } else {
        None
    }
}

impl Engine {
    /// What there is to migrate from: the stamp, or the format the user
    /// assumed for an unstamped store. `None`: nothing to do.
    fn source(&self, root: &Path, options: &MigrateOptions) -> Result<Option<(u32, bool)>> {
        let target = self.registry.target();
        let from_format = |found: u32, assumed: bool| -> Result<Option<(u32, bool)>> {
            if found > target {
                Err(StoreError::Newer { found }.into())
            } else if found == target {
                // A stamped store at the target has nothing to do; an
                // unstamped one the user says is at the target gets its
                // stamp.
                Ok(assumed.then_some((found, true)))
            } else if self.registry.can_migrate_from(found) {
                Ok(Some((found, assumed)))
            } else {
                Err(StoreError::Older { found }.into())
            }
        };
        match (read_stamp(root), options.assume_format) {
            (Stamp::NoHistory, None) => Ok(None),
            (Stamp::NoHistory, Some(_)) => Err(MigrateError::Assume {
                reason: "there is no version history in `.comline/` to migrate".into(),
            }
            .into()),
            (Stamp::Format(found), None) => from_format(found, false),
            (Stamp::Format(found), Some(_)) => Err(MigrateError::Assume {
                reason: format!(
                    "the store is stamped with format {found}; the option is for a store that \
                     has lost its stamp"
                ),
            }
            .into()),
            (Stamp::Unstamped, Some(assumed)) => from_format(assumed, true),
            (Stamp::Unstamped, None) => Err(StoreError::Unstamped.into()),
            (Stamp::Unreadable(reason), _) => Err(StoreError::UnreadableStamp { reason }.into()),
            (Stamp::Invalid(text), _) => Err(StoreError::InvalidStamp { text }.into()),
            (Stamp::Merged(ours, theirs), _) => {
                Err(StoreError::MergedFormats { ours, theirs }.into())
            }
        }
    }

    fn plan_unguarded(&self, root: &Path, options: &MigrateOptions) -> Result<Option<Plan>> {
        match self.source(root, options)? {
            None => Ok(None),
            Some((from, assumed)) => Ok(Some(Plan::build(
                &self.registry,
                &store_dir(root),
                from,
                assumed,
            )?)),
        }
    }

    /// Plan the migration of the project's store, in memory: read every
    /// object, run the chain, run the checks that need no disk. **Writes
    /// nothing**, and needs no consent. `None`: nothing to do. An interrupted
    /// migration or reset is an error that names the command that finishes
    /// it.
    pub fn plan(&self, root: &Path, options: &MigrateOptions) -> Result<Option<Plan>> {
        if let Some(what) = interrupted(root) {
            return Err(StoreError::Interrupted { what }.into());
        }
        self.plan_unguarded(root, options)
    }

    /// `comline store migrate --check`: [`plan`](Self::plan), as one of the
    /// three outcomes the exit code tells apart.
    pub fn check(&self, root: &Path, options: &MigrateOptions) -> Check {
        match self.plan(root, options) {
            Ok(None) => Check::NothingToDo,
            Ok(Some(plan)) => Check::WouldMigrate(Box::new(plan)),
            Err(error) => Check::Refused(error),
        }
    }

    /// `comline store migrate`: migrate the project's store, or finish a
    /// migration that was interrupted. Only the user's command calls this
    /// (`consent` says how they asked); no other command does, and nothing
    /// resumes on its own. Idempotent: on a migrated store it does nothing.
    pub fn migrate(
        &mut self,
        root: &Path,
        options: &MigrateOptions,
        _consent: Consent,
    ) -> Result<Migrated> {
        if resetting_dir(root).exists() {
            return Err(StoreError::Interrupted {
                what: Interrupted::Reset,
            }
            .into());
        }
        let store = store_dir(root);
        let migrating = migrating_dir(root);

        if migrating.exists() {
            return self.resume(root, options);
        }
        if journal_path(&store).exists() {
            // The new store is in place; the last moves are not done.
            let journal = Journal::read(&store).map_err(|e| MigrateError::Leftover {
                dir: store.clone(),
                why: Leftover::UnreadableJournal(format!("{e:#}")),
            })?;
            return self.complete(root, &journal, true);
        }
        self.fresh(root, options)
    }

    fn fresh(&mut self, root: &Path, options: &MigrateOptions) -> Result<Migrated> {
        let Some(plan) = self.plan_unguarded(root, options)? else {
            return Ok(Migrated::NothingToDo);
        };
        let journal = Journal {
            state: JournalState::Writing,
            from: plan.from,
            assumed: plan.assumed,
            to: plan.to,
            old_heads: heads_hex(&plan.old_refs),
            backup: backup_name(root, &plan.from.to_string()),
        };
        // The claim: the directory's existence is the lock.
        let migrating = migrating_dir(root);
        disk::create_dir(&migrating)?;
        journal.write(&migrating)?;
        self.reach(Point::Claimed)?;
        self.write_and_swap(root, plan, journal, false)
    }

    /// `.comline.migrating/` exists: say whether it can be gone on from, and
    /// go on from where it stopped.
    fn resume(&mut self, root: &Path, options: &MigrateOptions) -> Result<Migrated> {
        let store = store_dir(root);
        let migrating = migrating_dir(root);
        let leftover = |why: Leftover| -> eyre::Report {
            MigrateError::Leftover {
                dir: migrating.clone(),
                why,
            }
            .into()
        };

        if journal_path(&store).exists() {
            return Err(leftover(Leftover::Inconsistent(
                "the new store is already in place and its journal is still there".into(),
            )));
        }

        let failed = migrating.join(FAILED_FILE);
        if failed.exists() {
            if options.restart {
                disk::remove_tree(&migrating)?;
                return self.fresh(root, options);
            }
            let reason = fs::read_to_string(&failed).unwrap_or_default();
            return Err(leftover(Leftover::Failed(reason.trim().to_string())));
        }

        // A write that didn't finish is not a file of the store.
        disk::sweep_unfinished_writes(&migrating)?;

        let journal = match Journal::read(&migrating) {
            Ok(journal) => journal,
            Err(error) => {
                let has_journal = journal_path(&migrating).exists();
                // Claimed and killed before the journal was written: there is
                // nothing in it to lose.
                if dir_is_empty(&migrating) || (has_journal && options.restart) {
                    disk::remove_tree(&migrating)?;
                    return self.fresh(root, options);
                }
                return Err(leftover(if has_journal {
                    Leftover::UnreadableJournal(format!("{error:#}"))
                } else {
                    Leftover::NotAJournal
                }));
            }
        };
        if journal.to != self.registry.target() {
            return Err(leftover(Leftover::Inconsistent(format!(
                "it migrates to format {}, and this comline writes format {}",
                journal.to,
                self.registry.target()
            ))));
        }

        match journal.state {
            JournalState::Writing => {
                if options.restart {
                    disk::remove_tree(&migrating)?;
                    return self.fresh(root, options);
                }
                // Writes are content-addressed, so repeating one writes the
                // same bytes: resume if the store is the one this was for.
                if !self.store_is_what_the_journal_says(root, &journal) {
                    return Err(leftover(Leftover::Diverged));
                }
                let plan = Plan::build(&self.registry, &store, journal.from, journal.assumed)?;
                self.write_and_swap(root, plan, journal, true)
            }
            JournalState::Ready => {
                // Verified before; checked again before anything is renamed.
                check_dir(&self.registry, &migrating, journal.to, true)?;
                self.swap(root, &journal)?;
                self.complete(root, &journal, true)
            }
        }
    }

    /// The store in place has the heads the journal recorded.
    fn store_is_what_the_journal_says(&self, root: &Path, journal: &Journal) -> bool {
        match read_refs(&store_dir(root)) {
            Ok(refs) => heads_hex(&refs) == journal.old_heads,
            Err(_) => false,
        }
    }

    /// Steps 3 to 5, from a plan and a `writing` journal.
    fn write_and_swap(
        &mut self,
        root: &Path,
        plan: Plan,
        mut journal: Journal,
        resumed: bool,
    ) -> Result<Migrated> {
        let migrating = migrating_dir(root);
        self.write_new_store(root, &plan)?;

        if let Err(error) = self.verify_new_store(root, &plan) {
            // What failed is the new store's content: keep it and say so. A
            // failure to write (a full disk, a refused permission) is not
            // that, and leaves the journal to resume from.
            if matches!(
                error.downcast_ref::<MigrateError>(),
                Some(
                    MigrateError::Verification { .. }
                        | MigrateError::Damaged { .. }
                        | MigrateError::Undecodable { .. }
                )
            ) {
                let _ = disk::write_file(
                    &migrating.join(FAILED_FILE),
                    format!("{error:#}").as_bytes(),
                );
            }
            return Err(error);
        }
        self.reach(Point::Verified)?;

        journal.state = JournalState::Ready;
        journal.write(&migrating)?;
        self.reach(Point::Ready)?;

        self.swap(root, &journal)?;
        self.complete(root, &journal, resumed)
    }

    /// Step 3: the new store, into `.comline.migrating/`.
    fn write_new_store(&mut self, root: &Path, plan: &Plan) -> Result<()> {
        let store = store_dir(root);
        let migrating = migrating_dir(root);
        let old = ObjectStore::at(&store);
        let new = ObjectStore::at(&migrating);

        for (position, (hash, bytes)) in plan.objects.iter().enumerate() {
            let target = new.object_path(hash);
            if !target.exists() {
                let source = old.object_path(hash);
                if source.exists() {
                    disk::link_or_copy(&source, &target)?;
                } else {
                    disk::write_file(&target, &compress(bytes))?;
                }
            }
            self.reach(Point::ObjectWritten(position))?;
        }

        for (name, heads) in &plan.new_refs {
            disk::write_file(&migrating.join(name), render(heads).as_bytes())?;
            self.reach(Point::RefWritten)?;
        }

        // The receipt: what was there, and a line for this migration. It is
        // the same on every machine, so Git never conflicts on it.
        let mut receipt = fs::read_to_string(store.join(RECEIPT_FILE)).unwrap_or_default();
        if !receipt.is_empty() && !receipt.ends_with('\n') {
            receipt.push('\n');
        }
        receipt.push_str(&format!("{} -> {}\n", plan.from, plan.to));
        disk::write_file(&migrating.join(RECEIPT_FILE), receipt.as_bytes())?;
        self.reach(Point::ReceiptWritten)?;

        disk::write_file(
            &migrating.join("FORMAT_VERSION"),
            plan.to.to_string().as_bytes(),
        )?;
        self.reach(Point::StampWritten)
    }

    /// Step 4: the new directory is a store in the target format, and it is
    /// the old one, migrated.
    fn verify_new_store(&self, root: &Path, plan: &Plan) -> Result<()> {
        let migrating = migrating_dir(root);
        disk::sweep_unfinished_writes(&migrating)?;
        let stamp = fs::read_to_string(migrating.join("FORMAT_VERSION")).unwrap_or_default();
        if stamp.trim() != plan.to.to_string() {
            return Err(MigrateError::Verification {
                check: "stamp",
                reason: format!("`FORMAT_VERSION` holds {:?}, not {}", stamp.trim(), plan.to),
            }
            .into());
        }
        let (new, _) = check_dir(&self.registry, &migrating, plan.to, true)?;
        let old = Container::open(&store_dir(root))?;
        check_map(&old, &new)?;
        Ok(())
    }

    /// Step 5's renames, from whatever state the disk is in.
    fn swap(&mut self, root: &Path, journal: &Journal) -> Result<()> {
        let store = store_dir(root);
        let migrating = migrating_dir(root);
        let backup = root.join(&journal.backup);
        let inconsistent = |reason: &str| -> eyre::Report {
            MigrateError::Leftover {
                dir: migrating.clone(),
                why: Leftover::Inconsistent(reason.into()),
            }
            .into()
        };

        match (store.exists(), migrating.exists(), backup.exists()) {
            // Nothing is renamed yet: the store has to still be the old one.
            (true, true, false) => {
                if !self.store_is_what_the_journal_says(root, journal) {
                    return Err(MigrateError::Leftover {
                        dir: migrating.clone(),
                        why: Leftover::Diverged,
                    }
                    .into());
                }
                disk::rename(&store, &backup)?;
                self.reach(Point::OldStoreMoved)?;
                disk::rename(&migrating, &store)?;
                self.reach(Point::NewStorePlaced)
            }
            // Between the two renames: there is no `.comline/`.
            (false, true, true) => {
                disk::rename(&migrating, &store)?;
                self.reach(Point::NewStorePlaced)
            }
            _ => Err(inconsistent(&format!(
                "`.comline/`, `.comline.migrating/` and `{}` are not in a state a crash leaves",
                journal.backup
            ))),
        }
    }

    /// What is left after both renames: `deps-cache/` comes across from the
    /// backup (an offline machine depends on it), and the journal goes.
    fn complete(&mut self, root: &Path, journal: &Journal, resumed: bool) -> Result<Migrated> {
        let store = store_dir(root);
        let backup = root.join(&journal.backup);
        if !backup.exists() {
            return Err(MigrateError::Leftover {
                dir: store,
                why: Leftover::Inconsistent(format!(
                    "the journal names the backup `{}`, which is not there",
                    journal.backup
                )),
            }
            .into());
        }

        let (cache_from, cache_to) = (backup.join("deps-cache"), store.join("deps-cache"));
        if cache_from.exists() && !cache_to.exists() {
            disk::rename(&cache_from, &cache_to)?;
        }
        self.reach(Point::CacheMoved)?;

        disk::remove_file(&journal_path(&store))?;
        self.reach(Point::JournalRemoved)?;

        let old = Container::open(&backup)?;
        let new = Container::open(&store)?;
        let map = check_map(&old, &new)?;
        Ok(Migrated::Done(Report {
            from: journal.from,
            to: journal.to,
            versions: new.commits.len(),
            backup,
            map,
            resumed,
        }))
    }

    /// `comline store verify`: the checks on the store as it is. A store that
    /// can't be used as it is (an older, newer or unstamped one) is the error
    /// that says so; a damaged one names the file. What the current encoder
    /// would write differently is reported, not failed
    /// ([`VerifyReport::not_canonical`]).
    pub fn verify(&self, root: &Path) -> Result<VerifyReport> {
        let target = self.registry.target();
        match ensure_compatible_store_with(root, &self.registry)? {
            StoreState::Fresh => Ok(VerifyReport {
                format: target,
                versions: 0,
                objects: 0,
                not_canonical: vec![],
            }),
            StoreState::Current => {
                Ok(check_dir(&self.registry, &store_dir(root), target, false)?.1)
            }
        }
    }

    /// Open the project's store to read it. A store in the current format is
    /// opened as it is. An older one that a chain of steps reaches the
    /// current format from is migrated **in memory**: the objects the
    /// migration makes are held by a scratch store, nothing is written, and
    /// the refs to read from are the migrated ones. Anything else is the
    /// error that says why (and that names `comline store migrate` or
    /// `reset`), as for a command that writes.
    pub fn open_for_reading(&self, root: &Path) -> Result<Reading> {
        match ensure_compatible_store_with(root, &self.registry) {
            Ok(StoreState::Fresh) => Ok(Reading {
                store: ObjectStore::new(root),
                refs: BTreeMap::new(),
                migrated_from: None,
            }),
            Ok(StoreState::Current) => Ok(Reading {
                store: ObjectStore::new(root),
                refs: read_refs(&store_dir(root))?,
                migrated_from: None,
            }),
            Err(StoreError::NeedsMigration { found }) => {
                let plan = Plan::build(&self.registry, &store_dir(root), found, false)?;
                let store = ObjectStore::scratch(root);
                for (_, bytes) in plan.objects.iter() {
                    store.write(bytes)?;
                }
                Ok(Reading {
                    store,
                    refs: plan.new_refs,
                    migrated_from: Some(found),
                })
            }
            Err(error) => Err(error.into()),
        }
    }
}
