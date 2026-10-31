//! The frozen IR's types and values, kept apart.
//!
//! A type is a [`TypeRef`]: structured, so nothing downstream has to parse
//! text to learn that `u8[16]` is a fixed array of 16 `u8`s, or that a field
//! is `f32` and not `f64`. A value written in the schema - a field default,
//! a constant - is a [`Literal`], held next to the type it belongs to rather
//! than folded into it.
//!
//! The `validate { ... }` condition of a validator freezes here too, as a
//! [`Condition`], so a generator can emit the check without re-parsing the
//! condition text.

use std::fmt;

use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "compiler")]
use crate::schema::idl::grammar;

/// One of the language's primitive types. Spelled exactly as
/// [`crate::schema::idl::vocabulary::PRIMITIVES`] spells it, plus `u128` /
/// `s128`: those aren't grammar keywords, but a type written with either
/// name has always been treated as that primitive, and the generators map
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveType {
    S8,
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    String,
    U128,
    S128,
}

impl PrimitiveType {
    pub const ALL: [PrimitiveType; 14] = [
        Self::S8, Self::S16, Self::S32, Self::S64,
        Self::U8, Self::U16, Self::U32, Self::U64,
        Self::F32, Self::F64, Self::Bool, Self::String,
        Self::U128, Self::S128,
    ];

