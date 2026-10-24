// Standard Uses

// Crate Uses
use crate::package::build::VersionBump;
use crate::package::config::ir::frozen::{wire_format, wire_framing, Dependency, FrozenUnit};

// External Uses

/// A single change to the frozen congregation, carrying its version-bump weight.
///
/// Only units that affect the package's *schema API* appear here.
/// `code_generation` (a tooling-capability list), `publish_registries` (publish
/// metadata) and `namespace` (a rename is a new package) are deliberately not
/// compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChange {
    /// The IDL spec version changed — the meaning of the schema syntax may have.
    SpecificationVersionChanged { old: u8, new: u8 },
    /// A dependency was removed. Conservatively breaking: consumers regenerating
    /// may reference types it provided.
    DependencyRemoved { project: String },
    /// A dependency's version changed. Conservatively breaking until dependency
    /// resolution can diff the two dependency schemas (core#6).
    DependencyVersionChanged {
        project: String,
        old: String,
        new: String,
    },
    /// A dependency was added — additive; the schema content that uses it
    /// carries its own bump.
    DependencyAdded { project: String, version: String },
    /// The package's wire format changed: old and new peers can't read each
    /// other.
    WireFormatChanged { old: String, new: String },
    /// The framing a protocol without `@framing` gets changed: old and new
    /// peers frame calls differently.
    WireFramingChanged { old: String, new: String },
}

impl ConfigChange {
    pub fn bump(&self) -> VersionBump {
        use ConfigChange::*;
        match self {
            SpecificationVersionChanged { .. }
            | DependencyRemoved { .. }
            | DependencyVersionChanged { .. }
            | WireFormatChanged { .. }
            | WireFramingChanged { .. } => VersionBump::Major,
            DependencyAdded { .. } => VersionBump::Minor,
        }
    }
}

/// The set of congregation changes between two frozen versions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConfigChanges(pub Vec<ConfigChange>);

impl ConfigChanges {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The largest bump any single change implies.
    pub fn bump(&self) -> VersionBump {
        self.0
            .iter()
            .map(ConfigChange::bump)
            .max()
            .unwrap_or(VersionBump::None)
    }
}

/// Diff two frozen congregations into the changes that move the package version.
///
/// `prev` is empty for a project whose last commit predates config being stored
/// in CAS — treated as "no prior config", so nothing is reported.
pub fn analyze_config_changes(prev: &[FrozenUnit], cur: &[FrozenUnit]) -> ConfigChanges {
    if prev.is_empty() {
        return ConfigChanges::default();
    }

    let mut changes = Vec::new();

    if let (Some(old), Some(new)) = (spec_version(prev), spec_version(cur)) {
        if old != new {
            changes.push(ConfigChange::SpecificationVersionChanged { old, new });
        }
    }

    // Compared as resolved, so writing out a default is no change.
    if wire_format(prev) != wire_format(cur) {
        changes.push(ConfigChange::WireFormatChanged {
            old: wire_format(prev).to_string(),
            new: wire_format(cur).to_string(),
        });
    }
    if wire_framing(prev) != wire_framing(cur) {
        changes.push(ConfigChange::WireFramingChanged {
            old: wire_framing(prev).to_string(),
            new: wire_framing(cur).to_string(),
        });
    }

    let prev_deps = dependencies(prev);
    let cur_deps = dependencies(cur);

    for cd in &cur_deps {
        match prev_deps.iter().find(|pd| pd.project == cd.project) {
            None => changes.push(ConfigChange::DependencyAdded {
                project: cd.project.clone(),
                version: cd.version.clone(),
            }),
            Some(pd) if pd.version != cd.version => {
                changes.push(ConfigChange::DependencyVersionChanged {
                    project: cd.project.clone(),
                    old: pd.version.clone(),
                    new: cd.version.clone(),
                })
            }
            Some(_) => {}
        }
    }
    for pd in &prev_deps {
        if !cur_deps.iter().any(|cd| cd.project == pd.project) {
            changes.push(ConfigChange::DependencyRemoved {
                project: pd.project.clone(),
            });
        }
    }

    ConfigChanges(changes)
}

