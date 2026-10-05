// Dependency configuration structures
// Parsed from congregation config dependencies block

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::package::build::cas::storage::Hash;

/// Where a package's `Git` dependencies get checked out, relative to the
/// package's own directory.
pub const DEPS_CACHE_DIR: &str = ".comline/deps-cache";

/// A git pin's checkout directory inside `cache_dir`, keyed by `uri` +
/// `commit` so distinct pins never collide.
pub fn git_checkout_dir(cache_dir: &Path, uri: &str, commit: &str) -> PathBuf {
    cache_dir.join(Hash::from_bytes(format!("{uri}#{commit}").as_bytes()).to_hex())
}

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

/// Why `name` can't be used as a dependency's local alias - the name a
/// consuming package's own `use` statements address it by - or `None` if
/// it's fine. Two reasons: it's `self`/`parent`/`package`, a `use`-path
/// prefix keyword (`use self::...` means "this schema," never "the
/// dependency named self"), or it's `std`, the embedded standard library's
/// own reserved name (every package can already `use std::...` with no
/// `dependencies` entry at all - see `package::stdlib` - so a real
/// dependency aliased to `std` would silently shadow it instead of adding
/// a second meaning).
fn reserved_dependency_name(name: &str) -> Option<&'static str> {
    if name == crate::package::stdlib::NAMESPACE {
        return Some("it's the embedded standard library's own name (every package can already `use std::...`)");
    }
    if crate::schema::idl::vocabulary::keyword(name)
        .is_some_and(|k| k.kind == crate::schema::idl::vocabulary::KeywordKind::PathPrefix)
    {
        return Some("it's a `use`-path prefix keyword (self/parent/package)");
    }
    None
}

impl DependencyConfig {
    /// Where this dependency's package lives once resolved, for a consuming
    /// package at `package_root`: a `Path` dependency's own directory, or a
    /// `Git` pin's checkout in the deps cache
    /// (`<package_root>/.comline/deps-cache/<key>`), fetched yet or not.
    /// `None` for a `Registry` source, which nothing resolves yet.
    ///
    /// Pure: it only says where, `package::deps` does the fetching. Not behind
    /// the `deps` feature, so the language server finds a fetched pin exactly
    /// where `comline check` put it.
    pub fn package_dir(&self, package_root: &Path) -> Option<PathBuf> {
        match &self.source {
            DependencySource::Path { path, .. } => Some(package_root.join(path)),
            DependencySource::Git { uri, commit, .. } => {
                Some(git_checkout_dir(&package_root.join(DEPS_CACHE_DIR), uri, commit))
            }
            DependencySource::Registry { .. } => None,
        }
    }

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

        if let Some(reason) = reserved_dependency_name(&name) {
            return Err(format!("dependency name '{name}' is reserved: {reason}"));
        }

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
                acme_lib = {
                    version = "1.0.0"
                    uri = "https://github.com/acme/std"
                    commit = "abc123"
                    hash = "blake3:deadbeef"
                }
            }
            "#,
        );
        let dep = deps.get("acme_lib").expect("present");
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
                acme_lib = {
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

    #[test]
    fn rejects_self_parent_package_and_std_as_a_dependency_name() {
        for reserved in ["self", "parent", "package", "std"] {
            let congregation = grammar::parse(&format!(
                r#"congregation my_api
                specification_version = 1
                dependencies = {{
                    {reserved} = {{
                        path = "../whatever"
                    }}
                }}
                "#,
            ))
            .expect("parses");
            let result = DependencyConfig::parse_dependencies(&congregation.assignments);
            let error = result.unwrap_err();
            assert!(error.contains(reserved), "{reserved}: {error}");
            assert!(error.contains("reserved"), "{reserved}: {error}");
        }
    }

    #[test]
    fn an_ordinary_name_is_not_reserved() {
        assert!(reserved_dependency_name("shared_types").is_none());
        assert!(reserved_dependency_name("standard").is_none(), "only the exact word std is reserved");
    }

    #[test]
    fn package_dir_for_each_source() {
        let root = Path::new("/work/consumer");
        let deps = parse_deps(
            r#"congregation consumer
            specification_version = 1
            dependencies = {
                local = {
                    path = "../shared-types"
                }
                pinned = {
                    version = "1.0.0"
                    uri = "https://example.test/acme/net"
                    commit = "4f2c9e1"
                }
                hosted = {
                    version = "1.0.0"
                    uri = "comline://registry.example.test/std"
                }
            }"#,
        );

        assert_eq!(deps["local"].package_dir(root), Some(root.join("../shared-types")));

        let pinned = deps["pinned"].package_dir(root).expect("a git pin has a checkout dir");
        assert_eq!(pinned.parent(), Some(root.join(DEPS_CACHE_DIR).as_path()));
        assert_eq!(
            pinned,
            git_checkout_dir(&root.join(DEPS_CACHE_DIR), "https://example.test/acme/net", "4f2c9e1")
        );
        assert_eq!(pinned.file_name().unwrap().len(), 64, "a blake3 hex key");

        assert_eq!(deps["hosted"].package_dir(root), None);
    }
}

