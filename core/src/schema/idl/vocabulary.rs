//! A description of the surface syntax `grammar.rs` defines — every
//! primitive type and every reserved word — for consumers that need to
//! *talk about* the language (an editor's completions and syntax
//! highlighting, docs tooling) rather than parse it. Not a second parser,
//! and not enforcement: same spirit as [`size`](super::size), which
//! already hosts consumer-facing metadata core itself doesn't act on.
//!
//! This is the canonical copy. `validator.rs`'s own primitive-name list,
//! `diagnostics.rs`'s "valid primitive types" hint, and `size.rs`'s test
//! table all converge onto [`PRIMITIVES`] rather than keeping their own.
//! One deliberate exception: [`crate::schema::ir::compiler::interpreter::incremental::type_to_string`]
//! keeps its own `f32`/`f64` → `"float"` collapse — that string feeds
//! `schema_ir_hash`, so canonicalizing it would change every frozen
//! schema's hash. Not touched here.

use super::grammar::Type;

/// Which family a primitive type belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveClass {
    Signed,
    Unsigned,
    Float,
    Bool,
    Text,
}

/// One primitive type keyword.
#[derive(Debug, Clone, Copy)]
pub struct PrimitiveInfo {
    pub name: &'static str,
    pub class: PrimitiveClass,
    /// Bit width for a fixed-size primitive (including `bool`, sized at 8
    /// — `size_of_type` packs it with `s8`/`u8`); `None` only for `str`/
    /// `string` (genuinely unbounded).
    pub bits: Option<u16>,
    pub description: &'static str,
}

/// Every primitive type keyword the grammar defines, in the order
/// `Type`'s own variants list them. Exactly 13 — `s8 s16 s32 s64 u8 u16
/// u32 u64 f32 f64 bool str string`. No `i8`/`i16`/`i32`/`i64` — those
/// are not Comline syntax (signed integers are `s`-prefixed).
pub const PRIMITIVES: &[PrimitiveInfo] = &[
    PrimitiveInfo { name: "s8", class: PrimitiveClass::Signed, bits: Some(8), description: "8-bit signed integer" },
    PrimitiveInfo { name: "s16", class: PrimitiveClass::Signed, bits: Some(16), description: "16-bit signed integer" },
    PrimitiveInfo { name: "s32", class: PrimitiveClass::Signed, bits: Some(32), description: "32-bit signed integer" },
    PrimitiveInfo { name: "s64", class: PrimitiveClass::Signed, bits: Some(64), description: "64-bit signed integer" },
    PrimitiveInfo { name: "u8", class: PrimitiveClass::Unsigned, bits: Some(8), description: "8-bit unsigned integer" },
    PrimitiveInfo { name: "u16", class: PrimitiveClass::Unsigned, bits: Some(16), description: "16-bit unsigned integer" },
    PrimitiveInfo { name: "u32", class: PrimitiveClass::Unsigned, bits: Some(32), description: "32-bit unsigned integer" },
    PrimitiveInfo { name: "u64", class: PrimitiveClass::Unsigned, bits: Some(64), description: "64-bit unsigned integer" },
    PrimitiveInfo { name: "f32", class: PrimitiveClass::Float, bits: Some(32), description: "32-bit floating point" },
    PrimitiveInfo { name: "f64", class: PrimitiveClass::Float, bits: Some(64), description: "64-bit floating point" },
    PrimitiveInfo { name: "bool", class: PrimitiveClass::Bool, bits: Some(8), description: "boolean" },
    PrimitiveInfo { name: "str", class: PrimitiveClass::Text, bits: None, description: "string — the canonical spelling" },
    PrimitiveInfo { name: "string", class: PrimitiveClass::Text, bits: None, description: "string — an accepted synonym for `str`" },
];

