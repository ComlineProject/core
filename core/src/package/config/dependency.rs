// Dependency configuration structures
// Parsed from congregation config dependencies block

use std::collections::HashMap;
use std::path::PathBuf;

/// Dependency configuration from config.idp
#[derive(Debug, Clone)]
pub struct DependencyConfig {
    pub name: String,
    pub source: DependencySource,
}

/// Source of a dependency
#[derive(Debug, Clone)]
pub enum DependencySource {
    /// Registry with version and URI
    Registry {
        version: String,
        uri: String,
        hash: Option<String>,
        signature: Option<String>,
    },
    /// Git repository with commit hash
    Git {
        version: String,
        uri: String,
        commit: String,
        hash: Option<String>,
    },
    /// Local filesystem path
    Path { path: PathBuf, hash: Option<String> },
}

impl DependencyConfig {
    /// The version declared in `config.idp`, when the source carries one.
    /// `Path` dependencies have no declared version — their effective version
    /// is whatever their own last build produced, known only after resolving
    /// them (see `package::deps::resolve`).
    pub fn declared_version(&self) -> Option<&str> {
        match &self.source {
            DependencySource::Registry { version, .. } => Some(version),
            DependencySource::Git { version, .. } => Some(version),
            DependencySource::Path { .. } => None,
        }
    }

    /// The declared integrity hash, if any. A `Path` dependency can carry one
    /// too — its content isn't immutable the way a pinned commit is, so a
    /// hash is the only thing that catches "the sibling package changed
    /// without the pin being updated."
    pub fn declared_hash(&self) -> Option<&str> {
        match &self.source {
            DependencySource::Registry { hash, .. } => hash.as_deref(),
            DependencySource::Git { hash, .. } => hash.as_deref(),
            DependencySource::Path { hash, .. } => hash.as_deref(),
        }
    }

    /// Best-effort "author" for the frozen `Dependency` unit's diffing
    /// identity — the org/user segment of a `Git`/`Registry` URI
    /// (`https://github.com/acme/std` → `"acme"`), falling back to the whole
    /// URI when that shape doesn't apply. A `Path` dependency has no natural
    /// author; it resolves to `"local"`.
    pub fn author(&self) -> String {
        match &self.source {
            DependencySource::Path { .. } => "local".to_string(),
            DependencySource::Git { uri, .. } | DependencySource::Registry { uri, .. } => {
                let trimmed = uri.trim_end_matches('/');
                let segments: Vec<&str> = trimmed.rsplitn(3, '/').collect();
                // rsplitn(3, '/') on ".../acme/std" yields ["std", "acme", "..."]
                match segments.as_slice() {
                    [_repo, org, _rest] if !org.is_empty() => org.to_string(),
                    _ => uri.clone(),
                }
            }
        }
    }

    /// Parse dependency config from grammar Dictionary value
    pub fn from_dict(
        name: String,
        dict: &crate::package::config::idl::grammar::Dictionary,
    ) -> Result<Self, String> {
        use crate::package::config::idl::grammar::{Key, Value};

        let mut version = None;
        let mut uri = None;
        let mut hash = None;
        let mut commit = None;
        let mut path = None;
        let mut signature = None;

        // Parse assignments in the dictionary
        for assignment in &dict.assignments {
            let key_str = match &assignment.key {
                Key::Identifier(id) => id.value.clone(),
                _ => continue,
            };

            match key_str.as_str() {
                "version" => {
                    if let Value::String(s) = &assignment.value {
                        version = Some(strip_quotes(&s.value));
                    }
                }
                "uri" => {
                    if let Value::String(s) = &assignment.value {
                        uri = Some(strip_quotes(&s.value));
                    }
                }
                "hash" => {
                    if let Value::String(s) = &assignment.value {
                        hash = Some(strip_quotes(&s.value));
                    }
                }
                "commit" => {
                    if let Value::String(s) = &assignment.value {
                        commit = Some(strip_quotes(&s.value));
                    }
                }
                "path" => {
                    if let Value::String(s) = &assignment.value {
                        path = Some(PathBuf::from(strip_quotes(&s.value)));
                    }
                }
                "signature" => {
                    if let Value::String(s) = &assignment.value {
                        signature = Some(strip_quotes(&s.value));
                    }
                }
                _ => {}
            }
        }

        // Determine source type based on available fields
        let source = if let Some(path) = path {
            DependencySource::Path { path, hash }
        } else if let Some(commit) = commit {
            // Git source
            DependencySource::Git {
                version: version
                    .ok_or_else(|| format!("dependency '{name}': git source missing 'version'"))?,
                uri: uri.ok_or_else(|| format!("dependency '{name}': git source missing 'uri'"))?,
                commit,
                hash,
            }
        } else {
            // Registry source
            DependencySource::Registry {
                version: version.ok_or_else(|| {
                    format!("dependency '{name}': registry source missing 'version'")
                })?,
                uri: uri
                    .ok_or_else(|| format!("dependency '{name}': registry source missing 'uri'"))?,
                hash,
                signature,
            }
        };

        Ok(DependencyConfig { name, source })
    }

