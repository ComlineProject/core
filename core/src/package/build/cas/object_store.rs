// Object store for content-addressable storage

use super::storage::{Hash, compress, decompress};
use eyre::{eyre, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// This binary's expected shape for everything persisted under `.comline/`
/// (`FrozenUnit`, `Blob`/`Tree`/`Commit`, ...) - **not** the package-authored
/// `specification_version` (that's the user's own chosen language edition,
/// unrelated to core's internal IR representation).
///
/// CBOR tags struct fields and enum variants by name, so growing (with a
/// ledger row, below) or reordering `FrozenUnit` needs no bump here - only *renaming* or
/// *removing* an existing field/variant does, since that's the one case a
/// self-describing format can't absorb for free. Bump this by hand when you
/// do one of those; [`ensure_compatible_store`] then refuses any `.comline/`
/// stamped with a different value instead of trying (and likely failing,
/// or in the worst case silently misreading) to decode it. It refuses and
/// changes nothing: no store is deleted, moved or rewritten to change its
/// format (core#268, `docs/proposals/ir-migrations`).
///
/// What a format is, and what may change in it without a bump, is written
/// down and checked: `formats/src/v3/` holds its `SCHEMA.md`, the frozen
/// copy of its types and a ledger of every addition made without a bump.
/// A test holds the live types to the copy, golden hashes hold what a fresh
/// build writes, and `cargo xtask check-format-bump` says what a bump has
/// to come with.
///
/// History:
/// - 1: CBOR, replacing bincode's position-based encoding.
/// - 2: `FrozenUnit::Property`'s `expression: Option<String>` became
///   `value: ConfigValue` - a field renamed *and* retyped, which is
///   exactly the case name tagging cannot absorb.
/// - 3: types became structured (`TypeRef`, `Literal`) instead of
///   `KindValue` text: `Field`/`Constant` `kind_value` became `ty` plus a
///   literal, `EnumVariant` holds its name, and `Assert` gained `expr`.
///
/// Additions that needed no bump (each is a row of the ledger): the root
/// tree's `docs` blob (the `//!` headers, `crate::doc::FrozenDocs`, core#145)
/// is a new entry that older readers skip and newer ones treat as absent on
/// older versions. So are a
/// vendored dependency's `config` and `dependency_configs` blobs, in its
/// `dep_<name>` tree (core#251): a tree without them reads as having not
/// kept the dependency's config
/// ([`StoredConfigs`](crate::package::config::ir::frozen::cas::blob::StoredConfigs)),
/// and bumping this would have refused every older store to say so.
pub const CURRENT_IR_FORMAT_VERSION: u32 = 3;

fn format_version_path(project_root: &Path) -> PathBuf {
    project_root.join(".comline/FORMAT_VERSION")
}

/// What [`ensure_compatible_store`] found, when the store can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreState {
    /// No `.comline/`, or one with no history in it yet (a dependency's
    /// `deps-cache/` is fetched into `.comline/` before anything is built):
    /// the next build is the first and stamps it.
    Fresh,
    /// Stamped with [`CURRENT_IR_FORMAT_VERSION`].
    Current,
}

/// What was interrupted, when [`StoreError::Interrupted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupted {
    /// A `comline store migrate`: `.comline.migrating/` exists, or the new
    /// store is in place and its journal is still there.
    Migration,
    /// A `comline store reset`: `.comline.resetting/` exists.
    Reset,
}