/// Which syntactic role a reserved word plays — not load-bearing for
/// parsing, just a grouping for presenting the vocabulary (e.g. completion
/// only offers `Declaration` keywords at top level).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
    /// Introduces a top-level or nested declaration.
    Declaration,
    /// Introduces a member inside a declaration's body.
    Member,
    /// A modifier on a field.
    Modifier,
    /// A `use`-path prefix.
    PathPrefix,
    /// A word-spelled operator in a validator condition.
    WordOperator,
    /// A literal value, valid only in a `settings` value position.
    Literal,
    /// Introduces an inline type expression.
    TypeConstructor,
    Other,
}

/// One reserved word.
#[derive(Debug, Clone, Copy)]
pub struct KeywordInfo {
    pub text: &'static str,
    pub kind: KeywordKind,
    pub description: &'static str,
    /// A minimal, complete, valid schema that uses this keyword — not
    /// documentation prose, an actual fixture: the drift-guard test parses
    /// every one of these and asserts success.
    pub example: &'static str,
    /// `true` when the word is reserved only in one specific position —
    /// today just `message`, which is a keyword solely in
    /// `error { message = ... }`; a struct/error field may be named
    /// `message` like any other identifier. A highlighter should gate a
    /// contextual keyword on its position, not color every occurrence.
    pub contextual: bool,
}

/// Every reserved word the grammar defines. Exactly 24 — deliberately
/// excludes `bool`/`str`/`string`, which are reserved too but belong to
/// [`PRIMITIVES`], not here.
pub const KEYWORDS: &[KeywordInfo] = &[
    KeywordInfo { text: "struct", kind: KeywordKind::Declaration, description: "A data shape — named, typed fields.", example: "struct S {\n    f: str\n}", contextual: false },
    KeywordInfo { text: "enum", kind: KeywordKind::Declaration, description: "A closed set of named variants.", example: "enum E {\n    A\n}", contextual: false },
    KeywordInfo { text: "protocol", kind: KeywordKind::Declaration, description: "A set of callable functions.", example: "protocol P {\n    function f();\n}", contextual: false },
    KeywordInfo { text: "error", kind: KeywordKind::Declaration, description: "A named failure a function can raise, with an interpolated message.", example: "error E {\n    message = \"x\"\n}", contextual: false },
    KeywordInfo { text: "const", kind: KeywordKind::Declaration, description: "A compile-time constant.", example: "const N: u8 = 1", contextual: false },
    KeywordInfo { text: "type", kind: KeywordKind::Declaration, description: "A transparent alias for another type.", example: "type T = u8", contextual: false },
    KeywordInfo { text: "settings", kind: KeywordKind::Declaration, description: "Schema-wide switches — parsed, not yet enforced.", example: "settings S {\n    k = True\n}", contextual: false },
    KeywordInfo { text: "validator", kind: KeywordKind::Declaration, description: "A named, parameterised field check.", example: "validator V {\n    validate {\n        assert(1 == 1, \"x\")\n    }\n}", contextual: false },
    KeywordInfo { text: "use", kind: KeywordKind::Declaration, description: "Pull declarations in from another schema or package.", example: "use pkg::Type", contextual: false },
    KeywordInfo { text: "import", kind: KeywordKind::Declaration, description: "The older single-item import form, still accepted.", example: "import pkg::Type", contextual: false },
    KeywordInfo { text: "function", kind: KeywordKind::Member, description: "A callable operation inside a protocol.", example: "protocol P {\n    function f();\n}", contextual: false },
    KeywordInfo { text: "validate", kind: KeywordKind::Member, description: "Introduces a validator's block of `assert(...)` checks.", example: "validator V {\n    validate {\n        assert(1 == 1, \"x\")\n    }\n}", contextual: false },
    KeywordInfo { text: "message", kind: KeywordKind::Member, description: "An error's required, interpolated failure text.", example: "error E {\n    message = \"x\"\n}", contextual: true },
    KeywordInfo { text: "optional", kind: KeywordKind::Modifier, description: "Marks a struct/error field as not required.", example: "struct S {\n    optional f: str\n}", contextual: false },
    KeywordInfo { text: "self", kind: KeywordKind::PathPrefix, description: "In a `use` path, the current schema.", example: "use self::Type", contextual: false },
    KeywordInfo { text: "parent", kind: KeywordKind::PathPrefix, description: "In a `use` path, one namespace level up.", example: "use parent::Type", contextual: false },
    KeywordInfo { text: "crate", kind: KeywordKind::PathPrefix, description: "In a `use` path, the package root.", example: "use crate::Type", contextual: false },
    KeywordInfo { text: "and", kind: KeywordKind::WordOperator, description: "Joins two comparisons in a validator's `assert` condition.", example: "validator V {\n    validate {\n        assert(1 == 1 and 2 == 2, \"x\")\n    }\n}", contextual: false },
    KeywordInfo { text: "or", kind: KeywordKind::WordOperator, description: "Joins two comparisons in a validator's `assert` condition.", example: "validator V {\n    validate {\n        assert(1 == 1 or 2 == 3, \"x\")\n    }\n}", contextual: false },
    KeywordInfo { text: "True", kind: KeywordKind::Literal, description: "A boolean literal, valid only as a `settings` value.", example: "settings S {\n    k = True\n}", contextual: false },
    KeywordInfo { text: "False", kind: KeywordKind::Literal, description: "A boolean literal, valid only as a `settings` value.", example: "settings S {\n    k = False\n}", contextual: false },
    KeywordInfo { text: "union", kind: KeywordKind::TypeConstructor, description: "An inline list of alternative types for one field.", example: "struct S {\n    f: union(u8 str)\n}", contextual: false },
    KeywordInfo { text: "as", kind: KeywordKind::Other, description: "Binds an import under a new local name.", example: "use pkg::Type as T", contextual: false },
    KeywordInfo { text: "assert", kind: KeywordKind::Other, description: "One check inside a validator's `validate` block.", example: "validator V {\n    validate {\n        assert(1 == 1, \"x\")\n    }\n}", contextual: false },
];

