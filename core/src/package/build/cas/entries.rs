// The root-tree entries the IR migrations framework reserves (core#268):
// what a step could not know (`unknown`), and the core and specification that
// wrote a version (`produced_by`). Nothing writes either yet. They are read
// here first, and fail closed, so that no step or build can write one into a
// store that a reader would take for complete.

use std::fmt;

use comline_ir_formats::v3::codec;
use comline_ir_formats::v3::entry::{PRODUCED_BY, UNKNOWN};
use comline_ir_formats::v3::types::{ProducedBy, UnknownRecord};
use eyre::{eyre, Result};

use super::objects::{Blob, EntryMode, Tree};
use super::ObjectStore;

/// A version records parts that the format it came from could not know, and
/// this comline can't say what they are: whatever it would read from the
/// version it would read as complete. **Nothing was changed**, and the
/// version isn't used. Recovered with `downcast_ref` from the `eyre::Report`
/// it travels in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionHasUnknowns {
    /// What the version's `unknown` entry lists.
    pub records: Vec<UnknownRecord>,
}

impl fmt::Display for VersionHasUnknowns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let paths: Vec<&str> = self.records.iter().map(|r| r.path.as_str()).collect();
        write!(
            f,
            "this version records {} part(s) that its IR format could not know ({}), and this \
             comline can't read them yet: it doesn't use the rest of the version as if it were \
             complete. Nothing was changed. Use a comline that reads them.",
            self.records.len(),
            paths.join(", ")
        )
    }
}

impl std::error::Error for VersionHasUnknowns {}

/// The blob a root tree holds under `name`, if it holds one. An entry of that
/// name that isn't a blob is an error: the name is reserved, not skipped.
fn read_entry(store: &ObjectStore, tree: &Tree, name: &str) -> Result<Option<Vec<u8>>> {
    let Some(entry) = tree.find_entry(name) else {
        return Ok(None);
    };
    if entry.mode != EntryMode::Blob {
        return Err(eyre!("the `{name}` entry of a version is not a blob"));
    }
    Ok(Some(Blob::from_bytes(&store.read(&entry.hash)?)?.content))
}

/// What the root tree's `unknown` entry lists: nothing for a version without
/// one. An entry that doesn't decode, or that has a field this comline
/// doesn't know, is an error and not an empty list.
pub fn read_unknown(store: &ObjectStore, tree: &Tree) -> Result<Vec<UnknownRecord>> {
    match read_entry(store, tree, UNKNOWN)? {
        None => Ok(vec![]),
        Some(bytes) => {
            codec::decode_unknown(&bytes).map_err(|e| eyre!("the `{UNKNOWN}` entry: {e}"))
        }
    }
}

/// Fail closed: an error ([`VersionHasUnknowns`]) if the version has unknowns,
/// or if its `unknown` entry can't be read. Called before a version is used
/// as complete: compared, documented, its history read.
pub fn require_known(store: &ObjectStore, tree: &Tree) -> Result<()> {
    let records = read_unknown(store, tree)?;
    if records.is_empty() {
        Ok(())
    } else {
        Err(VersionHasUnknowns { records }.into())
    }
}

/// The core and specification that wrote the version, if it says. `None` is
/// **unknown**, not the reading core's: a version without the entry was
/// written before it existed, or by a step that couldn't know.
pub fn read_produced_by(store: &ObjectStore, tree: &Tree) -> Result<Option<ProducedBy>> {
    match read_entry(store, tree, PRODUCED_BY)? {
        None => Ok(None),
        Some(bytes) => codec::decode_produced_by(&bytes)
            .map(Some)
            .map_err(|e| eyre!("the `{PRODUCED_BY}` entry: {e}")),
    }
}
