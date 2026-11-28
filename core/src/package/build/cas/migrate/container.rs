// Reading a store with no format knowledge: the refs, every commit reachable
// from them by parents (second parents included) and by a release's `since`,
// and every tree and blob under those commits, each object read through the
// object store, which checks it against its hash. The container is not a
// format (formats.md): it is the same in every CBOR format, so this reader
// works on a store whatever it is stamped with, and is what `comline store
// status`, `verify` and the migration plan start from.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use eyre::Result;

use super::super::objects::{Blob, Commit, EntryMode, Tree};
use super::super::reconcile::{parse_heads, Heads};
use super::super::storage::Hash;
use super::super::ObjectStore;
use super::error::MigrateError;
use super::model::VersionId;

/// One commit of the history, with its place in it.
#[derive(Debug, Clone)]
pub struct CommitNode {
    pub id: VersionId,
    pub hash: Hash,
    pub commit: Commit,
}

/// A store's refs, commits and the objects under them.
pub struct Container {
    /// The store directory: `.comline/`, `.comline.migrating/` or a backup.
    pub dir: PathBuf,
    pub store: ObjectStore,
    /// Each ref by its path under the store directory (`refs/heads/main`).
    pub refs: BTreeMap<String, Heads>,
    /// Every commit, parents before children: a version's id is its index.
    pub commits: Vec<CommitNode>,
    pub index: HashMap<Hash, VersionId>,
    /// Every object reachable from the refs: commits, trees and blobs.
    pub objects: BTreeSet<Hash>,
}

fn damaged(file: PathBuf, reason: impl Into<String>) -> eyre::Report {
    MigrateError::Damaged {
        file,
        reason: reason.into(),
    }
    .into()
}

/// The object `hash`, checked against its hash, or what is wrong with it.
fn read_object(store: &ObjectStore, hash: &Hash) -> Result<Vec<u8>> {
    let path = store.object_path(hash);
    if !path.exists() {
        return Err(damaged(path, "is missing"));
    }
    store
        .read(hash)
        .map_err(|e| damaged(path, format!("is damaged ({e})")))
}

fn read_commit(store: &ObjectStore, hash: &Hash) -> Result<Commit> {
    let bytes = read_object(store, hash)?;
    Commit::from_bytes(&bytes)
        .map_err(|e| damaged(store.object_path(hash), format!("is not a commit ({e})")))
}

fn read_tree(store: &ObjectStore, hash: &Hash) -> Result<Tree> {
    let bytes = read_object(store, hash)?;
    Tree::from_bytes(&bytes)
        .map_err(|e| damaged(store.object_path(hash), format!("is not a tree ({e})")))
}

fn read_blob(store: &ObjectStore, hash: &Hash) -> Result<()> {
    let bytes = read_object(store, hash)?;
    Blob::from_bytes(&bytes)
        .map(drop)
        .map_err(|e| damaged(store.object_path(hash), format!("is not a blob ({e})")))
}

/// Every ref under `dir/refs`, by its path relative to `dir`. A file ending
/// in `.tmp` is a write that did not finish, not a ref.
pub(super) fn read_refs(dir: &Path) -> Result<BTreeMap<String, Heads>> {
    let mut refs = BTreeMap::new();
    let base = dir.join("refs");
    if !base.is_dir() {
        return Ok(refs);
    }
    let mut pending = vec![base];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_some_and(|e| e == "tmp") {
                continue;
            }
            let name = path
                .strip_prefix(dir)
                .map(|relative| {
                    relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .unwrap_or_default();
            let text = fs::read_to_string(&path)
                .map_err(|e| damaged(path.clone(), format!("can't be read ({e})")))?;
            let heads = parse_heads(&text)
                .map_err(|e| damaged(path.clone(), format!("is not a ref ({e})")))?;
            refs.insert(name, heads);
        }
    }
    Ok(refs)
}

