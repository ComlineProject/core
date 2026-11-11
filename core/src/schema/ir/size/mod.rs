//! The bytes a type takes in a data format: its least and its most, from the
//! format's size rules in the specification's catalog
//! ([`comline_catalog::Sizes`]), applied to the frozen IR.
//!
//! [`range`] is the entry point: a [`TypeRef`], a format's rules and a
//! [`Lookup`] for the names in it give a [`Range`], or `None` where the type
//! can't be sized at all. It works on what the IR already holds resolved: each
//! `char[max N]` and `T[max N]`, each fixed length, and each struct's
//! `@max_depth`. Nothing here encodes a value: the rules are the
//! specification's, and its size vectors (`vectors/sizes-<format>.json`) hold
//! this module to them, case by case.
//!
//! **What a range says.** Every value of the type takes `min` to `max` bytes
//! encoded, and some value takes `min`, and some `max`, unless `upper_bound`:
//! then `max` is only a bound no value is known to reach (no official format
//! has one). `max` is `None` where there is no bound at all: a `string`, a
//! `T[]`, anything holding one, a recursive type with no depth limit.
//!
//! **What can't be sized** (`None`): a `union` (no format has a rule for one),
//! an enum with fields, a format with no `sizes`, a name that doesn't resolve,
//! an enum with no variants, a type that has to contain itself (a required
//! field of its own type has no finite value), and a type no value of which
//! can be written (below).
//!
//! **Where the formats stop.** No official format writes a count or a length
//! above [`MAX_COUNT`] (2^32 - 1) in a header that says it. So a bound above
//! it (`T[max 5000000000]`, `char[max 2000000000]`, whose bytes would be
//! more) is cut to what can be written, and a fixed length above it (`T[N]`
//! where the format writes N) can't be written at all: no value exists, and
//! the type isn't sized. A header that says nothing (`none`, or a delimited
//! one) has no such limit. A *maximum* above `u64::MAX` bytes has no number:
//! it is `None`, no bound. A *minimum* above it can't be written: the type
//! isn't sized.
//!
//! **Recursion** follows the specification: a struct that contains itself,
//! under `@max_depth = D`, takes the sizes of the rules to depth D, and a
//! value at level D + 1 can't be: an option there is absent, an array
//! empty, and a required field makes the struct itself impossible. Each
//! struct (or alias) counts its own nesting: with `A` and `B` containing each
//! other, `A`'s limit counts the `A`s on the way down. One without a limit
//! ends nothing: its maximum is `None`. A declaration more than
//! [`MAX_NESTING`] deep isn't sized.
//!
//! **Witnesses** ([`witnesses`]) are a value of the type of the least and of
//! the most size, in the specification's typed notation (`{"u8": 0}`,
//! `{"struct": {"name": ..., "fields": [[name, value], ...]}}`), for a
//! differential test to encode: each must encode to exactly `min` and `max`
//! bytes. They are built by the same code as the range, so they follow it.
//!
//! The old, codec-independent estimate (`WireSize`) is gone; the editor's
//! entry, over the grammar a half-written file parses to, is in
//! `schema::idl::size` (with the `compiler` feature).

use std::collections::HashMap;

use serde_json::Value;

use crate::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use comline_catalog::Primitive;

mod lookup;
mod sizer;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;

pub use comline_catalog::Sizes;
pub use lookup::{Decl, FrozenLookup, Lookup, Member, Resolved};
pub use sizer::MAX_COUNT;

use sizer::{Size, Sizer, NO_VALUE};

/// The most declarations a type may be nested through, `@max_depth`'s
/// included, before it isn't sized: bounds the recursion.
pub const MAX_NESTING: usize = 256;

/// How many bytes a type takes in a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    /// The fewest bytes of any value.
    pub min: u64,
    /// The most; `None` where there is no bound.
    pub max: Option<u64>,
    /// `max` is only a bound that no value is known to reach.
    pub upper_bound: bool,
}