/// Why a `.comline/` can't be used as it is. **Nothing was changed** in any
/// of these cases, and nothing in core changes it afterwards on its own: the
/// store is not deleted, moved, migrated or rewritten to change its format.
/// Only the user's `comline store migrate` and `comline store reset` do
/// that (core#268, `docs/proposals/ir-migrations`). Core is a library, so the
/// caller decides how to show this: a CLI prints it (and on a terminal may
/// ask whether to run [`StoreError::offer`]), the language server makes it a
/// diagnostic.
///
/// It is a `std::error::Error`, so it travels in an `eyre::Report` and is
/// recovered with `downcast_ref::<StoreError>()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Stamped with an older format than this binary writes, and no chain
    /// of migration steps reaches the current one. The user's way on is
    /// `comline store reset`.
    Older { found: u32 },
    /// Stamped with an older format than this binary writes, and a chain of
    /// migration steps reaches the current one: `comline store migrate`.
    NeedsMigration { found: u32 },
    /// Stamped with a newer format than this binary reads: a newer comline
    /// wrote it.
    Newer { found: u32 },
    /// A `.comline/` with history in it and no `FORMAT_VERSION`.
    Unstamped,
    /// `FORMAT_VERSION` exists and can't be read.
    UnreadableStamp { reason: String },
    /// `FORMAT_VERSION` doesn't hold a format number.
    InvalidStamp { text: String },
    /// `FORMAT_VERSION` holds Git conflict markers around two different
    /// formats: a merge joined stores in two formats.
    MergedFormats { ours: u32, theirs: u32 },
    /// A migration or a reset was interrupted and has to be finished first.
    Interrupted { what: Interrupted },
}

/// What a command on a terminal may offer to run, in place of stopping: the
/// exact command and the question to put to the user. The command is never
/// run without a yes, and never with no terminal to ask on (CI fails with the
/// error's message). See [`StoreError::offer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The command, as the user would type it.
    pub command: &'static str,
    /// The question to ask, which names that command and what it does.
    pub question: String,
}

pub(super) const MIGRATE_COMMAND: &str = "comline store migrate";
pub(super) const RESET_COMMAND: &str = "comline store reset";

const RESET_HINT: &str = "To start a new history, run `comline store reset`: it moves `.comline/` \
     aside (to `.comline.format-<its format>.bak/`), keeps `deps-cache/` and deletes nothing.";

impl StoreError {
    /// The command that fixes this, if there is one. Named in the message.
    pub fn command(&self) -> Option<&'static str> {
        match self {
            Self::Older { .. }
            | Self::Unstamped
            | Self::UnreadableStamp { .. }
            | Self::InvalidStamp { .. }
            | Self::Interrupted { what: Interrupted::Reset } => Some(RESET_COMMAND),
            Self::NeedsMigration { .. } | Self::Interrupted { what: Interrupted::Migration } => {
                Some(MIGRATE_COMMAND)
            }
            Self::Newer { .. } | Self::MergedFormats { .. } => None,
        }
    }

    /// What a command run on a terminal may ask the user whether to run
    /// instead of stopping: the migration, for a store that has a chain to the
    /// current format or a migration to finish. **Never a reset**: a reset is
    /// the user's own command and is never offered inline. The command does
    /// nothing unless the user says yes; with no terminal, the command fails
    /// with this error's message.
    pub fn offer(&self) -> Option<Offer> {
        let question = match self {
            Self::NeedsMigration { found } => format!(
                "`.comline/` is in IR format {found} and this comline writes format {}. Run \
                 `{MIGRATE_COMMAND}` now? It writes a new store beside the old one, verifies it, \
                 and keeps the old one as `.comline.format-{found}.bak/` until \
                 `comline store prune`.",
                CURRENT_IR_FORMAT_VERSION
            ),
            Self::Interrupted { what: Interrupted::Migration } => format!(
                "A migration of `.comline/` was interrupted. Run `{MIGRATE_COMMAND}` now to \
                 finish it? Nothing is deleted."
            ),
            _ => return None,
        };
        Some(Offer { command: MIGRATE_COMMAND, question })
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = CURRENT_IR_FORMAT_VERSION;
        match self {
            Self::Older { found } => write!(
                f,
                "the version history in `.comline/` is in IR format {found}, but this comline \
                 reads and writes format {current}, and has no migration from format {found}. \
                 Nothing was changed. To keep the history, use the comline that wrote it. \
                 {RESET_HINT}"
            ),
            Self::NeedsMigration { found } => write!(
                f,
                "the version history in `.comline/` is in IR format {found}, but this comline \
                 writes format {current}. Nothing was changed. Run `{MIGRATE_COMMAND}` to \
                 migrate it (`{MIGRATE_COMMAND} --check` shows what it would do first): the old \
                 store is kept as `.comline.format-{found}.bak/` until `comline store prune`."
            ),
            Self::Newer { found } => write!(
                f,
                "the version history in `.comline/` is in IR format {found}, which a newer \
                 comline wrote; this comline reads and writes format {current}. Nothing was \
                 changed and nothing will be: use a comline that reads format {found}."
            ),
            Self::Unstamped => write!(
                f,
                "`.comline/` holds a version history but no `FORMAT_VERSION` stamp, so its IR \
                 format is unknown (it predates the stamp, or the file was lost). Nothing was \
                 changed. If you know its format, `{MIGRATE_COMMAND} --assume-format <N>` \
                 migrates it from there. {RESET_HINT}"
            ),
            Self::UnreadableStamp { reason } => write!(
                f,
                "`.comline/FORMAT_VERSION` can't be read ({reason}), so the IR format of the \
                 version history is unknown. Nothing was changed. Restore the file, or: \
                 {RESET_HINT}"
            ),
            Self::InvalidStamp { text } => write!(
                f,
                "`.comline/FORMAT_VERSION` holds {text:?}, which is not an IR format number, so \
                 the format of the version history is unknown (the file may be damaged, or \
                 written by a comline this one doesn't know). Nothing was changed. {RESET_HINT}"
            ),
            Self::MergedFormats { ours, theirs } => write!(
                f,
                "`.comline/FORMAT_VERSION` is in conflict between IR format {ours} and format \
                 {theirs}: a Git merge joined stores in two formats. Nothing was changed and \
                 nothing is resolved for you. Migrate both sides first: check out each branch, \
                 run `{MIGRATE_COMMAND}` on it, then merge."
            ),
            Self::Interrupted { what: Interrupted::Migration } => write!(
                f,
                "a migration of `.comline/` was interrupted: `.comline.migrating/` or the \
                 migration's journal is still there. Nothing was changed and nothing resumes on \
                 its own. Run `{MIGRATE_COMMAND}` to finish it."
            ),
            Self::Interrupted { what: Interrupted::Reset } => write!(
                f,
                "a `{RESET_COMMAND}` was interrupted: `.comline.resetting/` is still there. \
                 Nothing was changed and nothing resumes on its own. Run `{RESET_COMMAND}` to \
                 finish it."
            ),
        }
    }
}

