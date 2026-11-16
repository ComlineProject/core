//! What a type written in a schema names: the Rust model (core#290).
//!
//! A name in scope in a schema is one of
//!
//! - a declaration of the schema itself;
//! - what a `use` of the schema brings in: its alias, else the last segment of
//!   its path (an alias replaces the name, `use package::types as t` brings in
//!   `t` and not `types`), or every name of a `{...}` or `*`;
//! - a root: `package`, `parent`, `self`, `std`, or a dependency's name.
//!
//! A whole-namespace `use` (`use package::types`) brings in only its own name,
//! `types`, and nothing in it: `types::User` reaches the item, a bare `User`
//! needs `use package::types::User`. A qualified reference resolves its first
//! segment by that rule, then walks down: after `use package::billing`,
//! `billing::api::Invoice` is the schema `billing::api`'s `Invoice`. A full
//! path from a root needs no `use` (`package::types::User`,
//! `parent::common::Error`, `self::nested::Type`, `std::http::Request`, a
//! dependency's `greeting::hello::Greeting`).
//!
//! A qualified reference freezes as its resolved path, from the package's root
//! (`api::Invoice`, `billing::api::Invoice` and an alias's `b::Invoice` all
//! freeze as `billing::api::Invoice`), so the frozen IR and its hashes are the
//! same however the file spells it. A bare name freezes as written.
//!
//! [`Scope`] is that rule over a schema's declarations and the project;
//! [`check_references`] reports what it refuses, every error at once:
//!
//! - [`NOT_IN_SCOPE`]: a bare name nothing brought in;
//! - [`QUALIFIER_NOT_IN_SCOPE`]: a qualified name whose first segment is
//!   neither a name in scope nor a root;
//! - [`DEFINED_TWICE`]: a name a `use` brings in that the schema also declares,
//!   or that two `use`s bring in.
//!
//! A `use` that fails to resolve (`import::unresolved`, reported by
//! [`check_imports`](super::import_resolver::check_imports)) still brings its
//! name into scope, so it never causes a second error here.
//!
//! The resolvers over the frozen IR (`compat`, the sizer, the validator's
//! lookups) read a name that is already resolved: see `schema::ir::resolve`.

use std::collections::{HashMap, HashSet};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Fix};
use crate::package::config::dependency::DependencyConfig;
use crate::package::config::ir::context::ProjectContext;
use crate::package::stdlib;
use crate::schema::idl::grammar::{Declaration, Type, Use, UsePath};
use crate::schema::ir::compiler::import_resolver::{
    importable_names, is_namespace, resolve_use_to_schema, ImportResolver,
};
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::frozen::types::PrimitiveType;

/// A bare name that no declaration of the schema and no `use` of it brought
/// in. The codes are in the specification's compiler cases (`references`).
pub const NOT_IN_SCOPE: DiagnosticCode = DiagnosticCode::error("reference::not-in-scope");

/// A qualified name whose first segment is not a name in scope, nor a root.
pub const QUALIFIER_NOT_IN_SCOPE: DiagnosticCode =
    DiagnosticCode::error("reference::qualifier-not-in-scope");

/// A name defined twice in a schema: a `use`d item and a declaration of the
/// same name, or two `use`s that bring the same name in.
pub const DEFINED_TWICE: DiagnosticCode = DiagnosticCode::error("reference::defined-twice");

/// A type name as it is written in a schema, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub name: String,
    /// The type it is in. A name inside an array or a union has no span of
    /// its own: [`locate`] finds it in the source.
    pub within: (usize, usize),
}

