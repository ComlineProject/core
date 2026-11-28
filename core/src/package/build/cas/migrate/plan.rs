// The plan: the whole migration, in memory, writing nothing. Every version of
// the store is decoded under its format, carried through the chain one step
// at a time (each step held to its witness and to the unknowns it declares),
// and encoded under the target, with the objects it makes kept in memory.
// `comline store migrate --check` stops here, after the checks that need no
// disk; `migrate` writes the plan.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use eyre::{eyre, Result};

use comline_ir_formats::v3::types::UnknownRecord;

use super::super::objects::{Commit, Release};
use super::super::reconcile::Heads;
use super::super::storage::Hash;
use super::container::{CommitNode, Container};
use super::error::MigrateError;
use super::format::{MemObjects, Registry};
use super::model::{path_matches, Erased, ReleaseRecord, Version, VersionId};

/// An unknown the migrated store will hold, with the version it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLine {
    pub version: String,
    pub record: UnknownRecord,
}

/// One version, as it was and as it will be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedVersion {
    pub version: String,
    /// The commit's hash now.
    pub old: Hash,
    /// The commit's hash after the migration.
    pub new: Hash,
}

/// What a migration would do. Everything in it was computed without writing
/// anything.
#[derive(Debug)]
pub struct Plan {
    /// The format the store is in (or was assumed to be in).
    pub from: u32,
    /// The format it will be in.
    pub to: u32,
    /// `from` is the user's `--assume-format`, not the store's stamp.
    pub assumed: bool,
    /// The steps, `(N, N+1)` each, in order.
    pub steps: Vec<(u32, u32)>,
    /// Every version, oldest first (parents before children).
    pub versions: Vec<PlannedVersion>,
    /// Every unknown the new store will hold, version by version.
    pub unknowns: Vec<UnknownLine>,
    /// Objects in the new store.
    pub objects_total: usize,
    /// Of those, the ones that are not in the old store: written new.
    pub objects_rewritten: usize,
    /// Of those, the ones the old store has under the same hash: linked or
    /// copied, never rewritten.
    pub objects_unchanged: usize,
    /// Files and directories of `.comline/` that a migration does not carry
    /// (anything but the objects, the refs, the stamp, the receipt and
    /// `deps-cache/`, which the swap moves). They stay in the backup.
    pub not_carried: Vec<String>,
    pub(super) objects: MemObjects,
    pub(super) old_refs: BTreeMap<String, Heads>,
    pub(super) new_refs: BTreeMap<String, Heads>,
}

