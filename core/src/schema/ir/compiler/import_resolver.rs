// Import resolution for use statements
// Handles resolving imports from same package, stdlib, and external dependencies

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::package::config::dependency::DependencyConfig;
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::constants::SCHEMA_EXTENSION;
use crate::schema::idl::grammar::{Declaration, UsePath, RelativePrefix};
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::validation::validator::closest;
use crate::schema::ir::validation::ValidationError;

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
                self.resolve_absolute(scoped.to_string(), current_namespace)
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

    /// The pure subset of [`resolve`](Self::resolve): just the namespace a
    /// `use` path points at, with no `schema_path`/dependency decoration -
    /// callable with no `ProjectContext` at all, unlike `resolve_use_to_schema`.
    /// The language server uses this directly (it never loads a full
    /// `ProjectContext` for an open file).
    ///
    /// Differs from `resolve` in exactly one case: an absolute `std::`-rooted
    /// path still resolves here even with no `stdlib_root` configured (where
    /// `resolve` hard-errors) - a strictly better contract for a caller that
    /// only wants the namespace, not a schema it can load from disk.
    ///
    /// `ImportResolver` lives under `schema::ir::compiler`, a tree that
    /// otherwise means "needs a `ProjectContext`"; this method is the one
    /// exception, not a sign the type should move.
    pub fn resolve_namespace(
        &self,
        use_path: &UsePath,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        match use_path {
            UsePath::Absolute(scoped) => {
                let parts = Self::split_absolute(&scoped.to_string())?;
                match self.try_resolve_relative_prefix(&parts, current_namespace) {
                    Some(result) => result,
                    None => Ok(ResolvedImport {
                        absolute_namespace: parts,
                        schema_path: None,
                        symbols: vec![],
                        alias: None,
                    }),
                }
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

    /// Split a `::`-joined absolute path into its segments.
    fn split_absolute(path: &str) -> Result<Vec<String>, String> {
        let parts: Vec<String> = path.split("::").map(|s| s.to_string()).collect();

        if parts.is_empty() {
            return Err("Empty import path".to_string());
        }

        Ok(parts)
    }

    /// If `parts`' first segment is `self`/`parent`/`package`, resolve it
    /// as such against `current_namespace`; `None` for a genuine absolute
    /// path, leaving the caller to decide what to do with it.
    ///
    /// This check belongs here, not in the grammar, because `self::`/
    /// `parent::`/`package::` text parses as `UsePath::Absolute`, never
    /// `UsePath::Relative`: `RelativePrefix`'s short keyword leaves always
    /// lose the lexer's longest-match tie-break against `ScopedIdentifier`'s
    /// own regex, which greedily matches `self::Profile` as one token.
    /// Fixing that in the grammar needs restructuring `ScopedIdentifier`'s
    /// shape, which (prototyped, then reverted) breaks `GlobPath`'s
    /// trailing `::*` and `MultiPath`'s trailing `::{...}` in ways that
    /// didn't resolve cleanly - too invasive for the actual payoff here.
    /// This resolver-level check gets the same correct resolution without
    /// touching the grammar at all.
    fn try_resolve_relative_prefix(
        &self,
        parts: &[String],
        current_namespace: &[String],
    ) -> Option<Result<ResolvedImport, String>> {
        let prefix = match parts[0].as_str() {
            "self" => RelativePrefix::Self_,
            "parent" => RelativePrefix::Parent,
            "package" => RelativePrefix::Package,
            _ => return None,
        };
        Some(self.resolve_relative_prefix(&prefix, &parts[1..], current_namespace))
    }

    /// Resolve absolute path (e.g., std::http::Request or mypackage::types::User)
    fn resolve_absolute(
        &self,
        path: String,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        let parts = Self::split_absolute(&path)?;

        if let Some(result) = self.try_resolve_relative_prefix(&parts, current_namespace) {
            return result;
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
    
    /// Resolve relative path (e.g., parent::common or self::utils). Dead
    /// through real parsing today - see `try_resolve_relative_prefix`'s
    /// doc - but kept because `RelativePath`/`RelativePrefix` are still
    /// real grammar productions, and because the actual resolution logic
    /// lives in the shared `resolve_relative_prefix`, not here.
    fn resolve_relative(
        &self,
        rel: &crate::schema::idl::grammar::RelativePath,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        let rest: Vec<String> =
            rel.path.to_string().split("::").map(|s: &str| s.to_string()).collect();
        self.resolve_relative_prefix(&rel.prefix, &rest, current_namespace)
    }

    /// Resolve `prefix`'s meaning (current namespace / one level up / the
    /// package root) plus the remaining path segments against
    /// `current_namespace`. The one place this logic lives - both
    /// `resolve_relative` (the grammar's own, currently-unreachable
    /// `RelativePath`) and `try_resolve_relative_prefix` (what
    /// `self::`/`parent::`/`package::` actually parses as today) resolve
    /// through here, so they can never disagree.
    fn resolve_relative_prefix(
        &self,
        prefix: &RelativePrefix,
        rest: &[String],
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        let mut absolute = current_namespace.to_vec();

        match prefix {
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
            RelativePrefix::Package => {
                // package:: means the package root
                absolute = self.package_namespace.clone();
            }
        }

        absolute.extend(rest.iter().cloned());

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

/// Whether a schema declares a top-level symbol
/// (struct/enum/protocol/const/type-alias) by name.
pub fn schema_declares_symbol(schema_context: &SchemaContext, symbol: &str) -> bool {
    schema_context.declarations.iter().any(|decl| match &decl.value {
        Declaration::Struct(s) => s.name.text == symbol,
        Declaration::Enum(e) => e.name.text == symbol,
        Declaration::Protocol(p) => p.name.text == symbol,
        Declaration::Const(c) => c.name.text == symbol,
        Declaration::TypeAlias(t) => t.name.text == symbol,
        _ => false,
    })
}

/// All top-level symbol (struct/enum/protocol/const/type-alias) names a
/// schema declares. Used to expand whole-namespace/glob `use` imports into
/// per-symbol `FrozenUnit::Import`s, so field types written as `ns::Symbol`
/// after such an import can actually be found by validation.
pub fn declared_symbol_names(schema_context: &SchemaContext) -> Vec<String> {
    schema_context
        .declarations
        .iter()
        .filter_map(|decl| match &decl.value {
            Declaration::Struct(s) => Some(s.name.text.clone()),
            Declaration::Enum(e) => Some(e.name.text.clone()),
            Declaration::Protocol(p) => Some(p.name.text.clone()),
            Declaration::Const(c) => Some(c.name.text.clone()),
            Declaration::TypeAlias(t) => Some(t.name.text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether a `use` resolving to `resolved` (with `remaining` path segments
/// left over after the longest-prefix schema match, and an optional `as`
/// `alias`) brings the bare name `name` into scope. The branch order is
/// load-bearing - glob, then explicit multi-import symbols, then a
/// whole-namespace `use`, then the remaining path/alias - and is the only
/// correct statement of this rule; shared by [`find_schema_bringing_into_scope`]
/// below and by the language server's own `use`-scoped symbol resolution,
/// so neither has to re-derive it by hand.
pub fn use_brings_into_scope(
    resolved: &ResolvedImport,
    remaining: &[String],
    alias: Option<&str>,
    name: &str,
) -> bool {
    if resolved.symbols == ["*".to_string()] {
        true
    } else if !resolved.symbols.is_empty() {
        resolved.symbols.iter().any(|s| s == name)
    } else if remaining.is_empty() {
        // `use ns;` - the whole namespace; a bare name resolves if `ns`
        // declares it.
        true
    } else {
        let symbol = remaining.join("::");
        symbol == name || alias == Some(name)
    }
}

/// Walk `declarations`' `use` statements for one that brings `name` into
/// scope, returning the schema it resolves to (if part of this project).
/// Shared by `resolve_foreign_error` (`incremental.rs`, for `! Name`
/// throws) and the alias-resolution pass (`alias_resolution.rs`) - both
/// need to answer "does some `use` here bring a bare name into scope, and
/// if so from which schema."
pub fn find_schema_bringing_into_scope(
    name: &str,
    declarations: &[rust_sitter::Spanned<Declaration>],
    current_namespace: &[String],
    project_context: &ProjectContext,
) -> Option<Rc<RefCell<SchemaContext>>> {
    let resolver = ImportResolver::new(vec![], Default::default(), None);

    for decl in declarations {
        let Declaration::Use(use_stmt) = &decl.value else {
            continue;
        };
        let Ok(target) =
            resolve_use_to_schema(project_context, &resolver, current_namespace, &use_stmt.path)
        else {
            continue;
        };
        let Some(schema) = &target.schema else {
            continue;
        };

        let alias = use_stmt.alias.as_ref().map(|a| a.name.text.as_str());
        let brings_into_scope =
            use_brings_into_scope(&target.resolved, &target.remaining, alias, name);

        if brings_into_scope {
            return Some(Rc::clone(schema));
        }
    }

    None
}

/// The first segment of a `use` path as written (`std` in `use std::x::Y`).
fn use_path_root(path: &UsePath) -> Option<&str> {
    let text = match path {
        UsePath::Absolute(scoped) => &scoped.text,
        UsePath::Glob(glob) => &glob.path.text,
        UsePath::Multi(multi) => &multi.path.text,
        UsePath::Relative(_) => return None,
    };
    text.split("::").next()
}

/// Every name a `use` can import from a schema: the type-level symbols
/// ([`declared_symbol_names`]) plus errors (`! Name` throws), validators and
/// settings.
fn importable_names(schema_context: &SchemaContext) -> Vec<String> {
    let mut names = declared_symbol_names(schema_context);
    names.extend(schema_context.declarations.iter().filter_map(|decl| match &decl.value {
        Declaration::Error(e) => Some(e.name.text.clone()),
        Declaration::Validator(v) => Some(v.name.text.clone()),
        Declaration::Settings(s) => Some(s.name.text.clone()),
        _ => None,
    }));
    names
}

/// Every `use` in `declarations` that doesn't resolve, as validation
/// errors: a path no schema of the project matches (its own, or a
/// dependency's, merged under the dependency's name), or a named item the
/// matched schema doesn't declare. Run by the real build (`interpret_context`)
/// before a schema is compiled, so `comline build` / `check` reject the import
/// instead of trusting it.
///
/// Passes, because there is nothing to check it against:
/// - a `std::` path: std schemas aren't part of a build yet (and with no
///   stdlib configured, the resolver rejects the path outright);
/// - a declared dependency with no schemas in this project: a build without
///   the `deps` feature never merges them;
/// - every `use` in a dependency's own schemas (`current_namespace` under a
///   dependency's name). Those resolve against the dependency itself, and were
///   checked when it was compiled on its own during resolution.
pub fn check_imports(
    declarations: &[rust_sitter::Spanned<Declaration>],
    current_namespace: &[String],
    project_context: &ProjectContext,
) -> Result<(), Vec<ValidationError>> {
    let dependencies: HashSet<String> =
        DependencyConfig::parse_dependencies(&project_context.config.assignments)
            .map(|deps| deps.into_keys().collect())
            .unwrap_or_default();

    if current_namespace.first().is_some_and(|first| dependencies.contains(first)) {
        return Ok(());
    }

    // The first namespace segment of every schema in the project.
    let top_level: BTreeSet<String> = project_context
        .schema_contexts
        .iter()
        .filter_map(|schema| schema.borrow().namespace.first().cloned())
        .collect();

    let resolver = ImportResolver::new(vec![], Default::default(), None);
    let mut errors = Vec::new();

    for decl in declarations {
        let Declaration::Use(use_stmt) = &decl.value else {
            continue;
        };
        if use_path_root(&use_stmt.path) == Some("std") {
            continue;
        }
        let unresolved = |context: String| ValidationError {
            message: "Unresolved import".to_string(),
            context,
            span: Some(decl.span),
        };

        let target = match resolve_use_to_schema(project_context, &resolver, current_namespace, &use_stmt.path) {
            Ok(target) => target,
            Err(message) => {
                errors.push(unresolved(message));
                continue;
            }
        };
        let first = target.resolved.absolute_namespace.first().map(String::as_str).unwrap_or_default();

        let Some(schema) = &target.schema else {
            if dependencies.contains(first) && !top_level.contains(first) {
                continue;
            }

            let mut context = format!(
                "no schema in this package or its dependencies matches '{}'",
                target.resolved.absolute_namespace.join("::")
            );
            if !top_level.contains(first) && !dependencies.contains(first) {
                let known = top_level.iter().chain(&dependencies).map(String::as_str);
                if let Some(suggestion) = closest(first, known) {
                    context.push_str(&format!(" - did you mean '{suggestion}'?"));
                }
            }
            errors.push(unresolved(context));
            continue;
        };

        let schema = schema.borrow();
        let named: Vec<String> = if target.resolved.symbols == ["*"] {
            vec![]
        } else if !target.resolved.symbols.is_empty() {
            target.resolved.symbols.clone()
        } else if target.remaining.is_empty() {
            vec![]
        } else {
            vec![target.remaining.join("::")]
        };

        let declared = importable_names(&schema);
        for name in named.iter().filter(|name| !declared.contains(name)) {
            let mut context = format!("schema '{}' doesn't declare '{name}'", schema.namespace_joined());
            if let Some(suggestion) = closest(name, declared.iter().map(String::as_str)) {
                context.push_str(&format!(" - did you mean '{suggestion}'?"));
            }
            errors.push(unresolved(context));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::idl::grammar;

    fn parse_use_path(source: &str) -> UsePath {
        let document = grammar::parse(source).expect("use statement should parse");
        let Declaration::Use(use_stmt) = &document.0[0].value else {
            panic!("expected a Use declaration, got {:?}", document.0[0]);
        };
        use_stmt.path.clone()
    }

    #[test]
    fn test_resolve_parent() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        // parent:: from mypackage::users::models should go to mypackage::users
        let current_ns =
            vec!["mypackage".to_string(), "users".to_string(), "models".to_string()];

        let path = parse_use_path("use parent::common");
        let resolved = resolver
            .resolve(&path, &current_ns)
            .expect("parent:: should resolve");

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "users".to_string(), "common".to_string()]
        );
    }

    #[test]
    fn resolve_self_via_resolve() {
        // `self::Profile` parses as `UsePath::Absolute` (see
        // `try_resolve_relative_prefix`'s doc), not `UsePath::Relative` -
        // this exercises the real compiler entry point (`resolve`, not
        // `resolve_namespace`) end to end.
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns = vec!["mypackage".to_string(), "users".to_string()];

        let path = parse_use_path("use self::Profile");
        let resolved = resolver.resolve(&path, &current_ns).expect("self:: should resolve");

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "users".to_string(), "Profile".to_string()]
        );
    }

    #[test]
    fn resolve_package_via_resolve() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns =
            vec!["mypackage".to_string(), "deeply".to_string(), "nested".to_string()];

        let path = parse_use_path("use package::types::User");
        let resolved = resolver.resolve(&path, &current_ns).expect("package:: should resolve");

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "types".to_string(), "User".to_string()]
        );
    }

    #[test]
    fn resolve_parent_from_root_namespace_errors() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let path = parse_use_path("use parent::common");
        assert!(resolver.resolve(&path, &[]).is_err());
    }

    #[test]
    fn resolve_namespace_self() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns = vec!["mypackage".to_string(), "users".to_string()];

        let path = parse_use_path("use self::Profile");
        let resolved = resolver.resolve_namespace(&path, &current_ns).unwrap();

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "users".to_string(), "Profile".to_string()]
        );
    }

    #[test]
    fn resolve_namespace_parent() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns =
            vec!["mypackage".to_string(), "users".to_string(), "models".to_string()];

        let path = parse_use_path("use parent::common");
        let resolved = resolver.resolve_namespace(&path, &current_ns).unwrap();

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "users".to_string(), "common".to_string()]
        );
    }

    #[test]
    fn resolve_namespace_package() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns =
            vec!["mypackage".to_string(), "deeply".to_string(), "nested".to_string()];

        let path = parse_use_path("use package::types::User");
        let resolved = resolver.resolve_namespace(&path, &current_ns).unwrap();

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "types".to_string(), "User".to_string()]
        );
    }

    #[test]
    fn resolve_namespace_absolute() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);

        let path = parse_use_path("use mypackage::types::User");
        let resolved = resolver.resolve_namespace(&path, &[]).unwrap();

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "types".to_string(), "User".to_string()]
        );
        assert_eq!(resolved.schema_path, None);
    }

    #[test]
    fn resolve_namespace_glob() {
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);

        let path = parse_use_path("use mypackage::types::*");
        let resolved = resolver.resolve_namespace(&path, &[]).unwrap();

        assert_eq!(
            resolved.absolute_namespace,
            vec!["mypackage".to_string(), "types".to_string()]
        );
        assert_eq!(resolved.symbols, vec!["*".to_string()]);
    }

    #[test]
    fn resolve_namespace_multi() {
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);

        let path = parse_use_path("use mypackage::{User, Post}");
        let resolved = resolver.resolve_namespace(&path, &[]).unwrap();

        assert_eq!(resolved.absolute_namespace, vec!["mypackage".to_string()]);
        assert_eq!(resolved.symbols, vec!["User".to_string(), "Post".to_string()]);
    }

    #[test]
    fn resolve_namespace_std_without_stdlib_root_still_resolves() {
        // Unlike `resolve`, which hard-errors here (no stdlib_root
        // configured), `resolve_namespace` only needs the namespace - it
        // never builds a schema_path, so it has nothing to be missing.
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);

        let path = parse_use_path("use std::collections::HashMap");
        let resolved = resolver
            .resolve_namespace(&path, &[])
            .expect("resolve_namespace should not need stdlib_root");

        assert_eq!(
            resolved.absolute_namespace,
            vec!["std".to_string(), "collections".to_string(), "HashMap".to_string()]
        );
        assert_eq!(resolved.schema_path, None);
    }

    #[test]
    fn resolve_still_errors_on_std_without_stdlib_root() {
        // `resolve_namespace`'s improved std:: contract is additive - the
        // real compiler path through `resolve` is unchanged.
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);
        let path = parse_use_path("use std::collections::HashMap");
        assert!(resolver.resolve(&path, &[]).is_err());
    }

    fn resolved_with_symbols(symbols: Vec<&str>) -> ResolvedImport {
        ResolvedImport {
            absolute_namespace: vec![],
            schema_path: None,
            symbols: symbols.into_iter().map(String::from).collect(),
            alias: None,
        }
    }

    #[test]
    fn use_brings_into_scope_glob_matches_anything() {
        let r = resolved_with_symbols(vec!["*"]);
        assert!(use_brings_into_scope(&r, &["Unrelated".to_string()], None, "AnyName"));
    }

    #[test]
    fn use_brings_into_scope_multi_only_matches_listed_symbols() {
        let r = resolved_with_symbols(vec!["User", "Post"]);
        assert!(use_brings_into_scope(&r, &[], None, "User"));
        assert!(!use_brings_into_scope(&r, &[], None, "Comment"));
    }

    #[test]
    fn use_brings_into_scope_whole_namespace_matches_anything() {
        let r = resolved_with_symbols(vec![]);
        assert!(use_brings_into_scope(&r, &[], None, "AnyName"));
    }

    #[test]
    fn use_brings_into_scope_remaining_path_must_match_name() {
        let r = resolved_with_symbols(vec![]);
        assert!(use_brings_into_scope(&r, &["User".to_string()], None, "User"));
        assert!(!use_brings_into_scope(&r, &["User".to_string()], None, "Other"));
    }

    #[test]
    fn use_brings_into_scope_alias_matches_too() {
        let r = resolved_with_symbols(vec![]);
        assert!(use_brings_into_scope(
            &r,
            &["User".to_string()],
            Some("Account"),
            "Account"
        ));
    }
}