    /// Parse a `dependencies = { name = {...}, ... }` dictionary's *inner*
    /// assignments (one `name = {...}` per dependency) into a name → config
    /// map. Errors on the first malformed declaration — a `dependencies`
    /// block is a schema input like any other; a typo in it should fail the
    /// build the same way a malformed `code_generation` block does, not
    /// silently disappear.
    pub fn parse_dict(
        dict: &crate::package::config::idl::grammar::Dictionary,
    ) -> Result<HashMap<String, DependencyConfig>, String> {
        use crate::package::config::idl::grammar::{Key, Value};

        let mut deps = HashMap::new();

        for dep_assignment in &dict.assignments {
            let Key::Identifier(dep_name) = &dep_assignment.key else {
                return Err("dependency names must be plain identifiers".to_string());
            };
            let Value::Dictionary(dep_dict) = &dep_assignment.value else {
                return Err(format!(
                    "dependency '{}' must be a dictionary",
                    dep_name.value
                ));
            };

            let dep = DependencyConfig::from_dict(dep_name.value.clone(), dep_dict)?;
            deps.insert(dep.name.clone(), dep);
        }

        Ok(deps)
    }

    /// Find the top-level `dependencies = {...}` assignment in a
    /// congregation's assignments (if any) and parse it via [`parse_dict`].
    pub fn parse_dependencies(
        assignments: &[crate::package::config::idl::grammar::Assignment],
    ) -> Result<HashMap<String, DependencyConfig>, String> {
        use crate::package::config::idl::grammar::{Key, Value};

        for assignment in assignments {
            if let Key::Identifier(id) = &assignment.key {
                if id.value == "dependencies" {
                    let Value::Dictionary(dict) = &assignment.value else {
                        return Err("'dependencies' must be a dictionary".to_string());
                    };
                    return DependencyConfig::parse_dict(dict);
                }
            }
        }

        Ok(HashMap::new())
    }
}

/// Strip leading and trailing quotes from a string
fn strip_quotes(s: &str) -> String {
    s.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::config::idl::grammar;

    fn parse_deps(source: &str) -> HashMap<String, DependencyConfig> {
        let congregation = grammar::parse(source).expect("parses");
        DependencyConfig::parse_dependencies(&congregation.assignments).expect("valid deps")
    }

    #[test]
    fn parses_a_path_dependency() {
        let deps = parse_deps(
            r#"congregation my_api
            specification_version = 1
            dependencies = {
                shared_types = {
                    path = "../shared-types"
                }
            }
            "#,
        );
        let dep = deps.get("shared_types").expect("present");
        assert_eq!(dep.declared_version(), None);
        assert_eq!(dep.author(), "local");
        match &dep.source {
            DependencySource::Path { path, hash } => {
                assert_eq!(path, &PathBuf::from("../shared-types"));
                assert_eq!(hash, &None);
            }
            other => panic!("expected Path, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_git_dependency() {
        let deps = parse_deps(
            r#"congregation my_api
            specification_version = 1
            dependencies = {
                std = {
                    version = "1.0.0"
                    uri = "https://github.com/acme/std"
                    commit = "abc123"
                    hash = "blake3:deadbeef"
                }
            }
            "#,
        );
        let dep = deps.get("std").expect("present");
        assert_eq!(dep.declared_version(), Some("1.0.0"));
        assert_eq!(dep.declared_hash(), Some("blake3:deadbeef"));
        assert_eq!(dep.author(), "acme");
    }

    #[test]
    fn rejects_a_malformed_dependency_instead_of_dropping_it() {
        let congregation = grammar::parse(
            r#"congregation my_api
            specification_version = 1
            dependencies = {
                std = {
                    uri = "https://github.com/acme/std"
                }
            }
            "#,
        )
        .expect("parses");
        let result = DependencyConfig::parse_dependencies(&congregation.assignments);
        assert!(
            result.is_err(),
            "missing version+commit should error, not warn-and-drop"
        );
    }
}
