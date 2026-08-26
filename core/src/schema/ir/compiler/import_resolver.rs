// Import resolution for use statements
// Handles resolving imports from same package, stdlib, and external dependencies

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::constants::SCHEMA_EXTENSION;
use crate::schema::idl::grammar::{UsePath, RelativePrefix};
use crate::schema::ir::context::SchemaContext;

/// Resolved import information
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// Absolute namespace of the import
    pub absolute_namespace: Vec<String>,
    
    /// Path to the schema file (if external)
    pub schema_path: Option<PathBuf>,
    
    /// Symbols to import (empty = all)
    pub symbols: Vec<String>,
    
    /// Alias if specified
    pub alias: Option<String>,
}

/// Import resolver - resolves use statements to absolute namespaces
#[derive(Debug)]
pub struct ImportResolver {
    /// Current package namespace (e.g., ["mypackage"])
    package_namespace: Vec<String>,
    
    /// Map of external dependencies: name -> root path
    dependencies: HashMap<String, PathBuf>,
    
    /// Standard library root path
    stdlib_root: Option<PathBuf>,
}

impl ImportResolver {
    /// Create a new import resolver for a package
    pub fn new(
        package_namespace: Vec<String>,
        dependencies: HashMap<String, PathBuf>,
        stdlib_root: Option<PathBuf>,
    ) -> Self {
        Self {
            package_namespace,
            dependencies,
            stdlib_root,
        }
    }
    
    /// Resolve a use path to absolute namespace
    pub fn resolve(
        &self,
        use_path: &UsePath,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        match use_path {
            UsePath::Absolute(scoped) => {
                self.resolve_absolute(scoped.to_string())
            }
            UsePath::Relative(rel) => {
                self.resolve_relative(rel, current_namespace)
            }
            UsePath::Glob(glob) => {
                self.resolve_glob(&glob.path.to_string())
            }
            UsePath::Multi(multi) => {
                let items: Vec<String> = multi.items.iter().map(|i| i.text.clone()).collect();
                self.resolve_multi(&multi.path.to_string(), &items)
            }
        }
    }
    
    /// Resolve absolute path (e.g., std::http::Request or mypackage::types::User)
    fn resolve_absolute(&self, path: String) -> Result<ResolvedImport, String> {
        let parts: Vec<String> = path.split("::").map(|s| s.to_string()).collect();
        
        if parts.is_empty() {
            return Err("Empty import path".to_string());
        }
        
        // Check if it's a stdlib import (std::)
        if parts[0] == "std" {
            return self.resolve_stdlib(&parts);
        }
        
        // Check if it's an external dependency
        if let Some(dep_path) = self.dependencies.get(&parts[0]) {
            let mut schema_path = dep_path.clone();
            for part in &parts[1..] {
                schema_path.push(part);
            }
            schema_path.set_extension(SCHEMA_EXTENSION);

            return Ok(ResolvedImport {
                absolute_namespace: parts,
                schema_path: Some(schema_path),
                symbols: vec![],
                alias: None,
            });
        }
        
        // It's from the same package - already loaded as a SchemaContext in the
        // ProjectContext being compiled, so no schema_path is needed to load it
        // from disk; see `resolve_use_to_schema` below for how it's located.
        Ok(ResolvedImport {
            absolute_namespace: parts,
            schema_path: None,
            symbols: vec![],
            alias: None,
        })
    }
    
    /// Resolve relative path (e.g., parent::common or self::utils)
    fn resolve_relative(
        &self,
        rel: &crate::schema::idl::grammar::RelativePath,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        let mut absolute = current_namespace.to_vec();
        
        match &rel.prefix {
            RelativePrefix::Self_ => {
                // self:: means current namespace
                // Keep absolute as is
            }
            RelativePrefix::Parent => {
                // parent:: means go up one level
                if absolute.is_empty() {
                    return Err("Cannot use parent:: from root namespace".to_string());
                }
                absolute.pop();
            }
            RelativePrefix::Crate => {
                // crate:: means package root
                absolute = self.package_namespace.clone();
            }
        }
        
        // Append the rest of the path
        let path_parts: Vec<String> = rel.path.to_string().split("::").map(|s: &str| s.to_string()).collect();
        absolute.extend(path_parts);
        
        Ok(ResolvedImport {
            absolute_namespace: absolute,
            schema_path: None,
            symbols: vec![],
            alias: None,
        })
    }
    
    /// Resolve stdlib import (e.g., std::http::Request)
    fn resolve_stdlib(&self, parts: &[String]) -> Result<ResolvedImport, String> {
        if let Some(stdlib_root) = &self.stdlib_root {
            // Build path to stdlib schema
            let mut schema_path = stdlib_root.clone();
            for part in &parts[1..] {  // Skip "std"
                schema_path.push(part);
            }
            schema_path.set_extension(SCHEMA_EXTENSION);
            
            Ok(ResolvedImport {
                absolute_namespace: parts.to_vec(),
                schema_path: Some(schema_path),
                symbols: vec![],
                alias: None,
            })
        } else {
            Err("Standard library not configured".to_string())
        }
    }
    
