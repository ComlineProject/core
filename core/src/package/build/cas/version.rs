// Version bumping types for CAS

use serde_derive::{Deserialize, Serialize};

/// Type of semantic version bump to apply
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionBump {
    /// No version change
    None,
    /// Patch version bump (0.0.x)
    Patch,
    /// Minor version bump (0.x.0)
    Minor,
    /// Major version bump (x.0.0)
    Major,
}
