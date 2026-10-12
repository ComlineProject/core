// Standard Uses

// Crate Uses
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;

// External Uses
use serde_derive::{Serialize, Deserialize};


pub type FrozenContextWhole = (SchemaContext, Vec<FrozenUnit>);


// TODO: A lot of string instances could be &'a str, boxes could also get ditched, etc
/// Persisted into `.comline/` as CBOR (`package::build::cas::storage::to_cbor`/
/// `from_cbor`), which tags both this enum's own variants and every nested
/// struct's fields by name, not declaration-order position - so *reordering*
/// a variant or field here is safe without touching anything else, and so is
/// adding one that carries `#[serde(default)]`.
///
/// Adding a field *without* a default is not free, despite what the name
/// tagging suggests: an old blob simply has no entry for it, and serde fails
/// the whole decode rather than skipping it. That case needs
/// `package::build::cas::object_store::CURRENT_IR_FORMAT_VERSION` bumped by
/// hand, same as *renaming* or *removing* an existing field - the bump makes
/// any `.comline/` built under the old shape get wiped and rebuilt instead of
/// misread.
#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum FrozenUnit {
    // TODO: Are Tags really necessary anymore since we hash Frozen Units by blob, trees and commits?
    //       Tag here means the same tag concept that CapNProto has
    // Tag(String),
    Namespace(String),
    Name(String),
    // Span is included in the hash/CAS identity deliberately: two schemas
    // that only differ in formatting/position should not be considered
    // content-identical.
    //
    // `(resolved path, optional local alias, span)`. The alias is the `X` in
    // `use ns::Name as X`; `None` for a plain `use` (the bare name it binds is
    // then the path's trailing segment).
    Import(String, Option<String>, (usize, usize)),
    Constant {
        docstring: Option<String>,
        name: String,
        kind_value: KindValue,
        span: (usize, usize),
    },
    /// One `@key` / `@key = value` annotation. `value` keeps the shape the
    /// author wrote (see [`ConfigValue`]); it used to be a flat
    /// `Option<String>` of already-rendered text, which could not tell
    /// `@framing = "datagram"` from `@framing = datagram`.
    Property {
        name: String,
        value: ConfigValue
    },
    /// A validator's `validate { ... }` block: the parsed asserts.
    ExpressionBlock {
        asserts: Vec<FrozenUnit>
    },
    /// One `assert(condition, message)` from a `validate` block.
    Assert {
        /// Canonical condition text, e.g. `value.length >= params.min_chars`.
        condition: String,
        /// The message template, with `{path}` placeholders intact.
        message: String,
        /// Every `root.seg...` member path used in the condition (`value.*`,
        /// `params.*`, ...), for reference checking. Literals are not listed.
        references: Vec<String>,
    },
    //
    Enum {
        docstring: Option<String>,
        name: String,
        variants: Vec<FrozenUnit>,
        span: (usize, usize),
    },
    EnumVariant(KindValue, (usize, usize)),
    /// `name: None` = an unnamed block, applying implicitly to the whole
    /// schema file; `Some(name)` = a reusable preset, opted into
    /// per-declaration via `@settings = name`. `values` is the dotted-key
    /// desugared tree (`crate::settings::desugar`). `spans` is a parallel,
    /// flat, dotted-path-keyed sidecar recording where each leaf was
    /// actually written - diagnostics-only, not part of `values`' own
    /// shape, so enforcement errors about a malformed leaf can point at its
    /// real origin instead of wherever it happened to be used.
    Settings {
        docstring: Option<String>,
        name: Option<String>,
        values: crate::settings::SettingsDict,
        spans: crate::settings::SettingsSpans,
    },
    Struct {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        name: String,
        fields: Vec<FrozenUnit>,
        span: (usize, usize),
    },
    Protocol {
        docstring: String,
        parameters: Vec<FrozenUnit>,
        name: String,
        functions: Vec<FrozenUnit>,
        span: (usize, usize),
    },
    Function {
        docstring: String,
        // `@key = value` function annotations (per-call settings: `@timeout_ms`,
        // `@idempotent`, ...) as `Property { name, expression }`, same shape as
        // `Protocol` / `Struct` carry. Open namespace - a consumer acts on the
        // keys it knows and ignores the rest.
        parameters: Vec<FrozenUnit>,
        name: String,
        // direction: Box<FrozenUnit>,
        arguments: Vec<FrozenArgument>,
        _return: Option<KindValue>,
        // The schema-global ordinal of each `error` this function can throw -
        // resolved at freeze from the `! Name` reference to the matching
        // `FrozenUnit::Error`'s `ordinal` (local or a re-exported import; see
        // `Error::ordinal`). An unresolvable name still gets a stable slot.
        throws: Vec<u16>,
        span: (usize, usize),
    },
    Error {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        /// Schema-global error ordinal - this error's slot in the schema's
        /// error space. Locally-declared errors take `0..N` in declaration
        /// order; an imported error named by a `throws` gets the next slot as
        /// a re-export. This is the `u16` that travels in the response
        /// envelope. Append-only: an `error` is retired in place, its ordinal
        /// never reordered or reused.
        ordinal: u16,
        /// `Some(ns)` when this unit is a re-export slot for an `error`
        /// declared in another schema (`ns` = that schema's namespace, or
        /// `<unresolved: Name>` if it could not be located); `None` for a
        /// local `error` declaration.
        imported_from: Option<String>,
        name: String,
        message: String,
        fields: Vec<FrozenUnit>,
        /// Where this `error` was written, when it was written *here*.
        ///
        /// `None` for a re-export slot (`imported_from: Some(_)`): that
        /// declaration lives in another schema, so its span indexes another
        /// file's source and underlining it would highlight an arbitrary
        /// byte range of the wrong document. Also `None` for the
        /// `<unresolved: Name>` placeholder a `throws` synthesises, which
        /// was never written anywhere.
        #[serde(default)]
        span: Option<(usize, usize)>,
    },
    Validator {
        docstring: Option<String>,
        properties: Vec<FrozenUnit>,
        name: String,
        expression_block: Box<FrozenUnit>,
        /// Always present - a `validator` is only ever frozen from a local
        /// declaration, never re-exported the way an `error` is.
        #[serde(default)]
        span: (usize, usize),
    },
    /// A validator applied to a field via `@validators = [Name(k = v, ...)]`.
    /// One per call in the list; `args` holds a `Property { name, expression }`
    /// per keyword argument.
    ValidatorRef {
        name: String,
        args: Vec<FrozenUnit>,
    },
    Field {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        optional: bool,
        name: String,
        kind_value: KindValue,
        span: (usize, usize),
    },
    /// A `type NAME = TARGET` declaration - transparent like Rust's own
    /// `type` (not a newtype): `target` is frozen exactly as written (by
    /// name, one level - an alias-of-alias's own target stays a `Named`
    /// reference, not flattened), the same way a struct/enum reference
    /// freezes. A reference to this alias elsewhere (a field, an argument,
    /// ...) freezes to the *same* `KindValue::Namespaced(name, _)` a
    /// struct/enum reference would, so generators need only one new case -
    /// emitting the alias declaration itself (`pub type NAME = ...;`,
    /// `export type NAME = ...;`) - to pick it up correctly; no other
    /// codegen path changes.
    TypeAlias {
        docstring: Option<String>,
        name: String,
        target: KindValue,
        span: (usize, usize),
    },
    /// `@deprecated`, frozen with its *semantics* rather than its syntax -
    /// is-deprecated plus an optional message, which is what
    /// `deprecation_of`'s consumers want. [`ConfigValue`] now carries
    /// every annotation's written shape faithfully, so this variant is no
    /// longer the only typed one; it stays because the semantic view is a
    /// different thing from the syntactic one.
    Deprecated(DeprecatedValue),
}