impl Container {
    /// Read the store in `dir`. Fails, naming the file, at the first object
    /// that is missing, doesn't decompress, doesn't match its hash or isn't
    /// what its place says (a commit, a tree, a blob), and at a ref that
    /// isn't one. A store with no history is an empty container.
    pub fn open(dir: &Path) -> Result<Self> {
        let store = ObjectStore::at(dir);
        let refs = read_refs(dir)?;

        if refs.is_empty() {
            if !store.list_objects()?.is_empty() {
                return Err(damaged(
                    dir.join("refs/heads/main"),
                    "is missing, and the store has objects that no ref reaches",
                ));
            }
            return Ok(Self {
                dir: dir.to_path_buf(),
                store,
                refs,
                commits: vec![],
                index: HashMap::new(),
                objects: BTreeSet::new(),
            });
        }

        let mut roots = vec![];
        for heads in refs.values() {
            match *heads {
                Heads::One(head) => roots.push(head),
                Heads::Two(ours, theirs) => roots.extend([ours, theirs]),
            }
        }

        // Every commit reachable from the refs.
        let mut commits: HashMap<Hash, Commit> = HashMap::new();
        let mut pending = roots.clone();
        while let Some(hash) = pending.pop() {
            if commits.contains_key(&hash) {
                continue;
            }
            let commit = read_commit(&store, &hash)?;
            pending.extend(dependencies(&commit));
            commits.insert(hash, commit);
        }

        // Parents (and a release's `since`) before their children, in an
        // order that depends only on the refs and the commits.
        let mut order: Vec<Hash> = vec![];
        let mut done: HashSet<Hash> = HashSet::new();
        for root in &roots {
            let mut stack = vec![(*root, false)];
            while let Some((hash, expanded)) = stack.pop() {
                if done.contains(&hash) {
                    continue;
                }
                if expanded {
                    done.insert(hash);
                    order.push(hash);
                    continue;
                }
                stack.push((hash, true));
                for dependency in dependencies(&commits[&hash]).into_iter().rev() {
                    if !done.contains(&dependency) {
                        stack.push((dependency, false));
                    }
                }
            }
        }

        let mut index = HashMap::new();
        let mut nodes = vec![];
        for (position, hash) in order.iter().enumerate() {
            let id = VersionId(position);
            index.insert(*hash, id);
            nodes.push(CommitNode {
                id,
                hash: *hash,
                commit: commits[hash].clone(),
            });
        }

        // Every tree and blob under those commits.
        let mut objects: BTreeSet<Hash> = order.iter().copied().collect();
        for node in &nodes {
            let mut stack = vec![node.commit.tree];
            while let Some(tree_hash) = stack.pop() {
                if !objects.insert(tree_hash) {
                    continue;
                }
                for entry in read_tree(&store, &tree_hash)?.entries {
                    match entry.mode {
                        EntryMode::Tree => stack.push(entry.hash),
                        EntryMode::Blob => {
                            if objects.insert(entry.hash) {
                                read_blob(&store, &entry.hash)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            dir: dir.to_path_buf(),
            store,
            refs,
            commits: nodes,
            index,
            objects,
        })
    }

    /// The commit `id`.
    pub fn commit(&self, id: VersionId) -> &CommitNode {
        &self.commits[id.0]
    }

    /// Every head the refs name, ours before theirs, by ref.
    pub fn heads(&self) -> Vec<Hash> {
        let mut heads = vec![];
        for entry in self.refs.values() {
            match *entry {
                Heads::One(head) => heads.push(head),
                Heads::Two(ours, theirs) => heads.extend([ours, theirs]),
            }
        }
        heads
    }
}

/// What a commit's meaning depends on besides its tree: its parents and the
/// release it counts from.
fn dependencies(commit: &Commit) -> Vec<Hash> {
    let mut found = commit.parents.clone();
    if let Some(since) = commit.release.as_ref().and_then(|release| release.since) {
        found.push(since);
    }
    found
}