    /// The IDL spelling (`s32`, `string`).
    pub fn name(self) -> &'static str {
        match self {
            Self::S8 => "s8",
            Self::S16 => "s16",
            Self::S32 => "s32",
            Self::S64 => "s64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Bool => "bool",
            Self::String => "string",
            Self::U128 => "u128",
            Self::S128 => "s128",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The inclusive range an integer primitive holds; `None` for the
    /// others.
    pub fn integer_range(self) -> Option<(i128, i128)> {
        Some(match self {
            Self::S8 => (i8::MIN.into(), i8::MAX.into()),
            Self::S16 => (i16::MIN.into(), i16::MAX.into()),
            Self::S32 => (i32::MIN.into(), i32::MAX.into()),
            Self::S64 => (i64::MIN.into(), i64::MAX.into()),
            Self::U8 => (0, u8::MAX.into()),
            Self::U16 => (0, u16::MAX.into()),
            Self::U32 => (0, u32::MAX.into()),
            Self::U64 => (0, u64::MAX.into()),
            Self::U128 => (0, i128::MAX),
            Self::S128 => (i128::MIN, i128::MAX),
            _ => return None,
        })
    }

    pub fn is_integer(self) -> bool {
        self.integer_range().is_some()
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A type as written in the schema.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeRef {
    Primitive(PrimitiveType),
    /// A struct, enum, error or `type` alias, by the name written -
    /// possibly qualified (`types::User`, `std::http::Request`).
    Named(String),
    /// `T[]` (`length: None`) or `T[N]`.
    Array {
        element: Box<TypeRef>,
        length: Option<u64>,
    },
    /// `union(A B ...)`.
    Union(Vec<TypeRef>),
    /// `()` - a reply carrying no value. Distinct from a function with no
    /// return at all (no reply).
    Unit,
}

impl TypeRef {
    /// The type a grammar node spells.
    #[cfg(feature = "compiler")]
    pub fn from_grammar(ty: &grammar::Type) -> Self {
        use grammar::Type as G;
        let primitive = |p| TypeRef::Primitive(p);
        match ty {
            G::S8(_) => primitive(PrimitiveType::S8),
            G::S16(_) => primitive(PrimitiveType::S16),
            G::S32(_) => primitive(PrimitiveType::S32),
            G::S64(_) => primitive(PrimitiveType::S64),
            G::U8(_) => primitive(PrimitiveType::U8),
            G::U16(_) => primitive(PrimitiveType::U16),
            G::U32(_) => primitive(PrimitiveType::U32),
            G::U64(_) => primitive(PrimitiveType::U64),
            G::F32(_) => primitive(PrimitiveType::F32),
            G::F64(_) => primitive(PrimitiveType::F64),
            G::Bool(_) => primitive(PrimitiveType::Bool),
            G::String(_) => primitive(PrimitiveType::String),
            G::Named(id) => {
                let name = id.to_string();
                match PrimitiveType::from_name(&name) {
                    // `u128` / `s128` parse as names; see `PrimitiveType`.
                    Some(p) => primitive(p),
                    None => TypeRef::Named(name),
                }
            }
            G::Array(array) => TypeRef::Array {
                element: Box::new(TypeRef::from_grammar(array.elem_type())),
                // A negative size doesn't parse as an array size; anything
                // else fits.
                length: array.size.as_ref().map(|n| n.value().max(0) as u64),
            },
            G::Union(union) => {
                TypeRef::Union(union.members().iter().map(TypeRef::from_grammar).collect())
            }
            G::Unit(_) => TypeRef::Unit,
        }
    }

    pub fn as_primitive(&self) -> Option<PrimitiveType> {
        match self {
            TypeRef::Primitive(p) => Some(*p),
            _ => None,
        }
    }

    /// The name a `Named` type is written with, through any arrays:
    /// `User` for `User[][]`. `None` for primitives, unions and `()`.
    pub fn base_name(&self) -> Option<&str> {
        match self {
            TypeRef::Named(name) => Some(name),
            TypeRef::Array { element, .. } => element.base_name(),
            _ => None,
        }
    }

    pub fn is_array(&self) -> bool {
        matches!(self, TypeRef::Array { .. })
    }

    /// `self` with every `Named` part replaced by what `resolve` maps it to
    /// (`None` keeps it). Used to see through `type` aliases.
    pub fn map_named(&self, resolve: &mut impl FnMut(&str) -> Option<TypeRef>) -> TypeRef {
        match self {
            TypeRef::Named(name) => resolve(name).unwrap_or_else(|| self.clone()),
            TypeRef::Array { element, length } => TypeRef::Array {
                element: Box::new(element.map_named(resolve)),
                length: *length,
            },
            TypeRef::Union(members) => {
                TypeRef::Union(members.iter().map(|m| m.map_named(resolve)).collect())
            }
            _ => self.clone(),
        }
    }
}

/// The canonical IDL spelling: `u8[16]`, `User[]`, `union(string u32)`, `()`.
impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeRef::Primitive(p) => write!(f, "{p}"),
            TypeRef::Named(name) => f.write_str(name),
            TypeRef::Array { element, length: Some(n) } => write!(f, "{element}[{n}]"),
            TypeRef::Array { element, length: None } => write!(f, "{element}[]"),
            TypeRef::Union(members) => {
                f.write_str("union(")?;
                for (i, member) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{member}")?;
                }
                f.write_str(")")
            }
            TypeRef::Unit => f.write_str("()"),
        }
    }
}

/// A value written in the schema: a field or property default, or a
/// constant's value. Checked against its type by validation, not here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Literal {
    Integer(i64),
    Bool(bool),
    String(String),
    /// A reference to another value - `MAX_NAME`, `u32::MAX`,
    /// `limits::DEFAULT` - as written; not resolved yet.
    Reference(String),
}

impl Literal {
    /// The literal a value expression is, if it's one a field or constant
    /// can hold. An f-string isn't.
    #[cfg(feature = "compiler")]
    pub fn from_grammar(expression: &grammar::Expression) -> Option<Self> {
        use grammar::Expression as E;
        Some(match expression {
            E::Integer(i) => Literal::Integer(i.value()),
            E::Bool(b) => Literal::Bool(matches!(b, grammar::BoolLiteral::True)),
            E::String(s) => Literal::String(s.value().to_string()),
            E::Path(p) => Literal::Reference(p.text.clone()),
            E::Identifier(id) => Literal::Reference(id.text.clone()),
            E::FString(_) => return None,
        })
    }