impl Range {
    /// Whether every value takes the same bytes.
    pub fn is_fixed(&self) -> bool {
        self.max == Some(self.min)
    }
}

/// A value of the least size and one of the most, in the specification's
/// typed notation.
#[derive(Debug, Clone, PartialEq)]
pub struct Witnesses {
    /// Encodes to exactly [`Range::min`] bytes.
    pub min: Value,
    /// Encodes to exactly [`Range::max`] bytes; `None` where there is no
    /// maximum, it is only a bound, or the value is too large to build.
    pub max: Option<Value>,
}

/// The size rules of the official format named `format` (its wire name or an
/// alias: `packed@1`, `json@1`); `None` for any other, which has none.
pub fn rules(format: &str) -> Option<&'static Sizes> {
    comline_catalog::resolve(format).and_then(|encoding| encoding.sizes)
}

/// The range of `ty`, written in `scope`.
pub fn range<L: Lookup>(ty: &TypeRef, rules: &Sizes, lookup: &L, scope: L::Scope) -> Option<Range> {
    run(rules, lookup, None, |walk| walk.size(ty, scope)).map(|size| size.range)
}

/// The range of `ty` as an `optional` field's type: absent, or a `ty`.
pub fn range_optional<L: Lookup>(ty: &TypeRef, rules: &Sizes, lookup: &L, scope: L::Scope) -> Option<Range> {
    run(rules, lookup, None, |walk| walk.optional(ty, scope)).map(|size| size.range)
}

/// The range of an already resolved declaration: what a declaration being
/// edited is, which no name need resolve to.
pub fn range_of_decl<L: Lookup>(decl: Resolved<'_, L::Scope>, rules: &Sizes, lookup: &L) -> Option<Range> {
    run(rules, lookup, None, |walk| walk.declared(decl)).map(|size| size.range)
}

/// Witnesses for `ty`, written in `scope`: a value of the least size and one
/// of the most. A value takes at most `budget` to build, a node or a
/// character each: a type with a bound of billions has a maximum value too
/// large to write down, and then `max` is `None`. `None` for a type that
/// isn't sized or whose least value is that large.
pub fn witnesses<L: Lookup>(
    ty: &TypeRef,
    rules: &Sizes,
    lookup: &L,
    scope: L::Scope,
    budget: usize,
) -> Option<Witnesses> {
    let size = run(rules, lookup, Some(budget), |walk| walk.size(ty, scope))?;
    Some(Witnesses { min: size.min_witness()?.clone(), max: size.max_witness().cloned() })
}

/// What can't be sized, as opposed to what has no value.
#[derive(Clone)]
struct Unsizable;

/// `Ok(None)`: no value of it exists here (past `@max_depth`).
type Out = Result<Option<Size>, Unsizable>;

fn run<L: Lookup>(
    rules: &Sizes,
    lookup: &L,
    budget: Option<usize>,
    f: impl FnOnce(&mut Walk<'_, L>) -> Out,
) -> Option<Size> {
    let mut walk = Walk { sizer: Sizer::new(rules, budget), lookup, stack: vec![], memo: HashMap::new() };
    match f(&mut walk) {
        Ok(Some(size)) if size.range.min != NO_VALUE => Some(size),
        _ => None,
    }
}

struct Walk<'a, L: Lookup> {
    sizer: Sizer<'a>,
    lookup: &'a L,
    /// The declarations being sized, outermost first, by key.
    stack: Vec<usize>,
    /// Each declaration's size, given the declarations being sized (as a set
    /// with counts): a type contains itself to a depth, and a tree of them
    /// would be walked once for each of its branches without this.
    memo: HashMap<(usize, Vec<usize>), Out>,
}

