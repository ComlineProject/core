// Reconciling a history that diverged after a Git merge (core#160).
//
// Two Git branches that each build each move `refs/heads/main`. Merging
// them merges the object files cleanly (they're named by hash), but the ref
// is one file, so Git leaves conflict markers in it, one head per side.
// The next build reads both heads and rebuilds one line from them:
//
// - If one head is an ancestor of the other, the newer one is the line
//   (a fast-forward): nothing to rebuild.
// - Otherwise the line continues from one head, the *onto* side: the one
//   that released since the two split, or, if neither did, the head built
//   first (earlier timestamp, then lower hash). The build commits the
//   merged working tree on top of it, the other side's net change as one
//   build, with the other head as its second parent so those commits stay
//   reachable. Its version, `-dev.N` count and facts are computed the
//   normal way, against the onto side's last release: anything unreleased
//   on the other side is renumbered to fit, and released versions keep
//   theirs.
// - If both sides released since they split, it refuses: two releases
//   can't both keep their version.
//
// Wire numbers (`comline@2`, runtime#15) need nothing extra here: like a
// protocol's line and its functions' "since", they'll be assigned against
// the last release on the first-parent line (`crate::compat::history`),
// so the reconciling build renumbers the ones added since that release the
// same way, and released ones keep theirs. Whatever assigns them must
// follow first parents only, as every history walk does today.
//
// Nothing is deleted: the other side's commits stay in the store. The ref
// is written clean (one hash) by the build that reconciles.

// Standard Uses
use std::collections::HashSet;
use std::fs;
use std::path::Path;

// Crate Uses
use super::object_store::ObjectStore;
use super::objects::Commit;
use super::refs::main_ref;
use super::storage::Hash;

// External Uses
use eyre::{bail, eyre, Result};

/// What `refs/heads/main` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heads {
    /// A clean ref: one head.
    One(Hash),
    /// A ref Git left conflicted after a merge: one head per side, ours
    /// then theirs.
    Two(Hash, Hash),
}

/// Whether `text`, a ref file's contents, holds Git conflict markers.
pub fn is_conflicted(text: &str) -> bool {
    text.lines().any(|line| line.starts_with("<<<<<<<"))
}

/// Parse a ref file: one hash, or a conflicted ref with a hash on each
/// side. Both Git conflict styles are read: `merge` (ours / theirs) and
/// `diff3` / `zdiff3`, whose base section (`|||||||`) is skipped.
pub fn parse_heads(text: &str) -> Result<Heads> {
    if !is_conflicted(text) {
        return Ok(Heads::One(Hash::from_hex(text.trim())?));
    }

    #[derive(PartialEq)]
    enum Section { Outside, Ours, Base, Theirs }
    let mut section = Section::Outside;
    let (mut ours, mut theirs) = (vec![], vec![]);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("<<<<<<<") {
            section = Section::Ours;
        } else if line.starts_with("|||||||") {
            section = Section::Base;
        } else if line.starts_with("=======") {
            section = Section::Theirs;
        } else if line.starts_with(">>>>>>>") {
            section = Section::Outside;
        } else if !line.is_empty() {
            match section {
                Section::Ours => ours.push(line),
                Section::Theirs => theirs.push(line),
                Section::Base => {}
                Section::Outside => bail!(
                    "`.comline/{}` has conflict markers and other content: {line:?}",
                    main_ref()
                ),
            }
        }
    }
    let one = |side: Vec<&str>, which: &str| -> Result<Hash> {
        match side.as_slice() {
            [hex] => Hash::from_hex(hex),
            _ => Err(eyre!(
                "`.comline/{}` is conflicted, but {which} side doesn't hold exactly one commit \
                 hash",
                main_ref()
            )),
        }
    };
    let (ours, theirs) = (one(ours, "our")?, one(theirs, "their")?);
    Ok(if ours == theirs { Heads::One(ours) } else { Heads::Two(ours, theirs) })
}