fn spec_version(units: &[FrozenUnit]) -> Option<u8> {
    units.iter().find_map(|u| match u {
        FrozenUnit::SpecificationVersion(v) => Some(*v),
        _ => None,
    })
}

fn dependencies(units: &[FrozenUnit]) -> Vec<&Dependency> {
    units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Dependency(d) => Some(d),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::config::ir::frozen::LanguageDetails;

    fn dep(project: &str, version: &str) -> FrozenUnit {
        FrozenUnit::Dependency(Dependency {
            author: "acme".into(),
            project: project.into(),
            version: version.into(),
        })
    }

    #[test]
    fn empty_previous_reports_nothing() {
        let cur = vec![FrozenUnit::SpecificationVersion(2)];
        assert!(analyze_config_changes(&[], &cur).is_empty());
    }

    #[test]
    fn spec_version_change_is_major() {
        let prev = vec![FrozenUnit::SpecificationVersion(1)];
        let cur = vec![FrozenUnit::SpecificationVersion(2)];
        let changes = analyze_config_changes(&prev, &cur);
        assert_eq!(changes.bump(), VersionBump::Major);
        assert_eq!(
            changes.0,
            vec![ConfigChange::SpecificationVersionChanged { old: 1, new: 2 }]
        );
    }

    #[test]
    fn dependency_added_is_minor_removed_is_major() {
        let prev = vec![dep("std", "1.0.0")];
        let cur = vec![dep("std", "1.0.0"), dep("uuid", "4.1.0")];
        assert_eq!(analyze_config_changes(&prev, &cur).bump(), VersionBump::Minor);
        assert_eq!(analyze_config_changes(&cur, &prev).bump(), VersionBump::Major);
    }

    #[test]
    fn dependency_version_change_is_major() {
        let prev = vec![dep("std", "1.0.0")];
        let cur = vec![dep("std", "2.0.0")];
        let changes = analyze_config_changes(&prev, &cur);
        assert_eq!(changes.bump(), VersionBump::Major);
        assert!(matches!(
            changes.0[0],
            ConfigChange::DependencyVersionChanged { .. }
        ));
    }

    #[test]
    fn a_wire_change_is_major_and_a_written_default_is_none() {
        let wire = |format: Option<&str>, framing: Option<&str>| FrozenUnit::Wire {
            format: format.map(Into::into),
            framing: framing.map(Into::into),
        };
        let base = FrozenUnit::SpecificationVersion(1);
        let plain = vec![base.clone()];

        let explicit = vec![base.clone(), wire(Some("packed@1"), Some("datagram@2"))];
        assert!(analyze_config_changes(&plain, &explicit).is_empty());

        let msgpack = vec![base.clone(), wire(Some("msgpack@1"), None)];
        let changes = analyze_config_changes(&plain, &msgpack);
        assert_eq!(changes.bump(), VersionBump::Major);
        assert_eq!(
            changes.0,
            vec![ConfigChange::WireFormatChanged { old: "packed@1".into(), new: "msgpack@1".into() }]
        );

        let datagram1 = vec![base, wire(None, Some("datagram"))];
        assert_eq!(
            analyze_config_changes(&plain, &datagram1).0,
            vec![ConfigChange::WireFramingChanged { old: "datagram@2".into(), new: "datagram".into() }]
        );
    }

    #[test]
    fn code_generation_is_not_compared() {
        let cg = |name: &str| FrozenUnit::CodeGeneration(LanguageDetails { name: name.into() });
        let prev = vec![FrozenUnit::SpecificationVersion(1), cg("rust#1.70.0")];
        let cur = vec![
            FrozenUnit::SpecificationVersion(1),
            cg("rust#1.70.0"),
            cg("python#3.11.0"),
        ];
        assert!(analyze_config_changes(&prev, &cur).is_empty());
    }
}
