//! What a reference to a constant stands for (core#168).
//!
//! A value written for a primitive can name a constant (`MAX`,
//! `limits::MAX`) or an integer type's bound (`u32::MAX`) instead of
//! spelling out a literal. That value can be a constant's own value, a
//! field's or a validator property's default, or a validator call's
//! argument. The IR keeps such a reference as written; this resolves it to
//! the value it stands for, so the build judges it as it would a literal. A
//! name that isn't a constant is an error, and so is a value of the wrong
//! kind or one the declared type can't hold.

use std::collections::HashMap;

use super::symbols::{SymbolTable, SymbolType};
use super::validator::ImportLookup;
use crate::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use crate::schema::ir::frozen::unit::FrozenUnit;

/// How many references in a row are followed before giving up: further than
/// any schema chains its constants, and a cycle is caught well before.
const MAX_CHAIN: usize = 64;

/// The word a field's default can be written as for its type's own default.
/// Not a constant's value: a constant states its value.
const DEFAULT: &str = "default";

/// A constant's value, once every reference on the way is followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstantValue {
    Integer(i128),
    Bool(bool),
    String(String),
}

impl std::fmt::Display for ConstantValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstantValue::Integer(i) => write!(f, "{i}"),
            ConstantValue::Bool(b) => write!(f, "{b}"),
            ConstantValue::String(s) => write!(f, "{s:?}"),
        }
    }
}

/// A resolved reference: the primitive the value is declared as (a bound's
/// own type, for `u32::MAX`), and the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConstant {
    pub ty: PrimitiveType,
    pub value: ConstantValue,
}

/// The value `literal` stands for as a `ty`, when it is one: the right kind,
/// and an integer `ty` can hold. `None` for anything else, including a
/// reference, which only a resolver can follow.
pub fn literal_value(ty: PrimitiveType, literal: &Literal) -> Option<ConstantValue> {
    match literal {
        Literal::Integer(i) if ty.is_integer() => {
            let (min, max) = ty.integer_range()?;
            let i = i128::from(*i);
            (min..=max).contains(&i).then_some(ConstantValue::Integer(i))
        }
        Literal::Integer(i) if ty.is_float() => Some(ConstantValue::Integer(i128::from(*i))),
        Literal::Bool(b) if ty == PrimitiveType::Bool => Some(ConstantValue::Bool(*b)),
        Literal::String(s) if ty == PrimitiveType::String => Some(ConstantValue::String(s.clone())),
        _ => None,
    }
}

/// `ty::bound` for a primitive `ty`: `Some(Ok(_))` for an integer's `MIN` or
/// `MAX`, `Some(Err(_))` for any other bound on a primitive (`f32::MAX`,
/// `u32::LIMIT`), and `None` when `reference` isn't written that way.
pub fn primitive_bound(reference: &str) -> Option<Result<ResolvedConstant, Problem>> {
    let (ty, bound) = reference.split_once("::")?;
    let ty = PrimitiveType::from_name(ty)?;
    let range = ty.integer_range().filter(|_| matches!(bound, "MIN" | "MAX"));
    Some(match range {
        Some((min, max)) => Ok(ResolvedConstant {
            ty,
            value: ConstantValue::Integer(if bound == "MIN" { min } else { max }),
        }),
        None => Err(Problem::NoBound { ty, bound: bound.to_string() }),
    })
}

/// What a reference turned out to be.
pub enum Resolution {
    Value(ResolvedConstant),
    /// Nothing to judge against: a constant whose own value is wrong (that
    /// is reported where it's declared), one that only another schema can
    /// answer for when the lookup can't, or a chain that loops through
    /// other constants (reported at each of them).
    Unjudged,
    Problem(Problem),
}

/// Why a reference stands for no value.
pub enum Problem {
    /// Nothing by that name is declared or imported.
    Unknown { suggestion: Option<String> },
    /// The name is declared, as something else.
    NotAConstant(SymbolType),
    /// A primitive's bound that doesn't exist: `f32::MAX`, `u32::LIMIT`.
    NoBound { ty: PrimitiveType, bound: String },
    /// The chain comes back to where it started: `A`, `B`, `A`.
    Cycle(Vec<String>),
}

/// The constants one schema can name: its own, and through `imports`, the
/// ones it brings into scope.
pub struct Constants<'a> {
    local: HashMap<&'a str, (&'a TypeRef, Option<&'a Literal>)>,
    namespace: Option<&'a str>,
    symbols: &'a SymbolTable<'a>,
    imports: &'a dyn ImportLookup,
}

impl<'a> Constants<'a> {
    pub fn new(
        units: &'a [FrozenUnit],
        namespace: Option<&'a str>,
        symbols: &'a SymbolTable<'a>,
        imports: &'a dyn ImportLookup,
    ) -> Self {
        let local = units
            .iter()
            .filter_map(|unit| match unit {
                FrozenUnit::Constant { name, ty, value, .. } => {
                    Some((name.as_str(), (ty, value.as_ref())))
                }
                _ => None,
            })
            .collect();
        Constants { local, namespace, symbols, imports }
    }

