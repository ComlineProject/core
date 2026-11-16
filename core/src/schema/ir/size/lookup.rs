//! What a name in a type names, for the sizer: its declaration, with the
//! types it holds written as the IR freezes them.

use std::borrow::Cow;

use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use crate::schema::ir::resolve::Resolver;

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

/// A [`Lookup`] over frozen schemas: the units of each, resolving a name as the
/// validator accepted it, through [`Resolver`]: the schema's own declarations,
/// its `use`s, aliases included, and a qualified path that is one of their
/// paths, and nothing looser. Its scope is the index of the schema, from
/// [`FrozenLookup::scope_of`].
pub struct FrozenLookup<'a> {
    resolver: Resolver<'a>,
}

impl<'a> FrozenLookup<'a> {
    /// The schemas given (each starting with its `Namespace` unit, as the
    /// frozen IR keeps them), all of the package being sized.
    pub fn new(schemas: impl IntoIterator<Item = &'a [FrozenUnit]>) -> Self {
        Self::with_packages(schemas, Vec::<String>::new())
    }

    /// The schemas given, among them those of other packages: a dependency's
    /// or `std`'s, whose namespaces begin with its name, one of `packages`.
    /// What a schema of those uses is read in its own package.
    pub fn with_packages(
        schemas: impl IntoIterator<Item = &'a [FrozenUnit]>,
        packages: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        FrozenLookup { resolver: Resolver::new(schemas, packages) }
    }

    /// The scope of the schema whose namespace is `namespace` (`api`,
    /// `billing::api`).
    pub fn scope_of(&self, namespace: &str) -> Option<usize> {
        self.resolver.scope_of(namespace)
    }
}

impl Lookup for FrozenLookup<'_> {
    type Scope = usize;

    fn resolve(&self, scope: usize, name: &str) -> Option<Resolved<'_, usize>> {
        // A name that doesn't resolve can't be sized: a type written in an
        // editor, or in a dependency's own dependency, which isn't here.
        let found = self.resolver.resolve(scope, name).ok()?;
        Some(Resolved { key: found.unit as *const FrozenUnit as usize, scope: found.schema, decl: decl(found.unit) })
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