/// Look up a primitive by its surface name.
pub fn primitive(name: &str) -> Option<&'static PrimitiveInfo> {
    PRIMITIVES.iter().find(|p| p.name == name)
}

/// Look up a reserved word by its surface text.
pub fn keyword(text: &str) -> Option<&'static KeywordInfo> {
    KEYWORDS.iter().find(|k| k.text == text)
}

/// Every keyword of one kind, in table order.
pub fn keywords_of_kind(kind: KeywordKind) -> impl Iterator<Item = &'static KeywordInfo> {
    KEYWORDS.iter().filter(move |k| k.kind == kind)
}

/// The surface spelling of a primitive `Type` variant — `None` for
/// `Named`/`Array`/`Union`/`Unit`. Deliberately not used by the IR
/// freezer; see the module doc on `type_to_string`.
pub fn primitive_name_of(ty: &Type) -> Option<&'static str> {
    let name = match ty {
        Type::S8(_) => "s8",
        Type::S16(_) => "s16",
        Type::S32(_) => "s32",
        Type::S64(_) => "s64",
        Type::U8(_) => "u8",
        Type::U16(_) => "u16",
        Type::U32(_) => "u32",
        Type::U64(_) => "u64",
        Type::F32(_) => "f32",
        Type::F64(_) => "f64",
        Type::Bool(_) => "bool",
        Type::Str(_) => "str",
        Type::String(_) => "string",
        _ => return None,
    };
    Some(name)
}

/// Every identifier-shaped leaf the grammar defines: every keyword's text,
/// plus every primitive's name. Exists for the drift-guard test, which
/// cross-checks this set against the grammar source directly.
pub fn all_word_tokens() -> impl Iterator<Item = &'static str> {
    KEYWORDS.iter().map(|k| k.text).chain(PRIMITIVES.iter().map(|p| p.name))
}