    /// What `reference` stands for, read from anywhere in this schema but a
    /// constant's own value. A chain that loops is reported at the
    /// constants on the loop, not again here.
    ///
    /// `default` is the type's own default (`0`, `false`, `""`, an enum's
    /// first variant), which every type has, so there's nothing to judge.
    pub fn resolve(&self, reference: &str) -> Resolution {
        if reference == DEFAULT {
            return Resolution::Unjudged;
        }
        match self.follow(reference, &mut vec![]) {
            Resolution::Problem(Problem::Cycle(_)) => Resolution::Unjudged,
            other => other,
        }
    }

    /// What constant `name`'s own `reference` stands for. A chain that comes
    /// back to `name` is a problem here, and only here: a chain that loops
    /// somewhere further on is reported at the constants on that loop.
    pub fn resolve_from(&self, name: &str, reference: &str) -> Resolution {
        let mut visiting = vec![name.to_string()];
        match self.follow(reference, &mut visiting) {
            Resolution::Problem(Problem::Cycle(chain)) if chain.first().map(String::as_str) != Some(name) => {
                Resolution::Unjudged
            }
            other => other,
        }
    }

    fn follow(&self, reference: &str, visiting: &mut Vec<String>) -> Resolution {
        if let Some(bound) = primitive_bound(reference) {
            return match bound {
                Ok(value) => Resolution::Value(value),
                Err(problem) => Resolution::Problem(problem),
            };
        }
        // A schema naming its own constant by its full path.
        let name = self
            .namespace
            .and_then(|ns| reference.strip_prefix(ns)?.strip_prefix("::"))
            .filter(|rest| self.local.contains_key(rest))
            .unwrap_or(reference);
        if let Some((ty, value)) = self.local.get(name) {
            if let Some(start) = visiting.iter().position(|v| v == name) {
                let mut chain = visiting[start..].to_vec();
                chain.push(name.to_string());
                return Resolution::Problem(Problem::Cycle(chain));
            }
            if visiting.len() >= MAX_CHAIN {
                return Resolution::Unjudged;
            }
            let (Some(ty), Some(value)) = (ty.as_primitive(), value) else {
                return Resolution::Unjudged;
            };
            return match value {
                Literal::Reference(next) => {
                    visiting.push(name.to_string());
                    let resolved = self.follow(next, visiting);
                    visiting.pop();
                    match resolved {
                        // Read as this constant's own type, which it has to
                        // fit (that's checked where it's declared).
                        Resolution::Value(r) if fits(ty, &r) => {
                            Resolution::Value(ResolvedConstant { ty, value: r.value })
                        }
                        Resolution::Value(_) => Resolution::Unjudged,
                        // A missing name is reported where it's written.
                        Resolution::Problem(Problem::Cycle(chain)) => {
                            Resolution::Problem(Problem::Cycle(chain))
                        }
                        Resolution::Problem(_) | Resolution::Unjudged => Resolution::Unjudged,
                    }
                }
                literal => match literal_value(ty, literal) {
                    Some(value) => Resolution::Value(ResolvedConstant { ty, value }),
                    None => Resolution::Unjudged,
                },
            };
        }
        match self.symbols.get(name) {
            Some(kind) if kind != SymbolType::Import && kind != SymbolType::Constant => {
                return Resolution::Problem(Problem::NotAConstant(kind));
            }
            _ => {}
        }
        if super::validator::could_be_imported(self.symbols, name) {
            return match self.imports.constant(name) {
                Some(value) => Resolution::Value(value),
                None => Resolution::Unjudged,
            };
        }
        let suggestion = crate::utils::text::closest(name, self.local.keys().copied());
        Resolution::Problem(Problem::Unknown { suggestion })
    }
}

/// Whether `value` can be written for a `ty`: the same kind (an integer is
/// also a fine float) and, for an integer `ty`, in its range.
pub fn fits(ty: PrimitiveType, value: &ResolvedConstant) -> bool {
    kind_fits(ty, value.ty) && in_range(ty, &value.value)
}

/// Whether a value declared as `source` is the kind of value a `target`
/// holds. An integer is also a fine float; a float isn't an integer.
pub fn kind_fits(target: PrimitiveType, source: PrimitiveType) -> bool {
    if target.is_integer() {
        source.is_integer()
    } else if target.is_float() {
        source.is_integer() || source.is_float()
    } else {
        target == source
    }
}

/// Whether an integer `value` is one `ty` can hold. Anything else is.
pub fn in_range(ty: PrimitiveType, value: &ConstantValue) -> bool {
    match (ty.integer_range(), value) {
        (Some((min, max)), ConstantValue::Integer(i)) => (min..=max).contains(i),
        _ => true,
    }
}