/// Read `refs/heads/main`: one head, or two after a Git merge.
pub fn read_heads(project_path: &Path) -> Result<Heads> {
    let path = project_path.join(".comline").join(main_ref());
    if !path.exists() {
        bail!("Ref {} does not exist", main_ref());
    }
    parse_heads(&fs::read_to_string(&path)?)
}

/// Mark the history as diverged by hand: when a merge's conflict on
/// `refs/heads/main` was already resolved by picking one side, this puts
/// `other` (the head that was dropped) back beside it, so the next build
/// reconciles the two. A no-op if the ref already points at `other`.
///
/// It writes the ref in the same form Git leaves a conflict in, so there's
/// one path for both. `comline build --reconcile <hash>` calls this
/// before building.
pub fn mark_diverged(project_path: &Path, other: &Hash) -> Result<()> {
    let store = ObjectStore::new(project_path);
    if !store.exists(other) {
        bail!("no commit {other} in `.comline/objects`");
    }
    Commit::from_bytes(&store.read(other)?)
        .map_err(|e| eyre!("{other} isn't a commit: {e}"))?;
    let current = match read_heads(project_path)? {
        Heads::One(hash) => hash,
        Heads::Two(..) => bail!("`.comline/{}` already holds two heads", main_ref()),
    };
    if current == *other {
        return Ok(());
    }
    let path = project_path.join(".comline").join(main_ref());
    fs::write(
        path,
        format!("<<<<<<< current\n{current}\n=======\n{other}\n>>>>>>> reconcile\n"),
    )?;
    Ok(())
}

/// What a build did with a history that diverged: carried by
/// [`BuildInfo`](super::build::BuildInfo) and the build result, so a caller
/// can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reconciled {
    /// The head the line continued from: the build's first parent, or the
    /// head the ref was repaired to when nothing needed building.
    pub onto: Hash,
    /// The other head, recorded as the new build's second parent. `None` on
    /// a fast-forward (one head already contained the other), or when
    /// nothing changed and no build was recorded.
    pub second_parent: Option<Hash>,
}

/// How a build continues from two heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The head the line continues from: the build's first parent.
    pub onto: Hash,
    /// The other head, recorded as the build's second parent. `None` on a
    /// fast-forward (one head already contains the other).
    pub other: Option<Hash>,
    /// The newest commit both heads share, if any.
    pub base: Option<Hash>,
}

/// Every commit reachable from `head`, through all parents.
fn ancestry(store: &ObjectStore, head: Hash) -> Result<HashSet<Hash>> {
    let mut seen = HashSet::new();
    let mut stack = vec![head];
    while let Some(hash) = stack.pop() {
        if !seen.insert(hash) {
            continue;
        }
        let commit = read_commit(store, &hash)?;
        stack.extend(commit.parents.iter().copied());
    }
    Ok(seen)
}

fn read_commit(store: &ObjectStore, hash: &Hash) -> Result<Commit> {
    if !store.exists(hash) {
        bail!(
            "commit {hash} isn't in `.comline/objects`: merge both branches' `.comline/objects` \
             before building"
        );
    }
    Commit::from_bytes(&store.read(hash)?)
}

/// The releases on `head`'s side only: reachable from it but not from the
/// other side, newest first along first parents.
fn releases_only(
    store: &ObjectStore,
    head: Hash,
    only: &HashSet<Hash>,
) -> Result<Vec<(Hash, Commit)>> {
    let mut found = vec![];
    for hash in only {
        let commit = read_commit(store, hash)?;
        if commit.is_release() {
            found.push((*hash, commit));
        }
    }
    // Newest first: by first-parent distance from the head where it's on
    // that line, then by timestamp.
    let mut line = vec![];
    let mut at = Some(head);
    while let Some(hash) = at.filter(|h| only.contains(h)) {
        line.push(hash);
        at = read_commit(store, &hash)?.parents.first().copied();
    }
    found.sort_by_key(|(hash, commit)| {
        (line.iter().position(|h| h == hash).unwrap_or(usize::MAX), -commit.timestamp)
    });
    Ok(found)
}