impl std::error::Error for StoreError {}

/// Whether `dir` holds anything at all (a missing or unreadable one doesn't).
fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// A stamp's text as a format number: decimal digits and nothing else.
pub(super) fn parse_stamp(text: &str) -> Option<u32> {
    Some(text.trim())
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<u32>().ok())
}

/// The two formats in a stamp that Git left in conflict (`<<<<<<<`, ours,
/// `=======`, theirs, `>>>>>>>`; a `|||||||` base section is skipped).
fn conflicted_stamp(text: &str) -> Option<(u32, u32)> {
    if !text.lines().any(|line| line.starts_with("<<<<<<<")) {
        return None;
    }
    let (mut ours, mut theirs, mut side) = (None, None, 0u8);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("<<<<<<<") {
            side = 1;
        } else if line.starts_with("|||||||") {
            side = 3;
        } else if line.starts_with("=======") {
            side = 2;
        } else if line.starts_with(">>>>>>>") {
            side = 0;
        } else if side == 1 {
            ours = parse_stamp(line);
        } else if side == 2 {
            theirs = parse_stamp(line);
        }
    }
    Some((ours?, theirs?))
}

/// What `.comline/FORMAT_VERSION` says, without judging it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stamp {
    /// No `.comline/`, or one with no history in it.
    NoHistory,
    /// A format number.
    Format(u32),
    /// History and no stamp.
    Unstamped,
    /// The file can't be read.
    Unreadable(String),
    /// The file holds something that is not a format number.
    Invalid(String),
    /// Git left it in conflict between two different formats.
    Merged(u32, u32),
}

/// Read the stamp of the `.comline/` under `project_root`. **Read-only.**
pub(super) fn read_stamp(project_root: &Path) -> Stamp {
    let comline_dir = project_root.join(".comline");
    if !comline_dir.exists() {
        return Stamp::NoHistory;
    }
    let text = match fs::read_to_string(format_version_path(project_root)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let has_history =
                has_entries(&comline_dir.join("refs")) || has_entries(&comline_dir.join("objects"));
            return if has_history { Stamp::Unstamped } else { Stamp::NoHistory };
        }
        Err(e) => return Stamp::Unreadable(e.to_string()),
    };
    if let Some((ours, theirs)) = conflicted_stamp(&text) {
        if ours != theirs {
            return Stamp::Merged(ours, theirs);
        }
    }
    match parse_stamp(&text) {
        Some(found) => Stamp::Format(found),
        None => Stamp::Invalid(text.trim().chars().take(64).collect()),
    }
}