fn primitive(p: PrimitiveType) -> Primitive {
    match p {
        PrimitiveType::S8 => Primitive::S8,
        PrimitiveType::S16 => Primitive::S16,
        PrimitiveType::S32 => Primitive::S32,
        PrimitiveType::S64 => Primitive::S64,
        PrimitiveType::U8 => Primitive::U8,
        PrimitiveType::U16 => Primitive::U16,
        PrimitiveType::U32 => Primitive::U32,
        PrimitiveType::U64 => Primitive::U64,
        PrimitiveType::F32 => Primitive::F32,
        PrimitiveType::F64 => Primitive::F64,
        PrimitiveType::Bool => Primitive::Bool,
        PrimitiveType::U128 => Primitive::U128,
        PrimitiveType::S128 => Primitive::S128,
        PrimitiveType::Char => Primitive::Char,
        // A `string` is sized as one, never as a primitive.
        PrimitiveType::String => Primitive::Char,
    }
}

impl<L: Lookup> Walk<'_, L> {
    fn size(&mut self, ty: &TypeRef, scope: L::Scope) -> Out {
        match ty {
            TypeRef::Primitive(PrimitiveType::String) => Ok(self.sizer.string(None)),
            TypeRef::Primitive(p) => Ok(Some(self.sizer.scalar(primitive(*p)))),
            TypeRef::Unit => Ok(Some(self.sizer.scalar(Primitive::Unit))),
            TypeRef::Array { element, length, max } => {
                // An array of `char` is a string.
                if **element == TypeRef::Primitive(PrimitiveType::Char) {
                    return Ok(match length {
                        Some(chars) => self.sizer.fixed_string(*chars),
                        None => self.sizer.string(*max),
                    });
                }
                let item = self.size(element, scope)?;
                Ok(match length {
                    Some(len) => self.sizer.fixed_array(item.as_ref(), *len),
                    None => self.sizer.array(item.as_ref(), *max),
                })
            }
            // No official format has a rule for one.
            TypeRef::Union(_) => Err(Unsizable),
            TypeRef::Named(name) => {
                let lookup = self.lookup;
                let resolved = lookup.resolve(scope, name).ok_or(Unsizable)?;
                self.declared(resolved)
            }
        }
    }

    /// `ty` as an optional field's.
    fn optional(&mut self, ty: &TypeRef, scope: L::Scope) -> Out {
        let item = self.size(ty, scope)?;
        Ok(Some(self.sizer.option(item.as_ref())))
    }

    fn declared(&mut self, resolved: Resolved<'_, L::Scope>) -> Out {
        let Resolved { key, scope, decl } = resolved;
        match decl {
            Decl::Opaque => Err(Unsizable),
            Decl::Enum { name, variants } => Ok(self.sizer.enumeration(&name, &variants)),
            Decl::Alias { target, max_depth } => self.enter(key, max_depth, |walk| walk.size(&target, scope)),
            Decl::Struct { name, fields, max_depth } => self.enter(key, max_depth, |walk| {
                let mut sized = Vec::with_capacity(fields.len());
                for field in &fields {
                    let size = if field.optional {
                        walk.optional(&field.ty, scope)?
                    } else {
                        walk.size(&field.ty, scope)?
                    };
                    sized.push((&*field.name, size));
                }
                Ok(walk.sizer.structure(&name, &sized))
            }),
        }
    }

    /// Size what `key` declares, one level deeper, unless it can't go: past
    /// its `@max_depth` no value exists, and with none to end it, it ends
    /// nothing.
    fn enter(&mut self, key: usize, max_depth: Option<u64>, f: impl FnOnce(&mut Self) -> Out) -> Out {
        let level = self.stack.iter().filter(|k| **k == key).count() as u64 + 1;
        match max_depth {
            Some(limit) if level > limit => return Ok(None),
            None if level > 1 => return Ok(Some(self.sizer.cyclic())),
            _ => {}
        }
        if self.stack.len() >= MAX_NESTING {
            return Err(Unsizable);
        }
        let mut active = self.stack.clone();
        active.sort_unstable();
        let at = (key, active);
        if let Some(known) = self.memo.get(&at) {
            return known.clone();
        }
        self.stack.push(key);
        let out = f(self);
        self.stack.pop();
        self.memo.insert(at, out.clone());
        out
    }
}