/// Decide how to continue from two heads: see the module comment.
pub fn plan(store: &ObjectStore, a: Hash, b: Hash) -> Result<Plan> {
    let (from_a, from_b) = (ancestry(store, a)?, ancestry(store, b)?);

    // The newest shared commit along b's first parents.
    let mut base = None;
    let mut at = Some(b);
    while let Some(hash) = at {
        if from_a.contains(&hash) {
            base = Some(hash);
            break;
        }
        at = read_commit(store, &hash)?.parents.first().copied();
    }

    if from_b.contains(&a) {
        return Ok(Plan { onto: b, other: None, base });
    }
    if from_a.contains(&b) {
        return Ok(Plan { onto: a, other: None, base });
    }

    let only_a: HashSet<Hash> = from_a.difference(&from_b).copied().collect();
    let only_b: HashSet<Hash> = from_b.difference(&from_a).copied().collect();
    let (released_a, released_b) =
        (releases_only(store, a, &only_a)?, releases_only(store, b, &only_b)?);

    let onto = match (released_a.first(), released_b.first()) {
        (Some((_, ra)), Some((_, rb))) => {
            let base = match base {
                Some(hash) => format!("{} ({})", read_commit(store, &hash)?.version, short(&hash)),
                None => "the start of either history (they share no commit)".to_string(),
            };
            bail!(
                "the history diverged and both branches released since they split from {base}: \
                 {} ({}) on one, {} ({}) on the other. Two releases can't both keep their \
                 version, so this build can't reconcile them.\n\
                 Pick the release to keep: write its branch's head into `.comline/{ref_}` \
                 ({} for {}, {} for {}), then build again. The other branch's changes then \
                 build on top of it, unreleased until the next release; its own release stays \
                 in `.comline/objects` but leaves the line.",
                ra.version, short(&released_a[0].0), rb.version, short(&released_b[0].0),
                a, ra.version, b, rb.version,
                ref_ = main_ref(),
            );
        }
        (Some(_), None) => a,
        (None, Some(_)) => b,
        (None, None) => {
            let (ca, cb) = (read_commit(store, &a)?, read_commit(store, &b)?);
            if (ca.timestamp, a.to_hex()) <= (cb.timestamp, b.to_hex()) { a } else { b }
        }
    };
    let other = if onto == a { b } else { a };
    Ok(Plan { onto, other: Some(other), base })
}

fn short(hash: &Hash) -> String {
    hash.to_hex()[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
    const B: &str = "bb00000000000000000000000000000000000000000000000000000000000000";
    const BASE: &str = "cc00000000000000000000000000000000000000000000000000000000000000";

    fn h(hex: &str) -> Hash {
        Hash::from_hex(hex).unwrap()
    }

    #[test]
    fn a_clean_ref_is_one_head() {
        assert_eq!(parse_heads(&format!("{A}\n")).unwrap(), Heads::One(h(A)));
    }

    #[test]
    fn a_merge_conflict_is_two_heads() {
        let text = format!("<<<<<<< HEAD\n{A}\n=======\n{B}\n>>>>>>> feature\n");
        assert_eq!(parse_heads(&text).unwrap(), Heads::Two(h(A), h(B)));
    }

    #[test]
    fn a_diff3_conflict_skips_the_base() {
        let text = format!(
            "<<<<<<< ours\n{A}\n||||||| base\n{BASE}\n=======\n{B}\n>>>>>>> theirs\n"
        );
        assert_eq!(parse_heads(&text).unwrap(), Heads::Two(h(A), h(B)));
    }

    #[test]
    fn the_same_head_on_both_sides_is_one() {
        let text = format!("<<<<<<< HEAD\n{A}\n=======\n{A}\n>>>>>>> feature\n");
        assert_eq!(parse_heads(&text).unwrap(), Heads::One(h(A)));
    }

    #[test]
    fn a_side_without_one_hash_is_an_error() {
        let text = format!("<<<<<<< HEAD\n=======\n{B}\n>>>>>>> feature\n");
        assert!(parse_heads(&text).is_err());
        let text = format!("{BASE}\n<<<<<<< HEAD\n{A}\n=======\n{B}\n>>>>>>> feature\n");
        assert!(parse_heads(&text).is_err());
    }
}
