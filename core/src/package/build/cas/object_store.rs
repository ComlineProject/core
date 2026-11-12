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
/// CBOR tags struct fields and enum variants by name, so growing or
/// reordering `FrozenUnit` needs no bump here - only *renaming* or
/// *removing* an existing field/variant does, since that's the one case a
/// self-describing format can't absorb for free. Bump this by hand when you
/// do one of those; [`ensure_compatible_store`] then refuses any `.comline/`
/// stamped with a different value instead of trying (and likely failing,
/// or in the worst case silently misreading) to decode it. It refuses and
/// changes nothing: no store is deleted, moved or rewritten to change its
/// format (core#268, `docs/proposals/ir-migrations`).
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
/// Additions that needed no bump: the root tree's `docs` blob (the `//!`
/// headers, `crate::doc::FrozenDocs`, core#145) is a new entry that older
/// readers skip and newer ones treat as absent on older versions. So are a
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

/// Why a `.comline/` can't be used as it is. **Nothing was changed** in any
/// of these cases, and nothing in core changes it afterwards: the store is
/// not deleted, moved or rewritten to change its format. Core is a library,
/// so the caller decides how to show this: a CLI prints it, the language
/// server makes it a diagnostic.
///
/// It is a `std::error::Error`, so it travels in an `eyre::Report` and is
/// recovered with `downcast_ref::<StoreError>()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Stamped with an older format than this binary writes, and no
    /// migration from it exists (there are no steps yet).
    Older { found: u32 },
    /// Stamped with a newer format than this binary reads: a newer comline
    /// wrote it.
    Newer { found: u32 },
    /// A `.comline/` with history in it and no `FORMAT_VERSION`.
    Unstamped,
    /// `FORMAT_VERSION` exists and can't be read.
    UnreadableStamp { reason: String },
    /// `FORMAT_VERSION` doesn't hold a format number.
    InvalidStamp { text: String },
}

/// `comline store reset` doesn't exist yet: step 1.3 of core#268 builds the
/// engine and `comline store`. Until then the message says so and tells
/// the user what to do by hand; 1.3 changes this one text.
const RESET_HINT: &str = "To start a new history, `comline store reset` will move `.comline/` aside \
     (to `.comline.format-N.bak/`) and keep `deps-cache/`, but that command is not in this \
     release yet: until it is, move `.comline/` aside yourself.";

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
                 changed. {RESET_HINT}"
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
        }
    }
}

impl std::error::Error for StoreError {}

/// Whether `dir` holds anything at all (a missing or unreadable one doesn't).
fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// Classify the `.comline/` under `project_root` against
/// [`CURRENT_IR_FORMAT_VERSION`]. **Read-only**: it never creates, moves,
/// rewrites or deletes anything, in any outcome. Call it before trusting
/// anything already in `.comline/` (before deciding whether a previous commit
/// exists to diff against), and before writing to it.
///
/// - no `.comline/`, or an unstamped one with no history: [`StoreState::Fresh`]
/// - the current stamp: [`StoreState::Current`]
/// - an older stamp, a newer one, a missing stamp on a store with history, a
///   stamp that can't be read or isn't a number: the matching [`StoreError`]
///
/// `deps-cache/` is not part of the store: a `.comline/` holding only it is
/// fresh, and an error here never touches it.
pub fn ensure_compatible_store(project_root: &Path) -> Result<StoreState, StoreError> {
    let comline_dir = project_root.join(".comline");
    if !comline_dir.exists() {
        return Ok(StoreState::Fresh); // first build ever - nothing to check
    }

    let text = match fs::read_to_string(format_version_path(project_root)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let has_history =
                has_entries(&comline_dir.join("refs")) || has_entries(&comline_dir.join("objects"));
            return if has_history { Err(StoreError::Unstamped) } else { Ok(StoreState::Fresh) };
        }
        Err(e) => return Err(StoreError::UnreadableStamp { reason: e.to_string() }),
    };

    let trimmed = text.trim();
    let found = Some(trimmed)
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<u32>().ok());
    match found {
        Some(found) if found == CURRENT_IR_FORMAT_VERSION => Ok(StoreState::Current),
        Some(found) if found < CURRENT_IR_FORMAT_VERSION => Err(StoreError::Older { found }),
        Some(found) => Err(StoreError::Newer { found }),
        None => Err(StoreError::InvalidStamp { text: trimmed.chars().take(64).collect() }),
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