/// Classify the `.comline/` under `project_root` against
/// [`CURRENT_IR_FORMAT_VERSION`], with the steps this comline has
/// ([`Registry::current`](super::migrate::Registry::current)). See
/// [`ensure_compatible_store_with`].
pub fn ensure_compatible_store(project_root: &Path) -> Result<StoreState, StoreError> {
    ensure_compatible_store_with(project_root, &super::migrate::Registry::current())
}

/// Classify the `.comline/` under `project_root` against the target of
/// `registry`. **Read-only**: it never creates, moves, rewrites or deletes
/// anything, in any outcome. Call it before trusting anything already in
/// `.comline/` (before deciding whether a previous commit exists to diff
/// against), and before writing to it.
///
/// - an interrupted migration or reset: [`StoreError::Interrupted`]
/// - no `.comline/`, or an unstamped one with no history: [`StoreState::Fresh`]
/// - the current stamp: [`StoreState::Current`]
/// - an older stamp: [`StoreError::NeedsMigration`] if `registry` has steps
///   from it to the current format, else [`StoreError::Older`]
/// - a newer stamp, a missing stamp on a store with history, a stamp that
///   can't be read or isn't a number, a stamp Git left in conflict: the
///   matching [`StoreError`]
///
/// `deps-cache/` is not part of the store: a `.comline/` holding only it is
/// fresh, and an error here never touches it.
pub fn ensure_compatible_store_with(
    project_root: &Path,
    registry: &super::migrate::Registry,
) -> Result<StoreState, StoreError> {
    use super::migrate::{journal_path, migrating_dir, resetting_dir};

    // Between the two renames of a swap there is no `.comline/`: that is an
    // interrupted migration, not a first build.
    if resetting_dir(project_root).exists() {
        return Err(StoreError::Interrupted { what: Interrupted::Reset });
    }
    if migrating_dir(project_root).exists()
        || journal_path(&project_root.join(".comline")).exists()
    {
        return Err(StoreError::Interrupted { what: Interrupted::Migration });
    }

    let target = registry.target();
    match read_stamp(project_root) {
        Stamp::NoHistory => Ok(StoreState::Fresh),
        Stamp::Format(found) if found == target => Ok(StoreState::Current),
        Stamp::Format(found) if found < target => {
            if registry.can_migrate_from(found) {
                Err(StoreError::NeedsMigration { found })
            } else {
                Err(StoreError::Older { found })
            }
        }
        Stamp::Format(found) => Err(StoreError::Newer { found }),
        Stamp::Unstamped => Err(StoreError::Unstamped),
        Stamp::Unreadable(reason) => Err(StoreError::UnreadableStamp { reason }),
        Stamp::Invalid(text) => Err(StoreError::InvalidStamp { text }),
        Stamp::Merged(ours, theirs) => Err(StoreError::MergedFormats { ours, theirs }),
    }
}

/// Object store manages content-addressable storage
pub struct ObjectStore {
    objects_dir: PathBuf,  // .comline/objects/
    /// Where a [`scratch`](ObjectStore::scratch) store keeps what it writes.
    scratch: Option<std::sync::Mutex<std::collections::HashMap<Hash, Vec<u8>>>>,
}

impl ObjectStore {
    /// Create a new object store
    pub fn new(project_root: &Path) -> Self {
        Self {
            objects_dir: project_root.join(".comline/objects"),
            scratch: None,
        }
    }

    /// The object store of the store directory `dir` (`.comline/`, or
    /// `.comline.migrating/` while a migration writes it), not of a project.
    pub fn at(dir: &Path) -> Self {
        Self { objects_dir: dir.join("objects"), scratch: None }
    }

    /// A store that reads the project's objects but keeps what it writes in
    /// memory: for working out what a build would store without storing it.
    pub fn scratch(project_root: &Path) -> Self {
        Self { scratch: Some(Default::default()), ..Self::new(project_root) }
    }