/// What an `@key = value` annotation was written as, preserved rather than
/// rendered to text.
///
/// `Property` used to carry `Option<String>` produced by
/// `AnnotationValue::render`, which strips quotes - so a frozen schema
/// could not distinguish `@framing = "datagram"` from
/// `@framing = datagram`. That is why validator-argument checking had to
/// re-parse text to guess a type, and why it only ever recognised three
/// categories.
///
/// Keeping the written form means typing is a *validation* question
/// answered against a registry, not a representation question - which is
/// what lets `@key` stay an open namespace (an unrecognised annotation
/// freezes in whatever shape it was written, with no registry entry
/// needed).
///
/// One variant per [`crate::schema::idl::grammar::Expression`] arm, plus
/// [`ConfigValue::Marker`] for a bare `@key` with no `= value`. A list
/// (`@validators = [...]`) is not here: it freezes as sibling
/// [`FrozenUnit::ValidatorRef`] units, unchanged.
#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ConfigValue {
    /// A bare `@key`, no `= value`.
    Marker,
    Bool(bool),
    Integer(i64),
    /// A quoted string, quotes stripped.
    Str(String),
    /// A bare identifier - the distinction from [`ConfigValue::Str`] that
    /// the old flat text could not represent.
    Name(String),
    /// A `::`-separated path.
    Path(String),
    /// An f-string, with its `{...}` placeholders intact.
    FString(String),
}

