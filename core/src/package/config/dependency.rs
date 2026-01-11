// Dependency configuration structures
// Parsed from congregation config dependencies block

use std::path::PathBuf;
use std::collections::HashMap;

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
    Path {
        path: PathBuf,
    },
}

impl DependencyConfig {
    /// Parse dependency config from grammar Dictionary value
    pub fn from_dict(
        name: String,
        dict: &crate::package::config::idl::grammar::Dictionary,
    ) -> Result<Self, String> {
        use crate::package::config::idl::grammar::{Assignment, Key, Value};
        
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
            DependencySource::Path { path }
        } else if let Some(commit) = commit {
            // Git source
            DependencySource::Git {
                version: version.ok_or("Git dependency missing version")?,
                uri: uri.ok_or("Git dependency missing uri")?,
                commit,
                hash,
            }
        } else {
            // Registry source
            DependencySource::Registry {
                version: version.ok_or("Registry dependency missing version")?,
                uri: uri.ok_or("Registry dependency missing uri")?,
                hash,
                signature,
            }
        };
        
        Ok(DependencyConfig { name, source })
    }
    
    /// Parse all dependencies from congregation config
    pub fn parse_dependencies(
        assignments: &[crate::package::config::idl::grammar::Assignment],
    ) -> HashMap<String, DependencyConfig> {
        use crate::package::config::idl::grammar::{Key, Value};
        
        let mut deps = HashMap::new();
        
        for assignment in assignments {
            // Look for "dependencies" key
            if let Key::Identifier(id) = &assignment.key {
                if id.value == "dependencies" {
                    // Value should be a dictionary
                    if let Value::Dictionary(dict) = &assignment.value {
                        // Each assignment in the dict is a dependency
                        for dep_assignment in &dict.assignments {
                            if let Key::Identifier(dep_name) = &dep_assignment.key {
                                if let Value::Dictionary(dep_dict) = &dep_assignment.value {
                                    match DependencyConfig::from_dict(
                                        dep_name.value.clone(),
                                        dep_dict,
                                    ) {
                                        Ok(dep) => {
                                            deps.insert(dep.name.clone(), dep);
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Failed to parse dependency {}: {}",
                                                dep_name.value,
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        deps
    }
}

/// Strip leading and trailing quotes from a string
fn strip_quotes(s: &str) -> String {
    s.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_dependencies() {
        // This would test with actual parsed grammar
        // For now, just a placeholder
    }
}