fn names_of(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn model_of(container: &Container, node: &CommitNode, payload: Erased) -> Result<Version<Erased>> {
    let id_of = |hash: &Hash| -> Result<VersionId> {
        container
            .index
            .get(hash)
            .copied()
            .ok_or_else(|| eyre!("commit {} is not in the history", hash))
    };
    let commit = &node.commit;
    Ok(Version {
        id: node.id,
        parents: commit.parents.iter().map(id_of).collect::<Result<_>>()?,
        version: commit.version.clone(),
        release: match &commit.release {
            None => None,
            Some(release) => Some(ReleaseRecord {
                computed: release.computed,
                applied: release.applied,
                reason: release.reason.clone(),
                covered: release.covered,
                stable: release.stable,
                since: release.since.as_ref().map(id_of).transpose()?,
            }),
        },
        author: commit.author.clone(),
        timestamp: commit.timestamp,
        message: commit.message.clone(),
        payload,
    })
}

impl Plan {
    /// Plan the migration of the store in `dir` from format `from` to the
    /// registry's target. Reads every object (checking it against its hash);
    /// writes nothing.
    pub(super) fn build(registry: &Registry, dir: &Path, from: u32, assumed: bool) -> Result<Plan> {
        let container = Container::open(dir)?;
        let chain = registry.chain(from)?;
        let from_format = registry.format(from)?;
        let to = registry.target();
        let to_format = registry.format(to)?;

        // Decode under `from`, and carry each version through the chain.
        let mut migrated: Vec<Version<Erased>> = Vec::with_capacity(container.commits.len());
        for node in &container.commits {
            let label = node.commit.version.clone();
            let payload = from_format
                .decode(&container.store, &node.commit.tree)
                .map_err(|e| MigrateError::Undecodable {
                    version: label.clone(),
                    format: from,
                    reason: format!("{e:#}"),
                })?;
            let mut version = model_of(&container, node, payload)?;
            let mut observed = from_format.unknowns(&version.payload)?;

            for step in &chain {
                let name = format!("{}-{}", step.from_format(), step.to_format());
                let failed = |reason: String| MigrateError::StepFailed {
                    step: name.clone(),
                    version: label.clone(),
                    reason,
                };
                let after = step.up(&version).map_err(|e| failed(format!("{e:#}")))?;
                if !after.same_commit_fields(&version) {
                    return Err(failed(
                        "it changed the version's own fields (id, parents, version, release, \
                         author, timestamp or message), which no step may"
                            .into(),
                    )
                    .into());
                }
                // The witness: nothing the version recorded was lost.
                if !step
                    .witness(&version, &after)
                    .map_err(|e| failed(format!("{e:#}")))?
                {
                    return Err(MigrateError::Witness {
                        step: name.clone(),
                        version: label.clone(),
                    }
                    .into());
                }
                // The unknowns it wrote are the ones it declares.
                let now = registry
                    .format(step.to_format())?
                    .unknowns(&after.payload)?;
                let declared = step.unknown();
                for record in &now {
                    if observed.iter().any(|before| before.path == record.path) {
                        continue;
                    }
                    if record.step != name {
                        return Err(failed(format!(
                            "it wrote an unknown at {} labelled with step {}",
                            record.path, record.step
                        ))
                        .into());
                    }
                    if !declared
                        .iter()
                        .any(|d| path_matches(&d.pattern, &record.path))
                    {
                        return Err(MigrateError::UndeclaredUnknown {
                            step: name.clone(),
                            version: label.clone(),
                            path: record.path.clone(),
                        }
                        .into());
                    }
                }
                observed = now;
                version = after;
            }
            migrated.push(version);
        }

        // Encode under the target: the objects of the new store, and each
        // commit with its parents and `since` mapped to the new hashes.
        let mut objects = MemObjects::default();
        let mut new_hashes: Vec<Hash> = Vec::with_capacity(migrated.len());
        let mut versions = Vec::with_capacity(migrated.len());
        let mut unknowns = vec![];
        for (version, node) in migrated.iter().zip(&container.commits) {
            let root = to_format.encode(&version.payload, &mut objects)?;
            to_format
                .canonical(&version.payload, &objects, &root)
                .map_err(|e| MigrateError::Verification {
                    check: "canonical encoding",
                    reason: format!("version {}: {e:#}", version.version),
                })?;

            let mapped = |id: &VersionId| -> Result<Hash> {
                new_hashes
                    .get(id.0)
                    .copied()
                    .ok_or_else(|| eyre!("version {} refers to a later version", version.version))
            };
            let commit = Commit {
                tree: root,
                parents: version.parents.iter().map(mapped).collect::<Result<_>>()?,
                author: version.author.clone(),
                timestamp: version.timestamp,
                message: version.message.clone(),
                version: version.version.clone(),
                release: match &version.release {
                    None => None,
                    Some(release) => Some(Release {
                        computed: release.computed,
                        applied: release.applied,
                        reason: release.reason.clone(),
                        covered: release.covered,
                        stable: release.stable,
                        since: release.since.as_ref().map(mapped).transpose()?,
                    }),
                },
            };
            let hash = objects.put(commit.to_bytes()?);
            new_hashes.push(hash);
            versions.push(PlannedVersion {
                version: version.version.clone(),
                old: node.hash,
                new: hash,
            });
            for record in to_format.unknowns(&version.payload)? {
                unknowns.push(UnknownLine {
                    version: version.version.clone(),
                    record,
                });
            }
        }

        let new_hash_of = |old: &Hash| -> Result<Hash> {
            container
                .index
                .get(old)
                .map(|id| new_hashes[id.0])
                .ok_or_else(|| eyre!("a ref names commit {old}, which is not in the history"))
        };
        let mut new_refs = BTreeMap::new();
        for (name, heads) in &container.refs {
            let mapped = match heads {
                Heads::One(head) => Heads::One(new_hash_of(head)?),
                Heads::Two(ours, theirs) => Heads::Two(new_hash_of(ours)?, new_hash_of(theirs)?),
            };
            new_refs.insert(name.clone(), mapped);
        }

        let objects_total = objects.len();
        let objects_unchanged = objects
            .iter()
            .filter(|(hash, _)| container.objects.contains(hash))
            .count();
        let not_carried = names_of(dir)
            .into_iter()
            .filter(|name| {
                ![
                    "objects",
                    "refs",
                    "FORMAT_VERSION",
                    "deps-cache",
                    super::RECEIPT_FILE,
                ]
                .contains(&name.as_str())
            })
            .collect();

        Ok(Plan {
            from,
            to,
            assumed,
            steps: chain
                .iter()
                .map(|s| (s.from_format(), s.to_format()))
                .collect(),
            versions,
            unknowns,
            objects_total,
            objects_rewritten: objects_total - objects_unchanged,
            objects_unchanged,
            not_carried,
            objects,
            old_refs: container.refs.clone(),
            new_refs,
        })
    }

    /// The old and new hash of every commit, oldest first.
    pub fn map(&self) -> Vec<(Hash, Hash)> {
        self.versions.iter().map(|v| (v.old, v.new)).collect()
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (first, last) = (self.versions.first(), self.versions.last());
        write!(
            f,
            "store format {}{}, {} version(s)",
            self.from,
            if self.assumed { " (assumed)" } else { "" },
            self.versions.len()
        )?;
        if let (Some(first), Some(last)) = (first, last) {
            write!(f, " ({} ... {})", first.version, last.version)?;
        }
        writeln!(f)?;
        if self.steps.is_empty() {
            writeln!(
                f,
                "  would stamp it format {} (no step to run): {} object(s), none rewritten",
                self.to, self.objects_total
            )?;
        } else {
            writeln!(
                f,
                "  would migrate {} -> {}: {} version(s), {} object(s) rewritten, {} unchanged",
                self.from,
                self.to,
                self.versions.len(),
                self.objects_rewritten,
                self.objects_unchanged
            )?;
        }
        writeln!(f, "  unknown after migration: {}", self.unknowns.len())?;
        for line in &self.unknowns {
            writeln!(
                f,
                "    {}  {}  {}",
                line.version, line.record.path, line.record.reason
            )?;
        }
        for name in &self.not_carried {
            writeln!(f, "  not carried over (it stays in the backup): {name}")?;
        }
        write!(
            f,
            "  nothing deleted: the old store is kept as .comline.format-{}.bak",
            self.from
        )
    }
}
