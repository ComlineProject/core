//! What a name written in a type names: one rule for the validator, `compat`
//! and the sizer, so they can't disagree about it (core#289).
//!
//! A name is resolved in the schema it is written in, by what the schema
//! holds and nothing else:
//!
//! 1. a declaration of the schema itself, by its bare name;
//! 2. a `use` of the schema: the name it brings in (its alias, else the last
//!    segment of its path), or a qualified path that is its path exactly.
//!
//! There is no suffix match and no taking the first schema that declares the
//! name: two schemas that declare `User` are told apart by which of them the
//! schema imports. A name that nothing in the schema brings in is an
//! [`Unresolved`], which a caller that already validated the schema takes for
//! an internal error.
//!
//! The frozen IR holds what this needs: a schema's units are its declarations
//! and its `Import`s, each import's path resolved to `namespace::Name` (and the
//! namespace of a whole-namespace `use`, a glob's `*`, and every name either
//! brings in, one import each).
//!
//! The validator sees one schema at a time, so it asks only what a `use` of
//! the schema brings in ([`Import::brings`]); [`Resolver`] goes on to the
//! declaration, among the schemas it is given.
//!
//! Qualified references that start with a name a `use` brought into scope
//! (core#290) are this module's to add: `Schema::reach` is where a name is
//! read in its schema, and [`Import::brings`] the rule for what an import is
//! a name for.

use std::collections::{HashMap, HashSet};

use crate::schema::ir::frozen::unit::{schema_namespace, FrozenUnit};

/// One `use` of a schema, as the IR freezes it: the path it resolved to and
/// the alias it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import<'a> {
    pub path: &'a str,
    pub alias: Option<&'a str>,
}

impl<'a> Import<'a> {
    /// The import `unit` is, if it is one.
    pub fn of(unit: &'a FrozenUnit) -> Option<Self> {
        match unit {
            FrozenUnit::Import(path, alias, _) => Some(Import {
                path,
                alias: alias.as_deref(),
            }),
            _ => None,
        }
    }

    /// The name it brings into scope on its own, Rust-style: its alias, else
    /// the last segment of its path. A path of one segment (a whole
    /// namespace) has none.
    pub fn bare(&self) -> Option<&'a str> {
        match self.alias {
            Some(alias) => Some(alias),
            None => self
                .path
                .rsplit("::")
                .next()
                .filter(|last| *last != self.path),
        }
    }

    /// Whether a name written in the schema is this import's: the name it
    /// brings in, or its path written out.
    pub fn brings(&self, name: &str) -> bool {
        self.bare() == Some(name) || self.path == name
    }
}

/// The name a declaration of a schema holds in its symbol table: any kind of
/// declaration, a re-export slot of another schema's error not being one.
pub fn declared_name(unit: &FrozenUnit) -> Option<&str> {
    match unit {
        FrozenUnit::Struct { name, .. }
        | FrozenUnit::Enum { name, .. }
        | FrozenUnit::Protocol { name, .. }
        | FrozenUnit::Constant { name, .. }
        | FrozenUnit::Validator { name, .. }
        | FrozenUnit::TypeAlias { name, .. }
        | FrozenUnit::Error {
            name,
            imported_from: None,
            ..
        } => Some(name),
        _ => None,
    }
}

/// Whether a declaration can stand where a type is written.
fn is_type(unit: &FrozenUnit) -> bool {
    matches!(
        unit,
        FrozenUnit::Struct { .. }
            | FrozenUnit::Enum { .. }
            | FrozenUnit::TypeAlias { .. }
            | FrozenUnit::Error { .. }
    )
}

/// A declaration a name resolved to, and the schema it is declared in: where
/// the names in it are written.
#[derive(Debug, Clone, Copy)]
pub struct Found<'a> {
    /// The schema, as [`Resolver::scope_of`] numbers them.
    pub schema: usize,
    pub unit: &'a FrozenUnit,
}

