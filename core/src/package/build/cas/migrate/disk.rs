// Every place core moves, replaces or removes anything of a store, and the
// only file that may: `core_deletes_and_moves_no_store` allows exactly this
// file (and the dependency cache's own checkout handling) to call
// the renames and removals, so that a new way to delete something is a
// change to this file, reviewed against the rule: nothing is deleted or
// changed without the user doing so. Everything here is called only from the
// engine's operations, each of which takes the user's `Consent`.
//
// Writes are atomic: a file is written beside its name and renamed onto it,
// so a crash leaves a whole file or none, which is what lets a resumed
// migration trust an object that is already there.

use std::fs;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

fn temp_of(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Make `path`, whose parent exists. Fails if it exists: that is the lock.
pub(super) fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir(path).wrap_err_with(|| format!("can't create `{}`", path.display()))
}

/// Write `bytes` to `path` (its directories made as needed), atomically.
pub(super) fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temp_of(path);
    fs::write(&temp, bytes).wrap_err_with(|| format!("can't write `{}`", temp.display()))?;
    fs::rename(&temp, path).wrap_err_with(|| format!("can't write `{}`", path.display()))
}

/// Put `from`'s file at `to` under the same name: a hard link, which costs no
/// space, or a copy where the filesystem can't link. Atomic like a write.
pub(super) fn link_or_copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temp_of(to);
    let _ = fs::remove_file(&temp);
    if fs::hard_link(from, &temp).is_err() {
        fs::copy(from, &temp)
            .wrap_err_with(|| format!("can't copy `{}` to `{}`", from.display(), temp.display()))?;
    }
    fs::rename(&temp, to).wrap_err_with(|| format!("can't write `{}`", to.display()))
}

/// Move `from` to `to`. Atomic where both are on one filesystem.
pub(super) fn rename(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)
        .wrap_err_with(|| format!("can't move `{}` to `{}`", from.display(), to.display()))
}

/// Remove one file.
pub(super) fn remove_file(path: &Path) -> Result<()> {
    fs::remove_file(path).wrap_err_with(|| format!("can't remove `{}`", path.display()))
}

/// Remove a directory and all in it: a partial `.comline.migrating/` the user
/// asked to restart over, or backups the user asked to prune.
pub(super) fn remove_tree(path: &Path) -> Result<()> {
    fs::remove_dir_all(path).wrap_err_with(|| format!("can't remove `{}`", path.display()))
}

/// Remove the writes that did not finish (`*.tmp`) under `dir`.
pub(super) fn sweep_unfinished_writes(dir: &Path) -> Result<()> {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "tmp") {
                remove_file(&path)?;
            }
        }
    }
    Ok(())
}
