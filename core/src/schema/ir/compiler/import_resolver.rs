// Import resolution for use statements
// Handles resolving imports from same package, stdlib, and external dependencies

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::package::config::dependency::DependencyConfig;
use crate::package::config::ir::context::ProjectContext;
use crate::package::stdlib;
use crate::schema::idl::constants::SCHEMA_EXTENSION;
use crate::schema::idl::grammar::{Declaration, UsePath, RelativePrefix};
use crate::schema::ir::context::SchemaContext;
use crate::utils::text::closest;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Fix};
use crate::schema::ir::frozen::types::{Literal, TypeRef};
use crate::schema::ir::validation::constants::{fits, literal_value, primitive_bound, ResolvedConstant};
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::compiler::interpreter::incremental::freeze_validator_properties_sized;
use crate::schema::ir::validation::ImportLookup;
use crate::schema::ir::validation::data::{carries_data, Answers};

const UNRESOLVED: DiagnosticCode = DiagnosticCode::error("import::unresolved");

/// A `use` path that reaches one of the package's own schemas without
/// `package::`, `parent::` or `self::`. An absolute first segment means `std`
/// or a dependency, so the path is an error where it reaches one. The code is
/// in the specification's compiler cases (`imports`).
pub const BARE_OWN_SCHEMA_PATH: DiagnosticCode =
    DiagnosticCode::error("import::bare-own-schema-path");

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
                self.resolve_glob(&glob.path.to_string(), current_namespace)
            }
            UsePath::Multi(multi) => {
                let items: Vec<String> = multi.items.iter().map(|i| i.text.clone()).collect();
                self.resolve_multi(&multi.path.to_string(), &items, current_namespace)
            }
            UsePath::RelativeGlob(glob) => {
                self.resolve_relative_glob(&glob.prefix, current_namespace)
            }
            UsePath::RelativeMulti(multi) => {
                let items: Vec<String> = multi.items.iter().map(|i| i.text.clone()).collect();
                self.resolve_relative_multi(&multi.prefix, &items, current_namespace)
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
                self.resolve_glob(&glob.path.to_string(), current_namespace)
            }
            UsePath::Multi(multi) => {
                let items: Vec<String> = multi.items.iter().map(|i| i.text.clone()).collect();
                self.resolve_multi(&multi.path.to_string(), &items, current_namespace)
            }
            UsePath::RelativeGlob(glob) => {
                self.resolve_relative_glob(&glob.prefix, current_namespace)
            }
            UsePath::RelativeMulti(multi) => {
                let items: Vec<String> = multi.items.iter().map(|i| i.text.clone()).collect();
                self.resolve_relative_multi(&multi.prefix, &items, current_namespace)
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

    /// Resolve absolute path (e.g., `std::http::Request` or a dependency's
    /// `shared::models::Thing`). A package's own schemas are reached only
    /// through `package::`, `parent::` or `self::`: what falls through to
    /// "the same package" below is the resolver's reading of a bare path, and
    /// [`check_imports`] refuses it where it reaches one of them.
    fn resolve_absolute(
        &self,
        path: String,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        let parts = Self::split_absolute(&path)?;

        if let Some(result) = self.try_resolve_relative_prefix(&parts, current_namespace) {
            return result;
        }

        // A stdlib import (std::) loads from `stdlib_root` when one is given.
        // Without one it's an ordinary path: builds merge the embedded std's
        // schemas under `std` (see `package::stdlib`).
        if parts[0] == "std" && self.stdlib_root.is_some() {
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
        
        // Neither `std` nor a dependency: a bare path, which names a schema of
        // the same package when there is one (already loaded as a SchemaContext
        // in the ProjectContext being compiled, so no schema_path is needed to
        // load it from disk; see `resolve_use_to_schema` below for how it's
        // located). `check_imports` makes that an error
        // (`import::bare-own-schema-path`): own schemas are reached through
        // `package::`, `parent::` or `self::`.
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

    /// Resolve a glob straight after a prefix (`parent::*`): every name in the
    /// namespace the prefix points at.
    fn resolve_relative_glob(
        &self,
        prefix: &RelativePrefix,
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        Ok(ResolvedImport {
            symbols: vec!["*".to_string()], // Glob marker
            ..self.resolve_relative_prefix(prefix, &[], current_namespace)?
        })
    }

    /// Resolve a multi-import straight after a prefix (`parent::{A, B}`).
    fn resolve_relative_multi(
        &self,
        prefix: &RelativePrefix,
        items: &[String],
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        Ok(ResolvedImport {
            symbols: items.to_vec(),
            ..self.resolve_relative_prefix(prefix, &[], current_namespace)?
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
    
    /// The namespace a glob's or multi-import's path names: its segments,
    /// with a leading `self`/`parent`/`package` resolved against
    /// `current_namespace` the way an absolute path's is, so
    /// `use parent::common::*` and `use parent::common::{A, B}` point where
    /// `use parent::common::A` does.
    fn path_namespace(&self, path: &str, current_namespace: &[String]) -> Result<Vec<String>, String> {
        let parts = Self::split_absolute(path)?;
        match self.try_resolve_relative_prefix(&parts, current_namespace) {
            Some(resolved) => Ok(resolved?.absolute_namespace),
            None => Ok(parts),
        }
    }

    /// Resolve glob import (e.g., mypackage::types::*)
    fn resolve_glob(&self, path: &str, current_namespace: &[String]) -> Result<ResolvedImport, String> {
        Ok(ResolvedImport {
            absolute_namespace: self.path_namespace(path, current_namespace)?,
            schema_path: None,
            symbols: vec!["*".to_string()], // Glob marker
            alias: None,
        })
    }

    /// Resolve multi-import (e.g., mypackage::{User, Post})
    fn resolve_multi(
        &self,
        path: &str,
        items: &[String],
        current_namespace: &[String],
    ) -> Result<ResolvedImport, String> {
        Ok(ResolvedImport {
            absolute_namespace: self.path_namespace(path, current_namespace)?,
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
            
            match crate::schema::idl::parse::parse_schema(&source) {
                Ok(doc) => Ok(doc),
                Err(diagnostics) => Err(diagnostics
                    .iter()
                    .map(|d| crate::diagnostics::render(d, &path.to_string_lossy(), &source))
                    .collect::<Vec<_>>()
                    .join("\n")),
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
    let mut resolved = resolver.resolve(use_path, current_namespace)?;

    // A dependency's schemas sit under its name in the consumer's project,
    // but what they `use` is written against the dependency's own package:
    // `use s0::A` in `p0::s1` is `p0::s0`, not the consumer's `s0`. Its own
    // package is tried first; a path it doesn't match (`std::...`) resolves
    // as written.
    if let Some(dependency) = dependency_of(project_context, current_namespace) {
        let mut within = vec![dependency.clone()];
        within.extend(resolved.absolute_namespace.iter().cloned());
        if project_context.find_schema_by_import_namespace_parts(&within).is_some() {
            resolved.absolute_namespace = within;
        }
    }

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

/// The declared dependency a schema of `namespace` belongs to (its first
/// segment, as `interpret_schemas` namespaces them), or `None` for the
/// project's own schemas.
fn dependency_of(project_context: &ProjectContext, namespace: &[String]) -> Option<String> {
    let first = namespace.first()?;
    let dependencies =
        DependencyConfig::parse_dependencies(&project_context.config.assignments).ok()?;
    dependencies.contains_key(first).then(|| first.clone())
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

/// The `for` targets of the validator `name` declared in `declarations`,
/// with that schema's own aliases resolved. `None` when it declares no such
/// validator. Shared by the build's [`ProjectImports`] and the language
/// server, which differ only in how they find the declaring schema.
pub fn declared_validator_targets(
    declarations: &[rust_sitter::Spanned<Declaration>],
    name: &str,
) -> Option<Vec<TypeRef>> {
    declarations.iter().find_map(|decl| match &decl.value {
        Declaration::Validator(v) if v.name() == name => Some(
            v.targets()
                .into_iter()
                .map(|t| resolve_local_aliases(declarations, &TypeRef::from_grammar_sized(t, &|n| size_constant(declarations, None, n))))
                .collect(),
        ),
        _ => None,
    })
}

/// The properties of the validator `name` declared in `declarations`, frozen
/// as its own schema freezes them, with that schema's own aliases resolved in
/// their types (the importing schema can't see them). `None` when it declares
/// no such validator.
pub fn declared_validator_properties(
    declarations: &[rust_sitter::Spanned<Declaration>],
    name: &str,
) -> Option<Vec<FrozenUnit>> {
    declarations.iter().find_map(|decl| match &decl.value {
        Declaration::Validator(v) if v.name() == name => Some(
            freeze_validator_properties_sized(v, &|n| size_constant(declarations, None, n))
                .into_iter()
                .map(|mut field| {
                    if let FrozenUnit::Field { ty, .. } = &mut field {
                        *ty = resolve_local_aliases(declarations, ty);
                    }
                    field
                })
                .collect(),
        ),
        _ => None,
    })
}

/// What the `type` alias `name` declared in `declarations` stands for, with
/// that schema's own aliases resolved. `None` when it declares no such alias.
pub fn declared_alias_target(
    declarations: &[rust_sitter::Spanned<Declaration>],
    name: &str,
) -> Option<TypeRef> {
    declarations.iter().find_map(|decl| match &decl.value {
        Declaration::TypeAlias(a) if a.name() == name => Some(resolve_local_aliases(
            declarations,
            &TypeRef::from_grammar_sized(a.target_type(), &|n| size_constant(declarations, None, n)),
        )),
        _ => None,
    })
}

/// `ty` with each alias `declarations` declares replaced by its target, all
/// the way down. Bounded: an alias cycle is reported elsewhere.
fn resolve_local_aliases(
    declarations: &[rust_sitter::Spanned<Declaration>],
    ty: &TypeRef,
) -> TypeRef {
    fn go(
        declarations: &[rust_sitter::Spanned<Declaration>],
        ty: &TypeRef,
        depth: usize,
    ) -> TypeRef {
        if depth == 32 {
            return ty.clone();
        }
        ty.map_named(&mut |name| {
            declarations.iter().find_map(|decl| match &decl.value {
                Declaration::TypeAlias(a) if a.name() == name => Some(go(
                    declarations,
                    &TypeRef::from_grammar_sized(a.target_type(), &|n| size_constant(declarations, None, n)),
                    depth + 1,
                )),
                _ => None,
            })
        })
    }
    go(declarations, ty, 0)
}

/// [`ImportLookup`] for a build: follows a schema's `use` statements through
/// its [`ProjectContext`] (std included) to the schema that declares a name.
pub struct ProjectImports<'a> {
    pub declarations: &'a [rust_sitter::Spanned<Declaration>],
    pub namespace: &'a [String],
    pub project: &'a ProjectContext,
}

impl ProjectImports<'_> {
    /// The schema that declares what `name` names, and its name there. A
    /// qualified type is frozen as its path from the package's root (core#290),
    /// so it is the schema by that namespace; one that isn't (a validator
    /// called `validators::StringBounds`, as written) and a bare name are read
    /// through the schema's `use`s.
    fn declaring_schema(&self, name: &str) -> Option<(Rc<RefCell<SchemaContext>>, String)> {
        let bare = name.rsplit("::").next()?.to_string();
        if let Some(schema) = self.by_path(name) {
            return Some((schema, bare));
        }
        let schema =
            find_schema_bringing_into_scope(&bare, self.declarations, self.namespace, self.project)?;
        Some((schema, bare))
    }

    /// The schema whose namespace is the qualifier of `name`, when it
    /// declares the name.
    fn by_path(&self, name: &str) -> Option<Rc<RefCell<SchemaContext>>> {
        let (qualifier, bare) = name.rsplit_once("::")?;
        let parts: Vec<String> = qualifier.split("::").map(str::to_string).collect();
        let (schema, remaining) = self.project.find_schema_by_import_namespace_parts(&parts)?;
        let declared = remaining.is_empty() && importable_names(&schema.borrow()).iter().any(|n| n == bare);
        declared.then_some(schema)
    }
}

impl ImportLookup for ProjectImports<'_> {
    fn encoding_declared(&self, kind: crate::encodings::EncodingKind, name: &str) -> bool {
        crate::encodings::is_declared(self.project, kind, name)
    }
    fn encodings(&self) -> crate::schema::ir::unverified::Encodings<'_> {
        crate::schema::ir::unverified::Encodings::of(self.project)
    }
    fn reaches(&self, name: &str) -> bool {
        self.by_path(name).is_some()
    }

    fn validator_targets(&self, name: &str) -> Option<Vec<TypeRef>> {
        let (schema, bare) = self.declaring_schema(name)?;
        let schema = schema.borrow();
        declared_validator_targets(&schema.declarations, &bare)
    }

    fn alias_target(&self, name: &str) -> Option<TypeRef> {
        let (schema, bare) = self.declaring_schema(name)?;
        let schema = schema.borrow();
        declared_alias_target(&schema.declarations, &bare)
    }

    fn validator_properties(&self, name: &str) -> Option<Vec<FrozenUnit>> {
        let (schema, bare) = self.declaring_schema(name)?;
        let schema = schema.borrow();
        declared_validator_properties(&schema.declarations, &bare)
    }

    fn carries_data(&self, name: &str) -> Option<bool> {
        let (schema, bare) = self.declaring_schema(name)?;
        let schema = schema.borrow();
        let mut declared = DeclaredData { project: self.project, answers: Answers::new() };
        Some(declared.named(&schema.declarations, &schema.namespace, &bare))
    }

    fn constant(&self, name: &str) -> Option<ResolvedConstant> {
        let (schema, bare) = self.declaring_schema(name)?;
        let schema = schema.borrow();
        declared_constant(Some(self.project), &schema.declarations, &schema.namespace, &bare, 0)
    }
}

/// The constant `name` names from a schema with these `declarations`: one it
/// declares, or one its own `use`s bring in, with its value followed through
/// further constants the same way. `None` when there's no such constant, or
/// its value doesn't come to one its type holds: that schema's own build
/// says why. Bounded, so a cycle (reported where it's declared) ends.
fn declared_constant(
    project: Option<&ProjectContext>,
    declarations: &[rust_sitter::Spanned<Declaration>],
    namespace: &[String],
    name: &str,
    depth: usize,
) -> Option<ResolvedConstant> {
    if depth == 32 {
        return None;
    }
    let (prefix, bare) = match name.rsplit_once("::") {
        Some((prefix, bare)) => (Some(prefix), bare),
        None => (None, name),
    };
    // Declared here: written bare, or with this schema's own path.
    let here = prefix.is_none_or(|prefix| prefix == namespace.join("::"));
    let constant = declarations.iter().find_map(|decl| match &decl.value {
        Declaration::Const(c) if here && c.name() == bare => Some(c),
        _ => None,
    });
    let Some(constant) = constant else {
        let schema = find_schema_bringing_into_scope(bare, declarations, namespace, project?)?;
        let schema = schema.borrow();
        return declared_constant(project, &schema.declarations, &schema.namespace, bare, depth + 1);
    };
    let ty = TypeRef::from_grammar(constant.type_def()).as_primitive()?;
    let value = match Literal::from_grammar(constant.value())? {
        Literal::Reference(reference) => match primitive_bound(&reference) {
            Some(bound) => bound.ok()?,
            None => declared_constant(project, declarations, namespace, &reference, depth + 1)?,
        },
        literal => ResolvedConstant { ty, value: literal_value(ty, &literal)? },
    };
    fits(ty, &value).then_some(ResolvedConstant { ty, value: value.value })
}

/// The size a constant named as an array's length or maximum stands for
/// (`char[max MAX_NAME]`, core#98), read from a schema with these
/// `declarations`: one it declares, or with `project`, one its `use`s
/// bring in. `None` unless it's an unsigned integer constant whose value
/// comes to a size; the build says why.
pub fn size_constant(
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
    name: &str,
) -> Option<u64> {
    use crate::schema::ir::frozen::types::PrimitiveType as P;
    use crate::schema::ir::validation::constants::ConstantValue;
    // An integer type's bound: `char[max u16::MAX]`.
    if let Some(bound) = crate::schema::ir::validation::constants::primitive_bound(name) {
        return match bound.ok()?.value {
            ConstantValue::Integer(n) if bound_is_unsigned(name) => u64::try_from(n).ok(),
            _ => None,
        };
    }
    let (namespace, project) = match use_context {
        Some((namespace, project)) => (namespace, Some(project)),
        None => (&[][..], None),
    };
    let resolved = declared_constant(project, declarations, namespace, name, 0)?;
    match (resolved.ty, resolved.value) {
        (P::U8 | P::U16 | P::U32 | P::U64, ConstantValue::Integer(n)) => u64::try_from(n).ok(),
        _ => None,
    }
}

/// Whether `ty::BOUND` names an unsigned type's bound: a size is never
/// negative, so `s8::MIN` isn't one.
fn bound_is_unsigned(name: &str) -> bool {
    name.starts_with('u')
}

/// [`carries_data`] for a type declared in another schema, read from its
/// declarations and following its own `use`s on to further schemas. Answers
/// are keyed by `namespace::Name`, so a cycle through several schemas ends
/// the same way one inside a schema does.
struct DeclaredData<'p> {
    project: &'p ProjectContext,
    answers: Answers,
}

impl DeclaredData<'_> {
    fn named(
        &mut self,
        declarations: &[rust_sitter::Spanned<Declaration>],
        namespace: &[String],
        name: &str,
    ) -> bool {
        let bare = name.rsplit("::").next().unwrap_or(name);
        let key = format!("{}::{bare}", namespace.join("::"));
        match self.answers.get(&key) {
            Some(Some(known)) => return *known,
            Some(None) => return true,
            None => {}
        }
        self.answers.insert(key.clone(), None);
        let declared = declarations.iter().find(|decl| match &decl.value {
            Declaration::Struct(s) => s.name() == bare,
            Declaration::TypeAlias(a) => a.name() == bare,
            Declaration::Enum(e) => e.name() == bare,
            _ => false,
        });
        let carries = |ty: &TypeRef, this: &mut Self| {
            carries_data(ty, &mut |name| this.named(declarations, namespace, name))
        };
        let answer = match declared.map(|decl| &decl.value) {
            Some(Declaration::Struct(s)) => s.fields().iter().any(|field| {
                field.value.optional()
                    || carries(&TypeRef::from_grammar(field.value.field_type()), self)
            }),
            Some(Declaration::TypeAlias(a)) => {
                carries(&TypeRef::from_grammar_sized(a.target_type(), &|n| size_constant(declarations, None, n)), self)
            }
            Some(_) => true,
            None => {
                match find_schema_bringing_into_scope(bare, declarations, namespace, self.project) {
                    Some(schema) => {
                        let schema = schema.borrow();
                        self.named(&schema.declarations, &schema.namespace, bare)
                    }
                    None => true,
                }
            }
        };
        self.answers.insert(key, Some(answer));
        answer
    }
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
/// `alias`) brings the bare name `name` into scope. This is the older, wider
/// reading, where a whole-namespace `use` brings in every name the schema has:
/// the names a schema's `use`s are still read for (validators, constants, the
/// errors a function throws). A type's name is read by
/// [`references::Scope`](super::references::Scope), where it brings in its own
/// name only (core#290). The branch order is
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

/// The error for a `std::` path no std schema matches, with the closest std
/// namespace when there's one (`std::htp::Request` → `std::http`).
fn unknown_std_path(path: &[String]) -> String {
    let mut context = format!("std has no schema matching '{}'", path.join("::"));
    let written = path.iter().take(2).cloned().collect::<Vec<_>>().join("::");
    let namespaces: BTreeSet<String> = stdlib::schemas()
        .map(|(namespace, _)| namespace.iter().take(2).cloned().collect::<Vec<_>>().join("::"))
        .collect();
    if let Some(suggestion) = closest(&written, namespaces.iter().map(String::as_str)) {
        context.push_str(&format!(" - did you mean '{suggestion}'?"));
    }
    context
}

/// Every name a `use` can import from a schema: the type-level symbols
/// ([`declared_symbol_names`]) plus errors (`! Name` throws), validators and
/// settings.
pub(crate) fn importable_names(schema_context: &SchemaContext) -> Vec<String> {
    let mut names = declared_symbol_names(schema_context);
    names.extend(schema_context.declarations.iter().filter_map(|decl| match &decl.value {
        Declaration::Error(e) => Some(e.name.text.clone()),
        Declaration::Validator(v) => Some(v.name.text.clone()),
        // An unnamed settings block (`name: None`) applies implicitly to
        // its own file and isn't importable by name - `filter_map` drops
        // it naturally.
        Declaration::Settings(s) => s.name.as_ref().map(|n| n.text.clone()),
        _ => None,
    }));
    names
}

/// The first segment of a `use` path as written, when it is an absolute one:
/// `None` for a path that starts with `self`, `parent` or `package`, which is
/// relative. A bare path's first segment is `std`, a dependency's name, or
/// (the error [`BARE_OWN_SCHEMA_PATH`]) one of the package's own schemas.
pub fn absolute_first_segment(use_path: &UsePath) -> Option<&str> {
    let path = match use_path {
        UsePath::Absolute(scoped) => scoped.as_str(),
        UsePath::Glob(glob) => glob.path.as_str(),
        UsePath::Multi(multi) => multi.path.as_str(),
        UsePath::Relative(_) | UsePath::RelativeGlob(_) | UsePath::RelativeMulti(_) => return None,
    };
    let first = path.split("::").next().unwrap_or(path);
    (!matches!(first, "self" | "parent" | "package")).then_some(first)
}

/// Whether `parts` is a namespace that has no schema of its own: a
/// dependency's name or `std` on its own, or a directory of the project's
/// schemas (`billing`, with `billing::api` a schema).
pub(crate) fn is_namespace(
    project_context: &ProjectContext,
    dependencies: &HashSet<String>,
    parts: &[String],
) -> bool {
    if let [only] = parts {
        if only == stdlib::NAMESPACE || dependencies.contains(only) {
            return true;
        }
    }
    !parts.is_empty()
        && project_context.schema_contexts.iter().any(|schema| {
            let schema = schema.borrow();
            schema.namespace.len() > parts.len() && schema.namespace.starts_with(parts)
        })
}

/// A `use` path as it is written, without its alias.
fn written_path(use_path: &UsePath) -> String {
    match use_path {
        UsePath::Absolute(scoped) => scoped.to_string(),
        UsePath::Relative(rel) => format!("{}::{}", rel.prefix.keyword(), rel.path.as_str()),
        UsePath::Glob(glob) => format!("{}::*", glob.path.as_str()),
        UsePath::Multi(multi) => {
            let items: Vec<&str> = multi.items.iter().map(|item| item.text.as_str()).collect();
            format!("{}::{{{}}}", multi.path.as_str(), items.join(", "))
        }
        UsePath::RelativeGlob(glob) => format!("{}::*", glob.prefix.keyword()),
        UsePath::RelativeMulti(multi) => {
            let items: Vec<&str> = multi.items.iter().map(|item| item.text.as_str()).collect();
            format!("{}::{{{}}}", multi.prefix.keyword(), items.join(", "))
        }
    }
}

/// The error for a bare path to one of the package's own schemas, with the
/// fix that writes `package::` in front of it. `span` is the `use`
/// declaration's, and `source` the schema's text: without it the error stands
/// without a fix. The fix is the whole of the rewrite: what the import brings
/// in is the schema's namespace, whichever path reached it, so a reference
/// written `models::Blob` after `use models` stays right after
/// `use package::models`.
fn bare_own_schema_path(
    use_path: &UsePath,
    first: &str,
    span: (usize, usize),
    source: Option<&str>,
) -> Diagnostic {
    let written = written_path(use_path);
    let write = format!("package::{written}");
    let diagnostic = Diagnostic::new(
        BARE_OWN_SCHEMA_PATH,
        format!("'{written}' is one of this package's own schemas, reached by a bare path"),
    )
    .with_context(format!(
        "an absolute path starts with `std` or a dependency's name; '{first}' is neither"
    ))
    .with_span(span)
    .with_help(format!(
        "write `{write}`: a package's own schemas are reached as `package::...`, `parent::...` or `self::...`"
    ));
    match own_path_fix(source, span, first, &format!("Write `{write}`"), "package::", 0) {
        Some(fix) => diagnostic.with_fix(fix),
        None => diagnostic,
    }
}

/// Replacing the `replaced` bytes where the `use` declaration at `span` names
/// `first`, the first segment of its path, with `text` (inserting, for 0, as
/// `package::` is put in front of a bare path); `None` when `source` isn't
/// there, or the declaration isn't `use`, whitespace, and then `first`.
fn own_path_fix(
    source: Option<&str>,
    span: (usize, usize),
    first: &str,
    title: &str,
    text: &str,
    replaced: usize,
) -> Option<Fix> {
    let source = source?;
    let after_use = source.get(span.0..span.1)?.strip_prefix("use")?;
    let at = span.1 - after_use.trim_start().len();
    source
        .get(at..)?
        .starts_with(first)
        .then(|| Fix::replace(title, (at, at + replaced), text))
}

/// Every `use` in `declarations` that doesn't resolve, as validation
/// errors: a path no schema of the project matches (its own, or a
/// dependency's, merged under the dependency's name), or a named item the
/// matched schema doesn't declare. Run by the real build (`interpret_context`)
/// before a schema is compiled, so `comline build` / `check` reject the import
/// instead of trusting it.
///
/// A path that starts with neither `self`, `parent`, `package`, `std` nor a
/// dependency's name, and reaches one of the package's own schemas, is the
/// error [`BARE_OWN_SCHEMA_PATH`], with a fix that writes `package::` in front
/// of it. A path that reaches nothing is `import::unresolved`, as it was.
///
/// A `std::` path is checked like any other: the build merges the std
/// schemas a package imports (see `package::stdlib`), and one that no std
/// schema matches gets a suggestion from std's namespaces.
///
/// A declared dependency with no schemas in this project is an unresolved
/// import too, but only with the `deps` feature. Without it nothing merges a
/// dependency, and the build refuses a package that declares one before it
/// gets here (see `package::build`); a caller that gets here some other way
/// has nothing to check the import against.
///
/// Passes, because there is nothing to check it against: every `use` in a
/// dependency's own schemas (`current_namespace` under a dependency's name).
/// Those resolve against the dependency itself, and were checked when it was
/// compiled on its own during resolution. The same goes for std's own
/// schemas, which core's tests compile. A package's own schema can't be named
/// like a dependency or `std`, so this namespace is never one of its own (the
/// build rejects that before it gets here).
pub fn check_imports(
    declarations: &[rust_sitter::Spanned<Declaration>],
    current_namespace: &[String],
    project_context: &ProjectContext,
) -> Result<(), Vec<Diagnostic>> {
    let dependencies: HashSet<String> =
        DependencyConfig::parse_dependencies(&project_context.config.assignments)
            .map(|deps| deps.into_keys().collect())
            .unwrap_or_default();

    if current_namespace
        .first()
        .is_some_and(|first| dependencies.contains(first) || first == stdlib::NAMESPACE)
    {
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

    // The schema's text, for the fix a bare path to an own schema carries.
    let source: Option<String> = project_context
        .schema_contexts
        .iter()
        .find(|schema| schema.borrow().namespace == current_namespace)
        .and_then(|schema| {
            schema.borrow().codemap.files().first().map(|file| file.contents().to_string())
        });

    for decl in declarations {
        let Declaration::Use(use_stmt) = &decl.value else {
            continue;
        };
        let unresolved = |context: String| Diagnostic { code: UNRESOLVED, help: None, secondary: None, severity_override: None, fix: None, unit: None,
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
            // Only a build that can't merge dependencies at all has nothing
            // to check this against; with `deps`, a dependency with no
            // schemas here is an unresolved import like any other.
            if !cfg!(feature = "deps") && dependencies.contains(first) && !top_level.contains(first) {
                continue;
            }

            // A namespace with no schema of its own brings its name into
            // scope, to qualify with (core#290): a dependency's name or `std`
            // on its own, or a directory of schemas (`use package::billing`).
            // A directory reached by a bare path is the same error as a schema.
            if target.resolved.symbols.is_empty()
                && is_namespace(project_context, &dependencies, &target.resolved.absolute_namespace)
            {
                if let Some(written) = absolute_first_segment(&use_stmt.path) {
                    if written != stdlib::NAMESPACE && !dependencies.contains(written) {
                        errors.push(bare_own_schema_path(&use_stmt.path, written, decl.span, source.as_deref()));
                    }
                }
                continue;
            }

            if first == stdlib::NAMESPACE {
                errors.push(unresolved(unknown_std_path(&target.resolved.absolute_namespace)));
                continue;
            }

            let mut context = format!(
                "no schema in this package or its dependencies matches '{}'",
                target.resolved.absolute_namespace.join("::")
            );
            let mut fix = None;
            if !top_level.contains(first) && !dependencies.contains(first) {
                let known = top_level.iter().chain(&dependencies).map(String::as_str);
                if let Some(suggestion) = closest(first, known) {
                    // One of the package's own schemas is reached through
                    // `package::`, so that is what is suggested (core#286); a
                    // dependency's name and `std` are absolute as they are.
                    let own = top_level.contains(&suggestion)
                        && !dependencies.contains(&suggestion)
                        && suggestion != stdlib::NAMESPACE
                        && absolute_first_segment(&use_stmt.path) == Some(first);
                    if own {
                        let written = written_path(&use_stmt.path);
                        let corrected = format!("package::{suggestion}{}", &written[first.len()..]);
                        context.push_str(&format!(" - did you mean '{corrected}'?"));
                        fix = own_path_fix(
                            source.as_deref(),
                            decl.span,
                            first,
                            &format!("Write `{corrected}`"),
                            &format!("package::{suggestion}"),
                            first.len(),
                        );
                    } else {
                        context.push_str(&format!(" - did you mean '{suggestion}'?"));
                    }
                }
            }
            let mut diagnostic = unresolved(context);
            diagnostic.fix = fix;
            errors.push(diagnostic);
            continue;
        };

        // A path that starts with neither `std` nor a dependency's name, and
        // reaches one of the package's own schemas: those are only reached
        // through `package::`, `parent::` or `self::`.
        if let Some(written) = absolute_first_segment(&use_stmt.path) {
            if written != stdlib::NAMESPACE && !dependencies.contains(written) {
                errors.push(bare_own_schema_path(&use_stmt.path, written, decl.span, source.as_deref()));
                continue;
            }
        }

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
    fn relative_prefixes_resolve_in_globs_and_multi_imports() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns =
            vec!["mypackage".to_string(), "users".to_string(), "models".to_string()];
        let common = vec!["mypackage".to_string(), "users".to_string(), "common".to_string()];

        for source in ["use parent::common::*", "use parent::common::{Error, Other}"] {
            let path = parse_use_path(source);
            for resolved in [
                resolver.resolve(&path, &current_ns).unwrap(),
                resolver.resolve_namespace(&path, &current_ns).unwrap(),
            ] {
                assert_eq!(resolved.absolute_namespace, common, "{source}");
            }
        }

        // With a segment after the prefix it's one `ScopedIdentifier` like any
        // path (a bare prefix has its own productions, see below).
        let path = parse_use_path("use self::nested::{Profile, Avatar}");
        let resolved = resolver.resolve(&path, &current_ns).unwrap();
        let mut nested = current_ns.clone();
        nested.push("nested".to_string());
        assert_eq!(resolved.absolute_namespace, nested);
        assert_eq!(resolved.symbols, vec!["Profile".to_string(), "Avatar".to_string()]);

        let path = parse_use_path("use package::types::*");
        let resolved = resolver.resolve(&path, &current_ns).unwrap();
        assert_eq!(resolved.absolute_namespace, vec!["mypackage".to_string(), "types".to_string()]);
        assert_eq!(resolved.symbols, vec!["*".to_string()]);
    }

    #[test]
    fn a_bare_prefix_before_a_glob_or_multi_import_resolves() {
        let resolver = ImportResolver::new(vec!["mypackage".to_string()], HashMap::new(), None);
        let current_ns =
            vec!["mypackage".to_string(), "users".to_string(), "models".to_string()];
        let strings = |parts: &[&str]| parts.iter().map(|part| part.to_string()).collect::<Vec<_>>();

        let cases = [
            ("use self::*", strings(&["mypackage", "users", "models"]), strings(&["*"])),
            ("use parent::*", strings(&["mypackage", "users"]), strings(&["*"])),
            ("use package::*", strings(&["mypackage"]), strings(&["*"])),
            ("use self::{A}", strings(&["mypackage", "users", "models"]), strings(&["A"])),
            ("use parent::{Error, Page}", strings(&["mypackage", "users"]), strings(&["Error", "Page"])),
            ("use package::{A, B, C}", strings(&["mypackage"]), strings(&["A", "B", "C"])),
        ];
        for (source, namespace, symbols) in cases {
            let path = parse_use_path(source);
            for resolved in [
                resolver.resolve(&path, &current_ns).unwrap(),
                resolver.resolve_namespace(&path, &current_ns).unwrap(),
            ] {
                assert_eq!(resolved.absolute_namespace, namespace, "{source}");
                assert_eq!(resolved.symbols, symbols, "{source}");
            }
        }
    }

    #[test]
    fn a_bare_parent_from_the_root_namespace_cannot_go_up() {
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);
        for source in ["use parent::*", "use parent::{Error}"] {
            let path = parse_use_path(source);
            assert!(resolver.resolve(&path, &[]).is_err(), "{source}");
            assert!(resolver.resolve_namespace(&path, &[]).is_err(), "{source}");
        }
    }

    /// A prefix with a segment after it is still one path: `parent::Error`
    /// imports an item, only `parent::*` / `parent::{..}` use the new forms.
    #[test]
    fn a_prefix_followed_by_a_segment_still_parses_as_before() {
        assert!(matches!(parse_use_path("use parent::Error"), UsePath::Absolute(_)));
        assert!(matches!(parse_use_path("use parent::common::*"), UsePath::Glob(_)));
        assert!(matches!(parse_use_path("use parent::common::{Error}"), UsePath::Multi(_)));
        assert!(matches!(parse_use_path("use parent::*"), UsePath::RelativeGlob(_)));
        assert!(matches!(parse_use_path("use parent::{Error}"), UsePath::RelativeMulti(_)));
    }

    #[test]
    fn a_glob_from_the_root_namespace_cannot_go_up() {
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);
        let path = parse_use_path("use parent::common::*");
        assert!(resolver.resolve(&path, &[]).is_err());
        assert!(resolver.resolve_namespace(&path, &[]).is_err());
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
    fn resolve_treats_std_as_merged_schemas_without_a_stdlib_root() {
        // Builds merge the embedded std's schemas under `std`, so with no
        // on-disk root a std path resolves like any other.
        let resolver = ImportResolver::new(vec![], HashMap::new(), None);
        let path = parse_use_path("use std::http::Request");
        let resolved = resolver.resolve(&path, &[]).expect("std resolves without a root");
        assert_eq!(resolved.absolute_namespace, vec!["std", "http", "Request"]);
        assert_eq!(resolved.schema_path, None);
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
