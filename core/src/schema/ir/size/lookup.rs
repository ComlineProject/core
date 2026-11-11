//! What a name in a type names, for the sizer: its declaration, with the
//! types it holds written as the IR freezes them.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::{schema_namespace, ConfigValue, FrozenUnit};

/// A declaration, as far as its size goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Decl<'a> {
    /// Its fields in declaration order. `max_depth` is `@max_depth`, as
    /// written or filled in from settings.
    Struct { name: Cow<'a, str>, fields: Vec<Member<'a>>, max_depth: Option<u64> },
    /// The names of its variants, none of which holds a value.
    Enum { name: Cow<'a, str>, variants: Vec<Cow<'a, str>> },
    /// `type NAME = TARGET`: the same type (an alias is transparent), and
    /// its `@max_depth` where it ends what contains itself through it.
    Alias { target: Cow<'a, TypeRef>, max_depth: Option<u64> },
    /// A declaration with no size rule: an enum whose variants hold values,
    /// an error. Its size can't be told.
    Opaque,
}

/// One field of a struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Member<'a> {
    pub name: Cow<'a, str>,
    pub ty: Cow<'a, TypeRef>,
    /// `optional name: T`: absent or a `T`.
    pub optional: bool,
}

/// What a name resolved to.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<'a, S> {
    /// What tells this declaration from another: the same for the same one,
    /// each time it is found. It is what a recursion through it is counted by.
    pub key: usize,
    /// Where the types in `decl` are written, which a name in them resolves from.
    pub scope: S,
    pub decl: Decl<'a>,
}

/// Resolves a name written in a type to its declaration. How is the
/// caller's: a build resolves through each schema's imports
/// ([`FrozenLookup`]), an editor through whatever it has open.
pub trait Lookup {
    /// Where a name is written: what it resolves relative to (a schema, for
    /// one). The sizer carries a declaration's own along into it.
    type Scope: Copy;

    /// The declaration `name` names, written in `scope`.
    fn resolve(&self, scope: Self::Scope, name: &str) -> Option<Resolved<'_, Self::Scope>>;
}

/// A [`Lookup`] over frozen schemas: the units of each, resolving a name
/// through the schema's own declarations, its `use`s, a qualified path, and
/// last any schema's declaration of that name, as `compat` finds the
/// declarations a protocol reaches. Its scope is the index of the schema, from
/// [`FrozenLookup::scope_of`].
pub struct FrozenLookup<'a> {
    schemas: Vec<(&'a str, &'a [FrozenUnit])>,
    /// Each declared name's schemas, in order.
    by_name: HashMap<&'a str, Vec<usize>>,
}

impl<'a> FrozenLookup<'a> {
    /// The schemas given (each starting with its `Namespace` unit, as the
    /// frozen IR keeps them).
    pub fn new(schemas: impl IntoIterator<Item = &'a [FrozenUnit]>) -> Self {
        let schemas: Vec<(&str, &[FrozenUnit])> =
            schemas.into_iter().map(|units| (schema_namespace(units).unwrap_or(""), units)).collect();
        let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        for (at, &(_, units)) in schemas.iter().enumerate() {
            for unit in units {
                if let Some(name) = declared_name(unit) {
                    by_name.entry(name).or_default().push(at);
                }
            }
        }
        FrozenLookup { schemas, by_name }
    }

    /// The scope of the schema whose namespace is `namespace` (`api`,
    /// `billing::api`).
    pub fn scope_of(&self, namespace: &str) -> Option<usize> {
        self.schemas.iter().position(|(ns, _)| *ns == namespace)
    }

    /// The declaration `name` in the schema `at`.
    fn declared(&self, at: usize, name: &str) -> Option<(usize, &'a FrozenUnit)> {
        let &(_, units) = self.schemas.get(at)?;
        units.iter().find(|u| declared_name(u) == Some(name)).map(|u| (at, u))
    }

