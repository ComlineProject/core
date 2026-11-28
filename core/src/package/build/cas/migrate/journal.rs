// A migration's journal: what is needed to tell, from the disk alone, where
// an interrupted migration stopped and whether it can go on (running.md,
// "Crashes"). It is written into `.comline.migrating/` when the directory is
// claimed (`writing`), rewritten `ready` once the new store is verified and
// before the first rename, and is the file that, left in `.comline/` after
// the swap, says the last moves (the cache, the receipt) are not done.

use std::collections::BTreeMap;
use std::path::Path;

use eyre::Result;
use serde_derive::{Deserialize, Serialize};

use super::{disk, journal_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalState {
    /// The new store is being written.
    Writing,
    /// The new store is written and verified: only the swap is left.
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub state: JournalState,
    /// The format the old store is in: its stamp, or the one assumed.
    pub from: u32,
    /// `from` was assumed with `--assume-format`.
    pub assumed: bool,
    /// The format the migration writes.
    pub to: u32,
    /// The heads of the old store, by ref, as hex: a migration resumes only
    /// if the store still has these.
    pub old_heads: BTreeMap<String, Vec<String>>,
    /// The directory (beside `.comline/`) the old store is renamed to.
    pub backup: String,
}

impl Journal {
    pub fn read(store_dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(journal_path(store_dir))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub(super) fn write(&self, store_dir: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        disk::write_file(&journal_path(store_dir), text.as_bytes())
    }
}
