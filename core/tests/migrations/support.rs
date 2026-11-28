// Helpers the engine's tests share.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Every file and directory under `root`, by relative path, with its bytes.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if path.is_dir() {
                into.insert(relative, None);
                walk(root, &path, into);
            } else {
                into.insert(relative, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

/// Every file under `dir`, relative to it.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    snapshot(dir)
        .into_iter()
        .filter(|(_, bytes)| bytes.is_some())
        .map(|(path, _)| path)
        .collect()
}