    fn find(&self, from: usize, name: &str) -> Option<(usize, &'a FrozenUnit)> {
        let (path, last) = match name.rsplit_once("::") {
            Some((path, last)) => (Some(path), last),
            None => (None, name),
        };
        match path {
            Some(path) => {
                let in_path = |namespace: &str| {
                    namespace == path
                        || namespace.ends_with(&format!("::{path}"))
                        || path.ends_with(&format!("::{namespace}"))
                };
                let found = (0..self.schemas.len())
                    .filter(|at| in_path(self.schemas[*at].0))
                    .find_map(|at| self.declared(at, last));
                if found.is_some() {
                    return found;
                }
            }
            None => {
                if let Some(found) = self.declared(from, last) {
                    return Some(found);
                }
                if let Some(found) = self.imported(from, last) {
                    return Some(found);
                }
            }
        }
        // As `compat` finds a name: any schema's declaration of it.
        let at = *self.by_name.get(last)?.first()?;
        self.declared(at, last)
    }

    /// What the schema `from`'s `use`s bring in as `name`.
    fn imported(&self, from: usize, name: &str) -> Option<(usize, &'a FrozenUnit)> {
        let &(_, units) = self.schemas.get(from)?;
        units.iter().find_map(|unit| {
            let FrozenUnit::Import(path, alias, _) = unit else { return None };
            if let Some(glob) = path.strip_suffix("::*") {
                return self.find(from, &format!("{glob}::{name}"));
            }
            let (_, last) = path.rsplit_once("::").unwrap_or(("", path));
            if alias.as_deref().unwrap_or(last) != name {
                return None;
            }
            self.find(from, path)
        })
    }
}

impl Lookup for FrozenLookup<'_> {
    type Scope = usize;

    fn resolve(&self, scope: usize, name: &str) -> Option<Resolved<'_, usize>> {
        let (at, unit) = self.find(scope, name)?;
        Some(Resolved { key: unit as *const FrozenUnit as usize, scope: at, decl: decl(unit) })
    }
}

fn declared_name(unit: &FrozenUnit) -> Option<&str> {
    match unit {
        FrozenUnit::Struct { name, .. } | FrozenUnit::Enum { name, .. } | FrozenUnit::TypeAlias { name, .. } => {
            Some(name)
        }
        // An error has no size of its own: a `throws` isn't sized, and one
        // named as a field's type is refused by validation.
        _ => None,
    }
}

/// A frozen declaration's size-relevant parts.
fn decl(unit: &FrozenUnit) -> Decl<'_> {
    match unit {
        FrozenUnit::Struct { name, fields, parameters, .. } => Decl::Struct {
            name: Cow::Borrowed(name),
            fields: fields
                .iter()
                .filter_map(|f| match f {
                    FrozenUnit::Field { name, ty, optional, .. } => {
                        Some(Member { name: Cow::Borrowed(name), ty: Cow::Borrowed(ty), optional: *optional })
                    }
                    _ => None,
                })
                .collect(),
            max_depth: max_depth(parameters),
        },
        FrozenUnit::Enum { name, variants, .. } => {
            let names: Vec<Cow<str>> = variants
                .iter()
                .filter_map(|v| match v {
                    FrozenUnit::EnumVariant(name, _) => Some(Cow::Borrowed(name.as_str())),
                    _ => None,
                })
                .collect();
            // A variant that is anything but a name holds a value.
            if names.len() == variants.len() {
                Decl::Enum { name: Cow::Borrowed(name), variants: names }
            } else {
                Decl::Opaque
            }
        }
        FrozenUnit::TypeAlias { target, parameters, .. } => {
            Decl::Alias { target: Cow::Borrowed(target), max_depth: max_depth(parameters) }
        }
        _ => Decl::Opaque,
    }
}

/// A declaration's `@max_depth`, as written or filled in from settings.
fn max_depth(parameters: &[FrozenUnit]) -> Option<u64> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value: ConfigValue::Integer(n) } if name == "max_depth" => {
            u64::try_from(*n).ok()
        }
        _ => None,
    })
}
