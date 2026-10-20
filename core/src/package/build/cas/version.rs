// Version bumping types for CAS

use serde_derive::{Deserialize, Serialize};

/// Type of semantic version bump to apply
/// Ordering: Major > Minor > Patch > None (for aggregating across schemas)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VersionBump {
    /// No version change
    None,
    /// Patch version bump (0.0.x) - lowest priority
    Patch,
    /// Minor version bump (0.x.0)
    Minor,
    /// Major version bump (x.0.0) - highest priority
    Major,
}

/// Whether `version` names a release: no pre-release part. A build between
/// releases carries one (`0.3.0-dev.2`); versions from before builds and
/// releases were separate have none, and count as releases.
pub fn is_release(version: &str) -> bool {
    semver::Version::parse(version).map_or(false, |v| v.pre.is_empty())
}

/// The version a release with `bump` makes after `last`.
///
/// Before 1.0 the minor plays the major's part, as Cargo does: a breaking
/// change goes `0.3.2` -> `0.4.0`, anything else `0.3.2` -> `0.3.3`. A
/// `0.0.z` version (from before the first release was `0.1.0`) moves to
/// `0.1.0` whatever changed. With no previous release, it's `0.1.0`.
pub fn next_release(last: Option<&semver::Version>, bump: VersionBump) -> semver::Version {
    use semver::Version;
    let Some(last) = last else {
        return Version::new(0, 1, 0);
    };
    let last = Version::new(last.major, last.minor, last.patch);
    match (last.major, last.minor, bump) {
        (_, _, VersionBump::None) => last,
        (0, 0, _) => Version::new(0, 1, 0),
        (0, minor, VersionBump::Major) => Version::new(0, minor + 1, 0),
        (0, minor, _) => Version::new(0, minor, last.patch + 1),
        (major, _, VersionBump::Major) => Version::new(major + 1, 0, 0),
        (major, minor, VersionBump::Minor) => Version::new(major, minor + 1, 0),
        (major, minor, VersionBump::Patch) => Version::new(major, minor, last.patch + 1),
    }
}

/// A build's version: the release it leads to, and how many builds since
/// the last release it is (`0.3.0-dev.2`).
pub fn dev_version(release: &semver::Version, count: u64) -> semver::Version {
    let mut version = release.clone();
    version.pre = semver::Prerelease::new(&format!("dev.{count}")).expect("a valid pre-release");
    version
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn next(last: &str, bump: VersionBump) -> String {
        next_release(Some(&Version::parse(last).unwrap()), bump).to_string()
    }

    #[test]
    fn before_1_0_the_minor_is_the_major() {
        assert_eq!(next("0.3.2", VersionBump::Major), "0.4.0");
        assert_eq!(next("0.3.2", VersionBump::Minor), "0.3.3");
        assert_eq!(next("0.3.2", VersionBump::Patch), "0.3.3");
        assert_eq!(next("0.3.2", VersionBump::None), "0.3.2");
    }

    #[test]
    fn from_1_0_it_is_plain_semver() {
        assert_eq!(next("1.2.3", VersionBump::Major), "2.0.0");
        assert_eq!(next("1.2.3", VersionBump::Minor), "1.3.0");
        assert_eq!(next("1.2.3", VersionBump::Patch), "1.2.4");
    }

    #[test]
    fn the_0_0_era_ends_at_0_1_0_and_a_first_release_is_0_1_0() {
        assert_eq!(next("0.0.4", VersionBump::Patch), "0.1.0");
        assert_eq!(next("0.0.4", VersionBump::Major), "0.1.0");
        assert_eq!(next_release(None, VersionBump::Major).to_string(), "0.1.0");
    }

    #[test]
    fn builds_sort_before_the_release_they_lead_to() {
        let release = Version::parse("1.3.0").unwrap();
        let build = dev_version(&release, 4);
        assert_eq!(build.to_string(), "1.3.0-dev.4");
        assert!(build < release);
        assert!(build > Version::parse("1.2.9").unwrap());
        assert!(!is_release("1.3.0-dev.4"));
        assert!(is_release("1.3.0") && is_release("0.0.1"));
    }
}
