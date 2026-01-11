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
