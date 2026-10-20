// Releases: turning the builds since the last release into a version.
//
// A build records history under a pre-release version (`0.3.0-dev.2`). A
// release is its own commit on top of the latest build - same tree - with a
// plain version and a record of how it got there: the bump the changes call
// for, the bump applied, and why they differ. Consumers and the handshake see
// releases.

// Standard Uses
use std::path::Path;

// Crate Uses
use super::build::{compare, last_release, Comparison};
use super::object_store::ObjectStore;
use super::objects::{Commit, Release, Tree};
use super::refs::{main_ref, read_ref, ref_exists, update_ref};
use super::version::{next_release, VersionBump};
use crate::schema::ir::frozen::cas::commit::create_version_commit;

// External Uses
use eyre::{bail, Result};

/// What to release.
#[derive(Debug, Clone, Default)]
pub struct ReleaseRequest {
    /// Raise the bump above the computed one. Never lowers it.
    pub raise: Option<VersionBump>,
    /// Why: required with `raise`, optional with `stable`.
    pub reason: Option<String>,
    /// Move a `0.x` package to `1.0.0`.
    pub stable: bool,
}

/// What a release made.
#[derive(Debug)]
pub struct ReleaseInfo {
    /// The release before this one, if any.
    pub previous_release: Option<String>,
    /// The version released.
    pub version: String,
    /// What was recorded on the release commit.
    pub record: Release,
    /// The changes since the previous release (none for the first).
    pub changes: Option<Comparison>,
}

/// The latest release in the package's history, if any.
pub fn latest_release(project_path: &Path) -> Result<Option<Commit>> {
    if !ref_exists(project_path, main_ref()) {
        return Ok(None);
    }
    let store = ObjectStore::new(project_path);
    let (release, _) = last_release(&store, read_ref(project_path, main_ref())?)?;
    Ok(release.map(|(_, commit)| commit))
}

/// Release the latest build.
pub fn release(project_path: &Path, request: &ReleaseRequest) -> Result<ReleaseInfo> {
    if request.raise.is_some() && request.reason.as_deref().map_or(true, |r| r.trim().is_empty()) {
        bail!("raising the bump needs a reason, recorded with the release");
    }
    if request.raise.is_some() && request.stable {
        bail!("a release to 1.0.0 can't also raise the bump: 1.0.0 is the version it makes");
    }
    if !ref_exists(project_path, main_ref()) {
        bail!("nothing built yet: run `comline build` first");
    }

    let store = ObjectStore::new(project_path);
    let head_hash = read_ref(project_path, main_ref())?;
    let head = Commit::from_bytes(&store.read(&head_hash)?)?;
    let (previous, _) = last_release(&store, head_hash)?;
    let previous_version = previous
        .as_ref()
        .map(|(_, commit)| semver::Version::parse(&commit.version))
        .transpose()?;

    if head.is_release() && !request.stable {
        bail!(
            "nothing to release: {} is already released, and nothing was built since",
            head.version
        );
    }

    let changes = match &previous {
        Some((_, commit)) => {
            let released = Tree::from_bytes(&store.read(&commit.tree)?)?;
            let built = Tree::from_bytes(&store.read(&head.tree)?)?;
            Some(compare(&store, &released, &built)?)
        }
        None => None,
    };
    let computed = changes.as_ref().map_or(VersionBump::None, |c| c.bump);

    let (version, applied, covered) = if request.stable {
        if previous_version.as_ref().is_some_and(|v| v.major >= 1) {
            bail!(
                "already stable: the last release is {}",
                previous_version.as_ref().unwrap()
            );
        }
        (semver::Version::new(1, 0, 0), VersionBump::Major, false)
    } else if previous.is_none() {
        if request.raise.is_some() {
            bail!(
                "the first release is 0.1.0, so there's nothing to raise from; release 1.0 to \
                 start at 1.0.0"
            );
        }
        (next_release(None, VersionBump::None), VersionBump::None, false)
    } else {
        let raise = request.raise.unwrap_or(VersionBump::None);
        let applied = computed.max(raise);
        if applied == VersionBump::None {
            bail!(
                "nothing since {} calls for a release; raise the bump, with a reason, to \
                 release anyway",
                previous_version.as_ref().unwrap()
            );
        }
        let covered = request.raise.is_some_and(|raise| raise <= computed);
        (next_release(previous_version.as_ref(), applied), applied, covered)
    };

    let record = Release {
        computed,
        applied,
        reason: request.reason.clone().filter(|r| !r.trim().is_empty()),
        covered,
        stable: request.stable,
        since: previous.as_ref().map(|(hash, _)| *hash),
    };
    let message = if request.stable {
        format!("Release {version}: stable")
    } else {
        format!("Release {version}")
    };
    let mut commit = create_version_commit(head.tree, head_hash, &version.to_string(), &message);
    commit.release = Some(record.clone());
    let commit_hash = store.write(&commit.to_bytes()?)?;
    update_ref(project_path, main_ref(), &commit_hash)?;

    Ok(ReleaseInfo {
        previous_release: previous.map(|(_, commit)| commit.version),
        version: version.to_string(),
        record,
        changes,
    })
}