impl ConfigValue {
    /// This value's syntactic form, for [`crate::config_value::check`].
    pub fn form(&self) -> crate::config_value::ValueForm<'_> {
        use crate::config_value::ValueForm;
        match self {
            ConfigValue::Marker => ValueForm::Marker,
            ConfigValue::Bool(b) => ValueForm::Bool(*b),
            ConfigValue::Integer(i) => ValueForm::Integer(*i),
            ConfigValue::Str(s) => ValueForm::Str(s),
            ConfigValue::Name(n) => ValueForm::Name(n),
            ConfigValue::Path(p) => ValueForm::Path(p),
            ConfigValue::FString(s) => ValueForm::FString(s),
        }
    }

    /// The flat text this value used to freeze as - `None` for a bare
    /// marker, and quotes already stripped for a string, byte-for-byte
    /// what `AnnotationValue::render` produced for a scalar.
    ///
    /// Kept so a consumer that genuinely only wants the text (a generator
    /// interpolating it into output, or `@timeout_ms`'s `parse::<u64>()`)
    /// needs no change. A consumer that cares *which kind* of value it got
    /// should match the variants instead - that distinction is the whole
    /// point of this type.
    ///
    /// `Cow` because a numeric or boolean value has no stored text to
    /// borrow: it is rendered here rather than stored twice.
    pub fn as_text(&self) -> Option<std::borrow::Cow<'_, str>> {
        use std::borrow::Cow;
        match self {
            ConfigValue::Marker => None,
            ConfigValue::Str(s)
            | ConfigValue::Name(s)
            | ConfigValue::Path(s)
            | ConfigValue::FString(s) => Some(Cow::Borrowed(s)),
            ConfigValue::Bool(b) => Some(Cow::Owned(b.to_string())),
            ConfigValue::Integer(i) => Some(Cow::Owned(i.to_string())),
        }
    }

    /// Whether this is a bare marker with no value.
    pub fn is_marker(&self) -> bool {
        matches!(self, ConfigValue::Marker)
    }
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum DeprecatedValue {
    False,
    True,
    Message(String),
    /// Not a bool literal or a plain string - a bare identifier (any
    /// casing, including a mistyped `True`/`False`), a `::`-path, an
    /// f-string, an integer, or a list. Carries the rendered text purely
    /// for the diagnostic `deprecation_of` (`validator.rs`) raises for
    /// it.
    Malformed(String),
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct  FrozenArgument {
    pub name: String,
    pub kind: KindValue,
    pub span: (usize, usize),
}


pub fn schema_namespace(frozen: &[FrozenUnit]) -> Option<&str> {
    for unit in frozen {
        if let FrozenUnit::Namespace(name) = unit {
            return Some(name)
        }
    }

    None
}

pub fn schema_namespace_as_path(frozen: &[FrozenUnit]) -> Option<String> {
    let Some(namespace) = schema_namespace(frozen) else {
        return None
    };

    // TODO: Since the variant FrozenUnit::Name was added, a split is not necessary anymore
    Some(namespace.split("::").collect::<Vec<_>>().join("/"))
}