/// Why a name resolved to no declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved<'a> {
    /// Nothing the schema declares and no `use` of it brings the name in.
    NotInScope,
    /// A `use` brings it in, from a schema that wasn't given: a dependency's
    /// own dependency, which what a dependency publishes doesn't hold.
    NotLoaded(&'a str),
    /// It is declared (or brought in), but not a type: a protocol, a
    /// constant, a validator, a namespace.
    NotAType,
    /// A `use` brings it in from a schema that declares nothing of the name.
    NotDeclared(&'a str),
}

/// The schemas a name can be resolved among, each as its units: the
/// declarations and the `use`s of each, and where each begins.
pub struct Resolver<'a> {
    schemas: Vec<Schema<'a>>,
    by_namespace: HashMap<&'a str, usize>,
    roots: HashSet<String>,
}

/// One schema, as far as a name in it goes.
struct Schema<'a> {
    namespace: &'a str,
    /// The package whose schema this is, where it isn't the one being
    /// compiled: its `use`s name the schemas of its own package without it.
    package: Option<&'a str>,
    imports: Vec<Import<'a>>,
    declared: HashMap<&'a str, &'a FrozenUnit>,
}

impl<'a> Schema<'a> {
    /// What the name `name`, written in this schema, points at on its own:
    /// one of its declarations, or the path a `use` of it brings in. `None`
    /// where neither does.
    fn reach(&self, name: &str) -> Option<Reach<'a, '_>> {
        if let Some(unit) = self.declared.get(name) {
            return Some(Reach::Declared(unit));
        }
        let imports: Vec<&Import<'a>> = self.imports.iter().filter(|i| i.brings(name)).collect();
        (!imports.is_empty()).then_some(Reach::Imported(imports))
    }
}

/// What a name points at, before the declaration is found.
enum Reach<'a, 's> {
    Declared(&'a FrozenUnit),
    /// Each `use` that brings it in, in order.
    Imported(Vec<&'s Import<'a>>),
}

impl<'a> Resolver<'a> {
    /// The resolver among `schemas` (each starting with its `Namespace` unit,
    /// as the frozen IR keeps them). `packages` are the names the schemas of
    /// other packages begin with (a dependency's, `std`): what the `use`s of
    /// one of those schemas mean is in its own package.
    pub fn new(
        schemas: impl IntoIterator<Item = &'a [FrozenUnit]>,
        packages: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let roots: HashSet<String> = packages.into_iter().map(Into::into).collect();
        let schemas: Vec<Schema<'a>> = schemas
            .into_iter()
            .map(|units| {
                let namespace = schema_namespace(units).unwrap_or("");
                let package = namespace
                    .split("::")
                    .next()
                    .filter(|first| roots.contains(*first));
                let mut declared = HashMap::new();
                for unit in units {
                    if let Some(name) = declared_name(unit) {
                        declared.entry(name).or_insert(unit);
                    }
                }
                Schema {
                    namespace,
                    package,
                    imports: units.iter().filter_map(Import::of).collect(),
                    declared,
                }
            })
            .collect();
        let mut by_namespace = HashMap::new();
        for (at, schema) in schemas.iter().enumerate() {
            by_namespace.entry(schema.namespace).or_insert(at);
        }
        Resolver {
            schemas,
            by_namespace,
            roots,
        }
    }

    /// The schema whose namespace is `namespace` (`api`, `billing::api`).
    pub fn scope_of(&self, namespace: &str) -> Option<usize> {
        self.by_namespace.get(namespace).copied()
    }

    /// The namespace of the schema `at`.
    pub fn namespace(&self, at: usize) -> &'a str {
        self.schemas[at].namespace
    }

    /// Whether the schema `at` is another package's, a dependency's or
    /// `std`'s.
    pub fn is_other_package(&self, at: usize) -> bool {
        self.schemas[at].package.is_some()
    }