    /// What kind of literal this is, for messages (`an integer`).
    pub fn describe(&self) -> &'static str {
        match self {
            Literal::Integer(_) => "an integer",
            Literal::Bool(_) => "a boolean",
            Literal::String(_) => "a string",
            Literal::Reference(_) => "a reference",
        }
    }
}

/// As it would be written: `42`, `true`, `"text"`, `MAX_NAME`.
impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Integer(i) => write!(f, "{i}"),
            Literal::Bool(b) => write!(f, "{b}"),
            Literal::String(s) => write!(f, "{s:?}"),
            Literal::Reference(r) => f.write_str(r),
        }
    }
}

/// A validator's `assert` condition: comparisons chained with `and` / `or`.
/// The grammar has no precedence, so this stays a flat chain; a generator
/// groups it left to right.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Condition {
    pub first: Comparison,
    pub rest: Vec<(LogicalOp, Comparison)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Comparison {
    pub left: Operand,
    pub op: CompareOp,
    pub right: Operand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Operand {
    /// `value.length`, `params.min_chars` - the dotted segments.
    Path(Vec<String>),
    Integer(i64),
    String(String),
}

impl Condition {
    #[cfg(feature = "compiler")]
    pub fn from_grammar(condition: &grammar::Condition) -> Self {
        Condition {
            first: Comparison::from_grammar(&condition.first),
            rest: condition
                .rest
                .iter()
                .map(|tail| {
                    let op = match tail.op {
                        grammar::LogicalOp::And => LogicalOp::And,
                        grammar::LogicalOp::Or => LogicalOp::Or,
                    };
                    (op, Comparison::from_grammar(&tail.comparison))
                })
                .collect(),
        }
    }
}

impl Comparison {
    #[cfg(feature = "compiler")]
    fn from_grammar(comparison: &grammar::Comparison) -> Self {
        use grammar::CompareOp as G;
        Comparison {
            left: Operand::from_grammar(&comparison.left),
            op: match comparison.op {
                G::Eq => CompareOp::Eq,
                G::Ne => CompareOp::Ne,
                G::Ge => CompareOp::Ge,
                G::Le => CompareOp::Le,
                G::Gt => CompareOp::Gt,
                G::Lt => CompareOp::Lt,
            },
            right: Operand::from_grammar(&comparison.right),
        }
    }
}

impl Operand {
    #[cfg(feature = "compiler")]
    fn from_grammar(operand: &grammar::Operand) -> Self {
        match operand {
            grammar::Operand::Path(path) => {
                Operand::Path(path.joined().split('.').map(str::to_string).collect())
            }
            grammar::Operand::Integer(i) => Operand::Integer(i.value()),
            grammar::Operand::Str(s) => Operand::String(s.value().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "compiler")]
    fn primitive_names_match_the_vocabulary() {
        let vocabulary: Vec<&str> =
            crate::schema::idl::vocabulary::PRIMITIVES.iter().map(|p| p.name).collect();
        let ours: Vec<&str> = PrimitiveType::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(ours[..vocabulary.len()], vocabulary);
        assert_eq!(ours[vocabulary.len()..], ["u128", "s128"]);
    }

    #[test]
    fn displays_the_idl_spelling() {
        let u8_16 = TypeRef::Array {
            element: Box::new(TypeRef::Primitive(PrimitiveType::U8)),
            length: Some(16),
        };
        assert_eq!(u8_16.to_string(), "u8[16]");
        let grid = TypeRef::Array {
            element: Box::new(TypeRef::Array {
                element: Box::new(TypeRef::Named("Cell".into())),
                length: None,
            }),
            length: None,
        };
        assert_eq!(grid.to_string(), "Cell[][]");
        assert_eq!(grid.base_name(), Some("Cell"));
        let either = TypeRef::Union(vec![
            TypeRef::Primitive(PrimitiveType::String),
            TypeRef::Primitive(PrimitiveType::F32),
        ]);
        assert_eq!(either.to_string(), "union(string f32)");
    }
}