    /// Resolve glob import (e.g., mypackage::types::*)
    fn resolve_glob(&self, path: &str) -> Result<ResolvedImport, String> {
        let parts: Vec<String> = path.split("::").map(|s| s.to_string()).collect();
        
        Ok(ResolvedImport {
            absolute_namespace: parts,
            schema_path: None,
            symbols: vec!["*".to_string()], // Glob marker
            alias: None,
        })
    }
    
    /// Resolve multi-import (e.g., mypackage::{User, Post})
    fn resolve_multi(&self, path: &str, items: &[String]) -> Result<ResolvedImport, String> {
        let parts: Vec<String> = path.split("::").map(|s| s.to_string()).collect();
        
        Ok(ResolvedImport {
            absolute_namespace: parts,
            schema_path: None,
            symbols: items.to_vec(),
            alias: None,
        })
    }
    
    /// Load a schema from a resolved import
    /// Returns the parsed schema document
    pub fn load_schema(
        &self,
        resolved: &ResolvedImport,
    ) -> Result<crate::schema::idl::grammar::Document, String> {
        // If we have a schema_path, load from filesystem
        if let Some(path) = &resolved.schema_path {
            let source = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            
            match crate::schema::idl::grammar::parse(&source) {
                Ok(doc) => Ok(doc),
                Err(errors) => {
                    // Print beautiful diagnostics for each error
                    eprintln!("\n{} Errors parsing schema {}:", errors.len(), path.display());
                    for error in &errors {
                        crate::schema::idl::diagnostics::print_parse_error(
                            error,
                            &source,
                            &path.to_string_lossy(),
                        );
                    }
                    Err(format!("Parse failed with {} error(s)", errors.len()))
                }
            }
        } else {
            // Same-package imports have no schema_path because the schema is
            // already loaded into the compiling ProjectContext's schema_contexts
            // (see `resolve_use_to_schema`) - there's nothing to load from disk.
            Err(format!(
                "Cannot load schema for {:?} - it should already be part of the \
                 current package's ProjectContext",
                resolved.absolute_namespace
            ))
        }
    }
}

/// Result of resolving a `use`/`import` declaration against a [`ProjectContext`].
pub struct ResolvedUseTarget {
    /// The schema that declares the imported namespace/symbol, if it's part of
    /// the same project (as opposed to an external dependency or stdlib).
    pub schema: Option<Rc<RefCell<SchemaContext>>>,
    /// Remaining path segments after the matched schema's namespace - empty for
    /// a whole-schema import, or the imported symbol's name/path otherwise.
    pub remaining: Vec<String>,
    /// The raw resolution result (absolute namespace, schema_path, symbols, alias).
    pub resolved: ResolvedImport,
}

/// Resolve a single `use` path against a project: locate the schema it points to
/// (if it's part of the same project) and figure out what's left over (a symbol
/// name, for item imports).
///
/// Shared by cross-file import-cycle detection and by the IR compiler so both
/// use the exact same resolution logic.
pub fn resolve_use_to_schema(
    project_context: &ProjectContext,
    resolver: &ImportResolver,
    current_namespace: &[String],
    use_path: &UsePath,
) -> Result<ResolvedUseTarget, String> {
    let resolved = resolver.resolve(use_path, current_namespace)?;

    // Longest-prefix match against known schema namespaces: this naturally
    // handles both whole-schema imports (`use pkg::types;`) and single-symbol
    // imports (`use pkg::types::User;`), leaving `User` as the remainder.
    let (schema, remaining) = match project_context
        .find_schema_by_import_namespace_parts(&resolved.absolute_namespace)
    {
        Some((schema, remaining)) => (Some(schema), remaining),
        None => (None, vec![]),
    };

    Ok(ResolvedUseTarget { schema, remaining, resolved })
}

/// Whether a schema declares a top-level symbol (struct/enum/protocol/const) by name.
pub fn schema_declares_symbol(schema_context: &SchemaContext, symbol: &str) -> bool {
    use crate::schema::idl::grammar::Declaration;

    schema_context.declarations.iter().any(|decl| match decl {
        Declaration::Struct(s) => s.name.text == symbol,
        Declaration::Enum(e) => e.name.text == symbol,
        Declaration::Protocol(p) => p.name.text == symbol,
        Declaration::Const(c) => c.name.text == symbol,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resolve_parent() {
        let resolver = ImportResolver::new(
            vec!["mypackage".to_string()],
            HashMap::new(),
            None,
        );
        
        // parent:: from mypackage::users::models should go to mypackage::users
        let current_ns = vec!["mypackage".to_string(), "users".to_string(), "models".to_string()];
        
        // This test would need actual UsePath struct
        // Just demonstrating the concept
    }
}