    /// The declaration `name` names, written in the schema `from`: the one
    /// the validator accepted it for.
    pub fn resolve(&self, from: usize, name: &str) -> Result<Found<'a>, Unresolved<'a>> {
        let schema = &self.schemas[from];
        match schema.reach(name).ok_or(Unresolved::NotInScope)? {
            Reach::Declared(unit) if is_type(unit) => Ok(Found { schema: from, unit }),
            Reach::Declared(_) => Err(Unresolved::NotAType),
            // The first of the `use`s that brings the name in and leads to a
            // declaration; where none does, why the first didn't.
            Reach::Imported(imports) => {
                let mut first = None;
                for import in imports {
                    match self.locate(schema, import.path) {
                        Ok(found) => return Ok(found),
                        Err(why) => {
                            first.get_or_insert(why);
                        }
                    }
                }
                Err(first.unwrap_or(Unresolved::NotInScope))
            }
        }
    }

    /// The declaration the path `namespace::Name` of a `use` in `schema`
    /// names.
    fn locate(&self, schema: &Schema<'a>, path: &'a str) -> Result<Found<'a>, Unresolved<'a>> {
        let Some((namespace, name)) = path.rsplit_once("::") else {
            return Err(Unresolved::NotAType);
        };
        let at = match schema.package {
            // A path in another package is written as that package does:
            // its own schemas' first, then std and the other packages'.
            Some(package) => {
                let within = format!("{package}::{namespace}");
                match self.scope_of(&within) {
                    Some(at) => Some(at),
                    None if self
                        .roots
                        .contains(namespace.split("::").next().unwrap_or(namespace)) =>
                    {
                        self.scope_of(namespace)
                    }
                    None => None,
                }
            }
            None => self.scope_of(namespace),
        };
        let at = at.ok_or(Unresolved::NotLoaded(path))?;
        match self.schemas[at].declared.get(name) {
            Some(unit) if is_type(unit) => Ok(Found { schema: at, unit }),
            Some(_) => Err(Unresolved::NotAType),
            None => Err(Unresolved::NotDeclared(path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(name: &str) -> FrozenUnit {
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: name.to_string(),
            fields: vec![],
            span: (0, 0),
        }
    }

    fn import(path: &str, alias: Option<&str>) -> FrozenUnit {
        FrozenUnit::Import(path.to_string(), alias.map(str::to_string), (0, 0))
    }

    fn schema(namespace: &str, mut units: Vec<FrozenUnit>) -> Vec<FrozenUnit> {
        units.insert(0, FrozenUnit::Namespace(namespace.to_string()));
        units
    }

    /// The namespace of the schema a name resolves to, or why it doesn't.
    fn namespace_of<'a>(
        resolver: &Resolver<'a>,
        schemas: &'a [Vec<FrozenUnit>],
        from: &str,
        name: &str,
    ) -> Result<&'a str, Unresolved<'a>> {
        let from = resolver.scope_of(from).unwrap();
        let found = resolver.resolve(from, name)?;
        Ok(schema_namespace(&schemas[found.schema]).unwrap())
    }

    #[test]
    fn a_name_is_what_the_schema_declares_or_imports() {
        let schemas = vec![
            schema("v1::api", vec![unit("User")]),
            schema("v2::api", vec![unit("User")]),
            schema(
                "app",
                vec![
                    import("v2::api::User", None),
                    import("v1::api::User", Some("Old")),
                    unit("Own"),
                ],
            ),
        ];
        let resolver = Resolver::new(schemas.iter().map(Vec::as_slice), Vec::<String>::new());
        let at = |name| namespace_of(&resolver, &schemas, "app", name);
        assert_eq!(at("User"), Ok("v2::api"), "bare");
        assert_eq!(at("Old"), Ok("v1::api"), "aliased");
        assert_eq!(at("v2::api::User"), Ok("v2::api"), "qualified");
        assert_eq!(
            at("v1::api::User"),
            Ok("v1::api"),
            "qualified, the path of an aliased import"
        );
        assert_eq!(at("Own"), Ok("app"), "its own");
    }

    #[test]
    fn nothing_is_matched_by_suffix_or_taken_first() {
        let schemas = vec![
            schema("rapi", vec![unit("User")]),
            schema("v1::api", vec![unit("User")]),
            schema(
                "app",
                vec![import("api::User", None), import("v1::other::Item", None)],
            ),
        ];
        let resolver = Resolver::new(schemas.iter().map(Vec::as_slice), Vec::<String>::new());
        let at = |name| namespace_of(&resolver, &schemas, "app", name);
        // `api::User` is a path no schema has: not `rapi`'s, not `v1::api`'s.
        assert_eq!(at("User"), Err(Unresolved::NotLoaded("api::User")));
        assert_eq!(at("api::User"), Err(Unresolved::NotLoaded("api::User")));
        // Not through the schema's imports: not in scope, whatever declares it.
        assert_eq!(at("v1::api::User"), Err(Unresolved::NotInScope));
        assert_eq!(at("Other"), Err(Unresolved::NotInScope));
        assert_eq!(at("api"), Err(Unresolved::NotInScope));
    }

    #[test]
    fn a_declaration_shadows_an_import_of_its_name() {
        let schemas = vec![
            schema("other", vec![unit("User")]),
            schema("app", vec![import("other::User", None), unit("User")]),
        ];
        let resolver = Resolver::new(schemas.iter().map(Vec::as_slice), Vec::<String>::new());
        assert_eq!(namespace_of(&resolver, &schemas, "app", "User"), Ok("app"));
    }

    #[test]
    fn what_is_no_type_is_said_so() {
        let constant = FrozenUnit::Constant {
            docstring: None,
            name: "LIMIT".to_string(),
            ty: crate::schema::ir::frozen::types::TypeRef::Unit,
            value: None,
            span: (0, 0),
        };
        let schemas = vec![
            schema("limits", vec![constant]),
            schema(
                "app",
                vec![
                    import("limits::LIMIT", None),
                    import("limits", None),
                    import("limits::Gone", None),
                ],
            ),
        ];
        let resolver = Resolver::new(schemas.iter().map(Vec::as_slice), Vec::<String>::new());
        let at = |name| namespace_of(&resolver, &schemas, "app", name);
        assert_eq!(at("LIMIT"), Err(Unresolved::NotAType));
        assert_eq!(at("Gone"), Err(Unresolved::NotDeclared("limits::Gone")));
        assert_eq!(at("limits"), Err(Unresolved::NotAType));
    }

    #[test]
    fn a_dependencys_paths_are_its_packages() {
        let schemas = vec![
            // The consumer's own `s0`, with an `A` of its own.
            schema("s0", vec![unit("A")]),
            schema("p0::s0", vec![unit("A")]),
            schema(
                "p0::s1",
                vec![
                    import("s0::A", None),
                    import("std::http::Request", None),
                    import("base::Gone", None),
                ],
            ),
            schema("std::http", vec![unit("Request")]),
        ];
        let resolver = Resolver::new(schemas.iter().map(Vec::as_slice), ["p0", "std"]);
        let at = |name| namespace_of(&resolver, &schemas, "p0::s1", name);
        assert_eq!(
            at("A"),
            Ok("p0::s0"),
            "its sibling, not the consumer's schema of the name"
        );
        assert_eq!(at("s0::A"), Ok("p0::s0"));
        assert_eq!(at("Request"), Ok("std::http"), "std, as written");
        // A package of its own that the consumer doesn't hold isn't guessed at.
        assert_eq!(at("Gone"), Err(Unresolved::NotLoaded("base::Gone")));
        assert!(resolver.is_other_package(resolver.scope_of("p0::s1").unwrap()));
        assert!(!resolver.is_other_package(resolver.scope_of("s0").unwrap()));
    }

    #[test]
    fn what_an_import_brings_in() {
        let plain = Import {
            path: "a::b::Name",
            alias: None,
        };
        assert_eq!(plain.bare(), Some("Name"));
        assert!(plain.brings("Name") && plain.brings("a::b::Name"));
        assert!(!plain.brings("b::Name") && !plain.brings("Other"));
        let aliased = Import {
            path: "a::b::Name",
            alias: Some("X"),
        };
        assert!(aliased.brings("X") && aliased.brings("a::b::Name") && !aliased.brings("Name"));
        // A whole namespace has no bare name, and is no more than its path.
        let namespace = Import {
            path: "types",
            alias: None,
        };
        assert_eq!(namespace.bare(), None);
        assert!(namespace.brings("types"));
    }
}