    /// Initialize the store (create directories), stamping it with
    /// [`CURRENT_IR_FORMAT_VERSION`] if this is the first time. A store
    /// [`ensure_compatible_store`] refuses is refused here too, before
    /// anything is created: `init` never stamps a store whose format it
    /// doesn't know as if it were current.
    pub fn init(&self) -> Result<()> {
        if self.scratch.is_some() {
            return Ok(());
        }
        let project_root = self.objects_dir.parent().and_then(Path::parent);
        if let Some(root) = project_root {
            ensure_compatible_store(root)?;
        }
        fs::create_dir_all(&self.objects_dir)?;
        let Some(comline_dir) = self.objects_dir.parent() else {
            return Ok(());
        };
        let stamp = comline_dir.join("FORMAT_VERSION");
        if !stamp.exists() {
            fs::write(&stamp, CURRENT_IR_FORMAT_VERSION.to_string())?;
        }
        Ok(())
    }

    /// Write content to store, returns hash
    pub fn write(&self, content: &[u8]) -> Result<Hash> {
        let hash = Hash::from_bytes(content);

        if let Some(scratch) = &self.scratch {
            if !self.exists(&hash) {
                scratch.lock().expect("scratch store").insert(hash, content.to_vec());
            }
            return Ok(hash);
        }

        // Check if already exists (deduplication)
        if self.exists(&hash) {
            tracing::debug!("Object {} already exists, skipping write", hash);
            return Ok(hash);
        }
        
        let compressed = compress(content);
        let path = self.objects_dir.join(hash.to_path());
        
        // Create parent directory (e.g., .comline/objects/ab/)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        tracing::trace!("Wrote object {} ({} byte(s) compressed)", hash, compressed.len());
        fs::write(&path, compressed)?;
        
        Ok(hash)
    }

    /// Read content from store by hash
    pub fn read(&self, hash: &Hash) -> Result<Vec<u8>> {
        if let Some(content) = self.scratch.as_ref().and_then(|s| s.lock().expect("scratch store").get(hash).cloned()) {
            return Ok(content);
        }
        let path = self.objects_dir.join(hash.to_path());
        
        if !path.exists() {
            return Err(eyre!("Object {} not found", hash));
        }
        
        let compressed = fs::read(path)?;
        let content = decompress(&compressed)?;
        
        // Verify hash matches
        let actual_hash = Hash::from_bytes(&content);
        if actual_hash != *hash {
            return Err(eyre!(
                "Hash mismatch: expected {}, got {}",
                hash,
                actual_hash
            ));
        }
        
        Ok(content)
    }

    /// Check if object exists
    pub fn exists(&self, hash: &Hash) -> bool {
        self.scratch.as_ref().is_some_and(|s| s.lock().expect("scratch store").contains_key(hash))
            || self.objects_dir.join(hash.to_path()).exists()
    }

    /// Get the path to an object (for debugging)
    pub fn object_path(&self, hash: &Hash) -> PathBuf {
        self.objects_dir.join(hash.to_path())
    }

    /// List all objects in the store (for debugging/GC)
    pub fn list_objects(&self) -> Result<Vec<Hash>> {
        let mut hashes = Vec::new();
        
        if !self.objects_dir.exists() {
            return Ok(hashes);
        }
        
        // Iterate through all subdirectories (ab/, cd/, etc.)
        for dir_entry in fs::read_dir(&self.objects_dir)? {
            let dir_entry = dir_entry?;
            let dir_path = dir_entry.path();
            
            if !dir_path.is_dir() {
                continue;
            }
            
            let dir_name = dir_entry.file_name().to_string_lossy().to_string();
            
            // Iterate through files in subdirectory
            for file_entry in fs::read_dir(&dir_path)? {
                let file_entry = file_entry?;
                let file_name = file_entry.file_name().to_string_lossy().to_string();
                
                // Reconstruct full hash (dir_name + file_name)
                let full_hex = format!("{}{}", dir_name, file_name);
                if let Ok(hash) = Hash::from_hex(&full_hex) {
                    hashes.push(hash);
                }
            }
        }
        
        Ok(hashes)
    }
}