/// Every type name written in `declarations`, in source order, as it is
/// written (`User`, `types::User`). Primitives, and `any`, are not names:
/// `any` is a keyword in a validator's `for`, and validation says where else
/// it may not stand.
pub fn named_types(declarations: &[rust_sitter::Spanned<Declaration>]) -> Vec<Written> {
    let mut out = vec![];
    for decl in declarations {
        match &decl.value {
            Declaration::Const(c) => names_in(c.type_def(), c.type_def_span(), &mut out),
            Declaration::Struct(s) => {
                for field in s.fields() {
                    names_in(
                        field.value.field_type(),
                        field.value.field_type_span(),
                        &mut out,
                    );
                }
            }
            Declaration::Error(e) => {
                for field in e.fields() {
                    names_in(
                        field.value.field_type(),
                        field.value.field_type_span(),
                        &mut out,
                    );
                }
            }
            Declaration::Protocol(p) => {
                for function in p.functions() {
                    let function = &function.value;
                    if let Some(list) = function.args() {
                        let first = list.first();
                        names_in(first.arg_type(), first.arg_type_span(), &mut out);
                        for rest in list.rest() {
                            let arg = rest.arg_type();
                            names_in(arg.arg_type(), arg.arg_type_span(), &mut out);
                        }
                    }
                    if let Some(returns) = function.return_type() {
                        names_in(returns.return_type(), returns.return_type_span(), &mut out);
                    }
                }
            }
            Declaration::TypeAlias(a) => names_in(a.target_type(), a.target_type_span(), &mut out),
            Declaration::Validator(v) => {
                if let Some(targets) = &v.targets {
                    names_in(&targets.first.value, targets.first.span, &mut out);
                    for rest in &targets.rest {
                        names_in(&rest.target.value, rest.target.span, &mut out);
                    }
                }
                for property in v.properties() {
                    let ty = &property.value.property_type;
                    names_in(&ty.value, ty.span, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// The names in `ty`, in the order they are written. `any` is a keyword in a
/// validator's `for` (core#290 leaves where it may stand to validation).
fn names_in(ty: &Type, within: (usize, usize), out: &mut Vec<Written>) {
    match ty {
        Type::Named(id) => {
            let name = id.text.clone();
            if PrimitiveType::from_name(&name).is_some() || name == "any" {
                return;
            }
            out.push(Written { name, within });
        }
        Type::Array(array) => names_in(array.elem_type(), within, out),
        Type::Union(union) => {
            for member in union.members() {
                names_in(member, within, out);
            }
        }
        _ => {}
    }
}

/// Where each of `written` is in `source`: the name's own span, or the span of
/// the type it is in (and `false`) where it can't be found.
pub fn locate(source: Option<&str>, written: &[Written]) -> Vec<((usize, usize), bool)> {
    let mut cursors: HashMap<(usize, usize), usize> = HashMap::new();
    written
        .iter()
        .map(|w| {
            let from = *cursors.get(&w.within).unwrap_or(&w.within.0);
            match source.and_then(|source| find_name(source, w.within, &w.name, from)) {
                Some(span) => {
                    cursors.insert(w.within, span.1);
                    (span, true)
                }
                None => (w.within, false),
            }
        })
        .collect()
}

/// `name` as a whole name in `source[within]`, from `from` on.
fn find_name(
    source: &str,
    within: (usize, usize),
    name: &str,
    from: usize,
) -> Option<(usize, usize)> {
    let text = source.get(within.0..within.1)?;
    let skip = from.saturating_sub(within.0);
    let part_of_name = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':';
    text.match_indices(name).find_map(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let whole = !before.is_some_and(part_of_name) && !after.is_some_and(part_of_name);
        (at >= skip && whole).then_some((within.0 + at, within.0 + at + name.len()))
    })
}

/// What a `use` brought into scope a name for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A declaration of a schema: a type.
    Item,
    /// A schema, a directory of schemas, `std` or a dependency: something to
    /// qualify with.
    Namespace,
    /// A path that didn't resolve. It stands for whatever it needs to, so the
    /// error it already is is the only one.
    Unknown,
}

/// A name a `use` brought into scope.
#[derive(Debug, Clone)]
struct Binding {
    name: String,
    kind: Kind,
    /// What it is: the path from the package's root (or a root's), the name
    /// of the item last.
    path: Vec<String>,
    /// From a `*`: a declaration, or any other `use`, takes precedence.
    weak: bool,
    /// The `use` declaration.
    span: (usize, usize),
    /// A `use path` or `use path as alias`, which `as` can rename.
    single: bool,
    /// Written with an `as`.
    aliased: bool,
}

/// What a written name resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the name freezes as: a bare name as written, a qualified one as
    /// its path from the package's root.
    pub frozen: String,
    /// The schema (its namespace) and the declaration it names, when the
    /// project holds them.
    pub target: Option<(Vec<String>, String)>,
}

/// Why a written name resolved to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A bare name nothing brought in. `namespace` where a `use` did bring
    /// the name in, but as a namespace, which is no type.
    NotInScope { namespace: bool },
    /// A qualified name whose first segment is not in scope.
    QualifierNotInScope,
}

/// The names in scope in one schema.
pub struct Scope<'p> {
    namespace: Vec<String>,
    project: &'p ProjectContext,
    /// `std` and the dependencies' names.
    roots: HashSet<String>,
    /// What the schema declares, with where.
    locals: Vec<(String, (usize, usize))>,
    bindings: Vec<Binding>,
    /// A `*` of a path that didn't resolve: it may bring in any name.
    opaque_glob: bool,
    /// Where the `use`s are, and where the first declaration starts, for the
    /// fix that adds one.
    uses: Vec<(usize, usize)>,
    first_declaration: Option<usize>,
}

impl<'p> Scope<'p> {
    /// The scope of the schema `namespace` with these `declarations`.
    pub fn new(
        declarations: &[rust_sitter::Spanned<Declaration>],
        namespace: &[String],
        project: &'p ProjectContext,
    ) -> Self {
        let mut roots: HashSet<String> =
            DependencyConfig::parse_dependencies(&project.config.assignments)
                .map(|deps| deps.into_keys().collect())
                .unwrap_or_default();
        roots.insert(stdlib::NAMESPACE.to_string());
        let mut scope = Scope {
            namespace: namespace.to_vec(),
            project,
            roots,
            locals: vec![],
            bindings: vec![],
            opaque_glob: false,
            uses: vec![],
            first_declaration: None,
        };
        let resolver = ImportResolver::new(vec![], Default::default(), None);
        for decl in declarations {
            let declared = match &decl.value {
                Declaration::Use(stmt) => {
                    scope.uses.push(decl.span);
                    scope.add_use(&resolver, stmt, decl.span);
                    continue;
                }
                // The legacy `import path`: its last segment, as it always was.
                Declaration::Import(import) => {
                    scope.add_legacy_import(&import.path(), decl.span);
                    continue;
                }
                Declaration::Const(c) => c.name(),
                Declaration::Struct(s) => s.name(),
                Declaration::Enum(e) => e.name(),
                Declaration::Protocol(p) => p.name(),
                Declaration::Error(e) => e.name(),
                Declaration::Validator(v) => v.name(),
                Declaration::TypeAlias(a) => a.name(),
                Declaration::Settings(_) => continue,
            };
            scope.first_declaration.get_or_insert(decl.span.0);
            scope.locals.push((declared, decl.span));
        }
        scope
    }

    fn bind(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    fn add_legacy_import(&mut self, path: &str, span: (usize, usize)) {
        let path: Vec<String> = path.split("::").map(str::to_string).collect();
        if let Some(name) = path.last().cloned() {
            self.bind(Binding {
                name,
                kind: Kind::Unknown,
                path,
                weak: false,
                span,
                single: false,
                aliased: false,
            });
        }
    }

    fn add_use(&mut self, resolver: &ImportResolver, stmt: &Use, span: (usize, usize)) {
        let alias = stmt.alias.as_ref().map(|a| a.name.text.clone());
        let target =
            match resolve_use_to_schema(self.project, resolver, &self.namespace, &stmt.path) {
                Ok(target) => target,
                Err(_) => return self.add_unresolved(stmt, alias, span),
            };
        let parts = target.resolved.absolute_namespace.clone();
        let symbols = &target.resolved.symbols;
        let item = |name: &str, weak: bool| {
            let mut path = parts.clone();
            path.push(name.to_string());
            Binding {
                name: name.to_string(),
                kind: Kind::Item,
                path,
                weak,
                span,
                single: false,
                aliased: false,
            }
        };

        // `use ns::*`: every name `ns` has, which a declaration or another
        // `use` of the name takes precedence over, as in Rust.
        if symbols.len() == 1 && symbols[0] == "*" {
            match &target.schema {
                Some(schema) => {
                    for name in importable_names(&schema.borrow()) {
                        let binding = item(&name, true);
                        self.bind(binding);
                    }
                }
                None => self.opaque_glob = true,
            }
            return;
        }
        // `use ns::{A, B}`.
        if !symbols.is_empty() {
            for name in symbols {
                let binding = item(name, false);
                self.bind(binding);
            }
            return;
        }

        // `use ns`, `use ns::Name`, either `as` an alias, which replaces the
        // name.
        let Some(last) = parts.last().cloned() else {
            return;
        };
        let kind = match (&target.schema, target.remaining.is_empty()) {
            (Some(_), true) => Kind::Namespace,
            (Some(_), false) => Kind::Item,
            (None, _) if self.is_namespace(&parts) => Kind::Namespace,
            (None, _) => Kind::Unknown,
        };
        self.bind(Binding {
            name: alias.clone().unwrap_or(last),
            kind,
            path: parts,
            weak: false,
            span,
            single: true,
            aliased: alias.is_some(),
        });
    }

    /// A `use` whose path doesn't resolve at all (`parent` from the root):
    /// what it names is still brought in, as written.
    fn add_unresolved(&mut self, stmt: &Use, alias: Option<String>, span: (usize, usize)) {
        let (path, items): (String, Vec<String>) = match &stmt.path {
            UsePath::Absolute(path) => (path.text.clone(), vec![]),
            UsePath::Relative(rel) => (rel.path.to_string(), vec![]),
            UsePath::Multi(multi) => (
                multi.path.to_string(),
                multi.items.iter().map(|i| i.text.clone()).collect(),
            ),
            UsePath::RelativeMulti(multi) => (
                String::new(),
                multi.items.iter().map(|i| i.text.clone()).collect(),
            ),
            UsePath::Glob(_) | UsePath::RelativeGlob(_) => {
                self.opaque_glob = true;
                return;
            }
        };
        let parts: Vec<String> = path
            .split("::")
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        if items.is_empty() {
            if let Some(last) = parts.last().cloned() {
                self.bind(Binding {
                    name: alias.clone().unwrap_or(last),
                    kind: Kind::Unknown,
                    path: parts.clone(),
                    weak: false,
                    span,
                    single: true,
                    aliased: alias.is_some(),
                });
            }
        }
        for name in items {
            self.bind(Binding {
                name,
                kind: Kind::Unknown,
                path: parts.clone(),
                weak: false,
                span,
                single: false,
                aliased: false,
            });
        }
    }

    /// Whether `parts` is a namespace without a schema of its own: a
    /// directory of schemas, `std` or a dependency's name.
    fn is_namespace(&self, parts: &[String]) -> bool {
        is_namespace(self.project, &self.roots, parts)
    }

    /// The schema and declaration the path `path` (a namespace, then a name)
    /// names, when the project holds them.
    fn locate(&self, path: &[String]) -> Option<(Vec<String>, String)> {
        let (schema, remaining) = self.project.find_schema_by_import_namespace_parts(path)?;
        match remaining.as_slice() {
            [name] => Some((schema.borrow().namespace.clone(), name.clone())),
            _ => None,
        }
    }

    /// The binding of `name` that wins: a strong one before one from a `*`.
    fn binding(&self, name: &str, wanted: impl Fn(Kind) -> bool) -> Option<&Binding> {
        let mut named = self
            .bindings
            .iter()
            .filter(|b| b.name == name && wanted(b.kind));
        let first = named.next()?;
        if !first.weak {
            return Some(first);
        }
        std::iter::once(first)
            .chain(named)
            .find(|b| !b.weak)
            .or(Some(first))
    }

    fn declares(&self, name: &str) -> bool {
        self.locals.iter().any(|(declared, _)| declared == name)
    }

    /// What `written` names in this schema, or why nothing.
    pub fn resolve(&self, written: &str) -> Result<Resolved, Refusal> {
        let segments: Vec<&str> = written.split("::").collect();
        if segments.len() == 1 {
            return self.resolve_bare(written);
        }
        let first = segments[0];
        let mut path: Vec<String> = if let Some(binding) = self.binding(first, |k| k != Kind::Item)
        {
            binding.path.clone()
        } else if self.declares(first) || self.binding(first, |_| true).is_some() {
            // A declaration or an item is no qualifier.
            return Err(Refusal::QualifierNotInScope);
        } else {
            match first {
                "package" => vec![],
                "self" => self.namespace.clone(),
                "parent" => {
                    let mut up = self.namespace.clone();
                    up.pop().ok_or(Refusal::QualifierNotInScope)?;
                    up
                }
                root if self.roots.contains(root) => vec![root.to_string()],
                _ => return Err(Refusal::QualifierNotInScope),
            }
        };
        path.extend(segments[1..].iter().map(|s| s.to_string()));
        let target = self.locate(&path);
        // One of this schema's own: it is a declaration here, by its name.
        let own = target
            .as_ref()
            .filter(|(namespace, _)| *namespace == self.namespace)
            .map(|(_, name)| name.clone());
        if let Some(name) = own {
            return Ok(Resolved {
                frozen: name,
                target,
            });
        }
        Ok(Resolved {
            frozen: path.join("::"),
            target,
        })
    }

    fn resolve_bare(&self, name: &str) -> Result<Resolved, Refusal> {
        if self.declares(name) {
            return Ok(Resolved {
                frozen: name.to_string(),
                target: Some((self.namespace.clone(), name.to_string())),
            });
        }
        if let Some(binding) = self.binding(name, |k| k != Kind::Namespace) {
            let target = match binding.kind {
                Kind::Item => self.locate(&binding.path),
                _ => None,
            };
            return Ok(Resolved {
                frozen: name.to_string(),
                target,
            });
        }
        if self.binding(name, |_| true).is_some() {
            return Err(Refusal::NotInScope { namespace: true });
        }
        if self.opaque_glob {
            return Ok(Resolved {
                frozen: name.to_string(),
                target: None,
            });
        }
        Err(Refusal::NotInScope { namespace: false })
    }

    /// What `written` freezes as, where that is not as written: a qualified
    /// name that resolves, as its path.
    pub fn frozen(&self, written: &str) -> Option<String> {
        if !written.contains("::") {
            return None;
        }
        self.resolve(written)
            .ok()
            .map(|resolved| resolved.frozen)
            .filter(|frozen| frozen != written)
    }

    /// Whether `written` names a declaration the project holds.
    pub fn exists(&self, written: &str) -> bool {
        let Ok(Resolved {
            target: Some((namespace, name)),
            ..
        }) = self.resolve(written)
        else {
            return false;
        };
        self.project.schema_contexts.iter().any(|schema| {
            let schema = schema.borrow();
            schema.namespace == namespace && importable_names(&schema).iter().any(|n| *n == name)
        })
    }

    /// The schema the type `written` is in scope as names, when it does and
    /// the project holds it: where the compile order and the import cycles
    /// look for what a schema uses.
    pub fn reaches(&self, written: &str) -> Option<Vec<String>> {
        self.resolve(written)
            .ok()?
            .target
            .map(|(namespace, _)| namespace)
    }

    // ---- What a refusal says, and its fix.

    /// How the namespace `namespace` of the project is written from anywhere:
    /// the package's own from `package`, `std`'s and a dependency's from their
    /// names.
    fn absolute(&self, namespace: &[String]) -> String {
        match namespace.first() {
            Some(first) if self.roots.contains(first) => namespace.join("::"),
            _ => std::iter::once("package")
                .chain(namespace.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("::"),
        }
    }

    /// The namespaces of the project's schemas (not this one) that declare a
    /// type `name` and end with `qualifier`.
    fn declaring(&self, name: &str, qualifier: &[&str]) -> Vec<Vec<String>> {
        let mut found: Vec<Vec<String>> = vec![];
        for schema in &self.project.schema_contexts {
            let schema = schema.borrow();
            let namespace = &schema.namespace;
            let ends = namespace.len() >= qualifier.len()
                && namespace[namespace.len() - qualifier.len()..]
                    .iter()
                    .zip(qualifier)
                    .all(|(a, b)| a.as_str() == *b);
            if *namespace != self.namespace
                && ends
                && declares_type(&schema, name)
                && !found.contains(namespace)
            {
                found.push(namespace.clone());
            }
        }
        found.sort();
        found
    }

    /// How `namespace`'s `name` is written from what is in scope: through the
    /// namespace a `use` brought in that it is under, else from its root.
    fn written_from_scope(&self, namespace: &[String], name: &str) -> String {
        let under = self
            .bindings
            .iter()
            .filter(|b| b.kind == Kind::Namespace && namespace.starts_with(&b.path))
            .max_by_key(|b| b.path.len());
        match under {
            Some(binding) => {
                let mut parts = vec![binding.name.clone()];
                parts.extend(namespace[binding.path.len()..].iter().cloned());
                parts.push(name.to_string());
                parts.join("::")
            }
            None => format!("{}::{name}", self.absolute(namespace)),
        }
    }

    /// The diagnostic for a name written as `written`, at `span` (exactly
    /// where `exact`), that `refusal` says nothing resolved.
    pub fn diagnostic(
        &self,
        written: &str,
        refusal: Refusal,
        span: (usize, usize),
        exact: bool,
        source: Option<&str>,
    ) -> Diagnostic {
        match refusal {
            Refusal::NotInScope { namespace } => {
                self.not_in_scope(written, namespace, span, exact, source)
            }
            Refusal::QualifierNotInScope => self.qualifier_not_in_scope(written, span, exact),
        }
    }

    fn not_in_scope(
        &self,
        name: &str,
        namespace: bool,
        span: (usize, usize),
        _exact: bool,
        source: Option<&str>,
    ) -> Diagnostic {
        let (message, context) = if namespace {
            (
                format!("'{name}' is a namespace, not a type"),
                format!(
                    "a `use` brought '{name}' in as a name to qualify with: write `{name}::Type`"
                ),
            )
        } else {
            (
                format!("'{name}' is not in scope"),
                format!(
                    "this schema doesn't declare '{name}', and no `use` of it brings '{name}' in"
                ),
            )
        };
        let diagnostic = Diagnostic::new(NOT_IN_SCOPE, message)
            .with_context(context)
            .with_span(span);
        if namespace {
            return diagnostic;
        }
        let found = self.declaring(name, &[]);
        match found.as_slice() {
            [] => diagnostic.with_help(format!(
                "no schema of this package or its dependencies declares '{name}'"
            )),
            [one] => {
                let item = format!("{}::{name}", self.absolute(one));
                let qualified = self.written_from_scope(one, name);
                let diagnostic = diagnostic.with_help(format!(
                    "'{name}' is declared in `{}`: add `use {item}`, or write `{qualified}`",
                    self.absolute(one)
                ));
                match self.insert_use(&format!("use {item}"), source) {
                    Some(edit) => diagnostic.with_fix(Fix {
                        title: format!("Add `use {item}`"),
                        edits: vec![edit],
                    }),
                    None => diagnostic,
                }
            }
            many => diagnostic.with_help(format!(
                "'{name}' is declared in {}: add a `use` of the one you mean",
                many.iter()
                    .map(|ns| format!("`{}`", self.absolute(ns)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    fn qualifier_not_in_scope(
        &self,
        written: &str,
        span: (usize, usize),
        exact: bool,
    ) -> Diagnostic {
        let segments: Vec<&str> = written.split("::").collect();
        let first = segments[0];
        let diagnostic = Diagnostic::new(
            QUALIFIER_NOT_IN_SCOPE,
            format!("'{first}' is not in scope, in `{written}`"),
        )
        .with_context(format!(
            "a qualified name starts with a name a `use` brought in, or with `package`, `parent`, `self`, `std` or a dependency's name; '{first}' is none of them"
        ))
        .with_span(span);
        let (name, qualifier) = (
            segments[segments.len() - 1],
            &segments[..segments.len() - 1],
        );
        let found = self.declaring(name, qualifier);
        let [namespace] = found.as_slice() else {
            return match found.is_empty() {
                true => diagnostic.with_help(format!("nothing declares `{written}`: no schema of this package or its dependencies has a '{name}' under `{}`", qualifier.join("::"))),
                false => diagnostic.with_help(format!(
                    "`{written}` could be {}: write the path from `package` of the one you mean",
                    found.iter().map(|ns| format!("`{}::{name}`", self.absolute(ns))).collect::<Vec<_>>().join(", ")
                )),
            };
        };

        // The same item, if a `use` already brings it in by its name.
        let mut item = namespace.clone();
        item.push(name.to_string());
        let brought = self
            .binding(name, |k| k == Kind::Item)
            .filter(|b| b.path == item);
        let (write, help) = match brought {
            Some(_) => (
                name.to_string(),
                format!("`{name}` is in scope already: write `{name}`"),
            ),
            None => {
                let write = self.written_from_scope(namespace, name);
                (
                    write.clone(),
                    format!(
                        "write `{write}`, or add `use {}` to keep `{written}`",
                        self.absolute(namespace)
                    ),
                )
            }
        };
        let diagnostic = diagnostic.with_help(help);
        match exact {
            true => diagnostic.with_fix(Fix::replace(format!("Write `{write}`"), span, write)),
            false => diagnostic,
        }
    }

    /// The edit that adds the line `text` to the schema's `use`s: after the
    /// last of them, else before the first declaration.
    fn insert_use(&self, text: &str, source: Option<&str>) -> Option<((usize, usize), String)> {
        let source = source?;
        if let Some(last) = self.uses.iter().max_by_key(|span| span.1) {
            let end = last.0 + source.get(last.0..last.1)?.trim_end().len();
            return Some(((end, end), format!("\n{text}")));
        }
        let at = self.first_declaration?;
        source.get(..at)?;
        Some(((at, at), format!("{text}\n\n")))
    }

    /// Every name defined twice: a `use`d item and a declaration of the same
    /// name, or two `use`s of different things that bring in the same name.
    /// Two `use`s of the same thing are redundant, not an error, and a name
    /// from a `*` gives way to either.
    pub fn defined_twice(&self, source: Option<&str>) -> Vec<Diagnostic> {
        enum Site<'a> {
            Declaration,
            Use(&'a Binding),
        }
        let mut sites: Vec<(&str, (usize, usize), Site)> = vec![];
        for (name, span) in &self.locals {
            sites.push((name.as_str(), *span, Site::Declaration));
        }
        for binding in self.bindings.iter().filter(|b| !b.weak) {
            sites.push((binding.name.as_str(), binding.span, Site::Use(binding)));
        }
        sites.sort_by_key(|(_, span, _)| *span);

        let mut names: Vec<&str> = vec![];
        for (name, ..) in &sites {
            if !names.contains(name) {
                names.push(*name);
            }
        }
        let mut errors = vec![];
        for name in names {
            let mut here = sites.iter().filter(|(n, ..)| *n == name);
            let Some(first) = here.next() else { continue };
            // The first site that defines the name differently.
            let second = here.find(|(_, _, site)| match (&first.2, site) {
                (Site::Use(a), Site::Use(b)) => a.path != b.path || a.kind != b.kind,
                (Site::Declaration, Site::Declaration) => false,
                _ => true,
            });
            let Some(second) = second else { continue };
            let what = match (&first.2, &second.2) {
                (Site::Use(_), Site::Use(_)) => format!("two `use`s bring in '{name}'"),
                _ => format!("a `use` brings in '{name}' and this schema declares it too"),
            };
            let mut diagnostic = Diagnostic::new(
                DEFINED_TWICE,
                format!("'{name}' is defined twice in this schema"),
            )
            .with_context(what)
            .with_span(second.1)
            .with_secondary_span(first.1, "defined here too");

            // The `use` that can take an alias: the later one, else the other.
            let aliasable = [&second.2, &first.2]
                .into_iter()
                .find_map(|site| match site {
                    Site::Use(b) if b.single && !b.aliased && b.kind == Kind::Item => Some(*b),
                    _ => None,
                });
            match aliasable {
                Some(binding) => {
                    let alias = alias_for(&binding.path);
                    let written = format!(
                        "use {} as {alias}",
                        self.absolute(&binding.path[..binding.path.len() - 1])
                            + "::"
                            + &binding.path[binding.path.len() - 1]
                    );
                    diagnostic = diagnostic
                        .with_help(format!("rename one, or import with an alias: `{written}`"));
                    let taken = sites.iter().any(|(n, ..)| *n == alias);
                    let end = source
                        .and_then(|source| source.get(binding.span.0..binding.span.1))
                        .map(|text| binding.span.0 + text.trim_end().len());
                    if let (false, Some(end)) = (taken, end) {
                        diagnostic = diagnostic.with_fix(Fix {
                            title: format!("Import `{name}` as `{alias}`"),
                            edits: vec![((end, end), format!(" as {alias}"))],
                        });
                    }
                }
                None => {
                    diagnostic = diagnostic.with_help(
                        "rename one, or import with an alias: `use path::Name as OtherName`",
                    );
                }
            }
            errors.push(diagnostic);
        }
        errors
    }
}

/// An alias for an imported item that tells it from another of its name: its
/// first namespace segment in PascalCase, then the name (`billing::api::Invoice`
/// is `BillingInvoice`).
fn alias_for(path: &[String]) -> String {
    let Some((name, namespace)) = path.split_last() else {
        return String::new();
    };
    let mut alias = String::new();
    if let Some(first) = namespace.first() {
        for word in first.split('_').filter(|w| !w.is_empty()) {
            let mut chars = word.chars();
            if let Some(c) = chars.next() {
                alias.extend(c.to_uppercase());
                alias.push_str(chars.as_str());
            }
        }
    }
    alias.push_str(name);
    alias
}

/// Whether `schema` declares a type `name`.
fn declares_type(schema: &SchemaContext, name: &str) -> bool {
    schema.declarations.iter().any(|decl| match &decl.value {
        Declaration::Struct(s) => s.name() == name,
        Declaration::Enum(e) => e.name() == name,
        Declaration::TypeAlias(a) => a.name() == name,
        Declaration::Error(e) => e.name() == name,
        _ => false,
    })
}

/// The text of the schema `namespace`, for the fixes: where they edit.
pub fn source_of(project: &ProjectContext, namespace: &[String]) -> Option<String> {
    project
        .schema_contexts
        .iter()
        .find(|schema| schema.borrow().namespace == namespace)
        .and_then(|schema| {
            schema
                .borrow()
                .codemap
                .files()
                .first()
                .map(|file| file.contents().to_string())
        })
}

/// Every name in `declarations` that isn't in scope, and every name defined
/// twice, as errors: all of them, not the first. A dependency's schemas and
/// `std`'s are written against their own packages and were checked when those
/// compiled, so they pass.
pub fn check_references(
    declarations: &[rust_sitter::Spanned<Declaration>],
    namespace: &[String],
    project: &ProjectContext,
) -> Result<(), Vec<Diagnostic>> {
    let scope = Scope::new(declarations, namespace, project);
    if namespace
        .first()
        .is_some_and(|first| scope.roots.contains(first))
    {
        return Ok(());
    }
    let source = source_of(project, namespace);
    let source = source.as_deref();

    let written = named_types(declarations);
    let spans = locate(source, &written);
    let mut errors: Vec<Diagnostic> = written
        .iter()
        .zip(spans)
        .filter_map(|(w, (span, exact))| {
            let refusal = scope.resolve(&w.name).err()?;
            Some(scope.diagnostic(&w.name, refusal, span, exact, source))
        })
        .collect();
    errors.extend(scope.defined_twice(source));
    errors.sort_by_key(|error| error.span);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
