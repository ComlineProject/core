// The checks on a store directory (running.md, "Verifying"): the ones a new
// store passes before it replaces the old one, and, minus the ones that
// compare it with the old, what `comline store verify` runs on the current
// store.
//
//   1. every object decompresses, matches its hash, and every reference
//      (tree entry, parent, `release.since`, ref) resolves  -> `Container`
//   2. every version decodes under the format, and (for what the engine
//      wrote) encoding it again gives the same bytes
//   3. the old-to-new map is a bijection on commits, parents map to parents,
//      and each commit's `version`, `release`, `author`, `timestamp` and
//      `message` are the same                                 -> `check_map`
//   4. the witness holds, 5. the unknowns written are the ones declared
//                                                              -> the plan
//   6. a payload whose bytes are the same keeps its hash: by construction,
//      objects are named by their bytes, and the plan reports how many were
//      kept (`Plan::objects_unchanged`)
//
// Not here, and not yet anywhere: the report of a version's `facts`
// recomputed by the current core against the carried ones (running.md,
// "reported, not failed"). It needs a step that carries them.

use std::path::Path;

use eyre::Result;

use super::super::reconcile::Heads;
use super::super::storage::Hash;
use super::container::Container;
use super::error::MigrateError;
use super::format::Registry;

/// What `verify` found in a store that passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// The format the store was verified as.
    pub format: u32,
    pub versions: usize,
    pub objects: usize,
    /// Versions that decode, and whose bytes the current encoder would not
    /// write back as they are: a build of an earlier ledger level, or one
    /// that wrote an entry this comline doesn't use. Reported, not failed:
    /// a store holds commits of several ledger levels (formats.md), and the
    /// bytes of a version already written are never touched. Always empty for
    /// a store the engine wrote, where it is a failure.
    pub not_canonical: Vec<(String, Hash)>,
}

/// Run checks 1 and 2 on the store in `dir` as format `format`. With
/// `canonical`, a version whose encoding differs from its bytes fails; without
/// it, it is listed in the report.
pub(super) fn check_dir(
    registry: &Registry,
    dir: &Path,
    format: u32,
    canonical: bool,
) -> Result<(Container, VerifyReport)> {
    let container = Container::open(dir)?;
    let codec = registry.format(format)?;
    let mut not_canonical = vec![];
    for node in &container.commits {
        let label = &node.commit.version;
        let payload = codec
            .decode(&container.store, &node.commit.tree)
            .map_err(|e| MigrateError::Undecodable {
                version: label.clone(),
                format,
                reason: format!("{e:#}"),
            })?;
        if let Err(e) = codec.canonical(&payload, &container.store, &node.commit.tree) {
            if canonical {
                return Err(MigrateError::Verification {
                    check: "canonical encoding",
                    reason: format!("version {label}: {e:#}"),
                }
                .into());
            }
            not_canonical.push((label.clone(), node.hash));
        }
    }
    let report = VerifyReport {
        format,
        versions: container.commits.len(),
        objects: container.objects.len(),
        not_canonical,
    };
    Ok((container, report))
}

/// Check 3: `new` is `old` migrated, commit for commit. Returns the old and
/// new hash of every commit, oldest first.
pub(super) fn check_map(old: &Container, new: &Container) -> Result<Vec<(Hash, Hash)>> {
    let fail = |reason: String| -> eyre::Report {
        MigrateError::Verification {
            check: "old-to-new map",
            reason,
        }
        .into()
    };
    if old.commits.len() != new.commits.len() {
        return Err(fail(format!(
            "the store had {} version(s) and the new one has {}",
            old.commits.len(),
            new.commits.len()
        )));
    }
    let map: Vec<(Hash, Hash)> = old
        .commits
        .iter()
        .zip(&new.commits)
        .map(|(o, n)| (o.hash, n.hash))
        .collect();
    let mut distinct: Vec<&Hash> = map.iter().map(|(_, new)| new).collect();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != map.len() {
        return Err(fail("two versions became one".into()));
    }

    let new_of =
        |hash: &Hash| -> Option<Hash> { old.index.get(hash).map(|id| new.commits[id.0].hash) };
    for (o, n) in old.commits.iter().zip(&new.commits) {
        let (a, b) = (&o.commit, &n.commit);
        let label = &a.version;
        if a.version != b.version
            || a.author != b.author
            || a.timestamp != b.timestamp
            || a.message != b.message
        {
            return Err(fail(format!(
                "version {label}: a field of the commit changed"
            )));
        }
        let parents: Option<Vec<Hash>> = a.parents.iter().map(new_of).collect();
        if parents.as_deref() != Some(b.parents.as_slice()) {
            return Err(fail(format!(
                "version {label}: its parents don't map to the new ones"
            )));
        }
        let release_matches = match (&a.release, &b.release) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                x.computed == y.computed
                    && x.applied == y.applied
                    && x.reason == y.reason
                    && x.covered == y.covered
                    && x.stable == y.stable
                    && x.since.as_ref().map(new_of) == y.since.map(Some)
            }
            _ => false,
        };
        if !release_matches {
            return Err(fail(format!("version {label}: its release record changed")));
        }
    }

    // The refs name the same versions.
    if old.refs.keys().ne(new.refs.keys()) {
        return Err(fail("the refs are not the same".into()));
    }
    for (name, heads) in &old.refs {
        let mapped = match *heads {
            Heads::One(head) => new_of(&head).map(Heads::One),
            Heads::Two(ours, theirs) => match (new_of(&ours), new_of(&theirs)) {
                (Some(ours), Some(theirs)) => Some(Heads::Two(ours, theirs)),
                _ => None,
            },
        };
        if mapped != Some(new.refs[name]) {
            return Err(fail(format!("`{name}` doesn't name the migrated head")));
        }
    }
    Ok(map)
}
