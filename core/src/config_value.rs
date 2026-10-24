//! The shared vocabulary for *configuration* values: `settings` keys'
//! values, `@annotation` values, and a validator call's arguments. All
//! three let a user write `key = value` against a shape core is supposed
//! to know, and before this module each of them re-derived "what type does
//! this key take" at its own use site, with its own error prose.
//!
//! Deliberately **not** named `typing`: this is about values *configuring*
//! a schema element, not about the schema type system ([`TypeRef`] and
//! the grammar's `Type`) that describes wire data. The two never mix - a
//! `settings` value is never a `u32`, and a struct field's type is never a
//! `Keyword`.
//!
//! Two halves that must not be confused:
//!
//! - [`ValueType`] is what a key *accepts*, declared once in a registry
//!   ([`crate::settings::keys`] for settings keys,
//!   [`crate::schema::idl::annotations`] for annotations).
//! - [`ValueForm`] is what a user *wrote*, read back off whichever surface
//!   it came from. Each surface keeps its own value enum and exposes a
//!   `form()` - so one [`check`] serves all three without merging
//!   `SettingsValue` and the IR's own annotation value into one union
//!   carrying variants that are meaningless in half their uses.
//!
//! [`TypeRef`]: crate::schema::ir::frozen::types::TypeRef

use std::fmt;

/// What a configuration key accepts. Declared as data in a registry, so a
/// compiler pass, a code generator, and the editor all read one answer
/// instead of three hand-written ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// A bare marker with no `= value` at all (`@idempotent`).
    Marker,
    Bool,
    Integer,
    /// Any quoted string.
    Str,
    /// A quoted string from a closed set, with aliases folded to one
    /// canonical spelling (`@framing`'s `json-rpc` -> `jsonrpc`). The one
    /// place that alias table lives - see [`canonicalize`].
    StrEnum(&'static [StrVariant]),
    /// A *bare* keyword, not a quoted string (`settings`' `mode = replace`).
    /// Distinct from [`ValueType::Str`] on purpose: `mode = "replace"` is
    /// wrong, and only a form-level distinction can say so.
    Keyword(&'static [&'static str]),
    /// A bare identifier naming something declared elsewhere
    /// (`@settings = Strict`).
    Name,
    /// A boolean *or* a string message - `@deprecated`'s shape, where both
    /// `= true` and `= "use NewThing instead"` are meaningful.
    BoolOrStr,
    /// A list of validator calls (`@validators = [StringBounds(min = 3)]`).
    ValidatorList,
    /// Any one of these (`@framing`: a name, or a dictionary naming it).
    AnyOf(&'static [ValueType]),
    /// `{ key = value, ... }` with these keys. [`check`] answers only that a
    /// dictionary was written; [`check_entries`] checks what's in it.
    Dict(&'static [DictKey]),
}

/// One key a [`ValueType::Dict`] accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictKey {
    pub key: &'static str,
    pub value: ValueType,
    pub required: bool,
}

/// One member of a [`ValueType::StrEnum`]: the spelling to emit, plus every
/// accepted spelling that means the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrVariant {
    pub canonical: &'static str,
    pub aliases: &'static [&'static str],
}

impl StrVariant {
    pub const fn new(canonical: &'static str) -> Self {
        Self { canonical, aliases: &[] }
    }

    pub const fn with_aliases(canonical: &'static str, aliases: &'static [&'static str]) -> Self {
        Self { canonical, aliases }
    }

    /// Whether `text` is this variant under any of its spellings.
    pub fn accepts(&self, text: &str) -> bool {
        self.canonical == text || self.aliases.contains(&text)
    }
}

/// The syntactic form of a value a user actually wrote, read back off
/// whichever surface carried it. Borrows rather than clones - every caller
/// already holds the value it is asking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueForm<'a> {
    /// No `= value` was written at all.
    Marker,
    Bool(bool),
    Integer(i64),
    /// A quoted string, with its quotes already stripped.
    Str(&'a str),
    /// A bare identifier - *not* a quoted string. Keeping this distinct
    /// from [`ValueForm::Str`] is what lets `@framing = datagram` be told
    /// apart from `@framing = "datagram"`.
    Name(&'a str),
    /// A `::`-separated path.
    Path(&'a str),
    /// An f-string.
    FString(&'a str),
    /// A list (of validator calls, today).
    List,
    /// A nested dictionary - a `settings` subtree, never a leaf value.
    Dict,
}

impl fmt::Display for ValueForm<'_> {
    /// Renders source-like, so a diagnostic's "got X" half reads the way
    /// the user wrote it. Replaces leaking Rust's `Debug` into user-facing
    /// text (`Identifier("Tru")` for what the user typed as `Tru`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueForm::Marker => write!(f, "no value"),
            ValueForm::Bool(b) => write!(f, "{b}"),
            ValueForm::Integer(i) => write!(f, "{i}"),
            ValueForm::Str(s) => write!(f, "{s:?}"),
            ValueForm::Name(n) => write!(f, "{n}"),
            ValueForm::Path(p) => write!(f, "{p}"),
            ValueForm::FString(s) => write!(f, "f{s:?}"),
            ValueForm::List => write!(f, "a list"),
            ValueForm::Dict => write!(f, "a group of nested values"),
        }
    }
}

/// A value whose form doesn't match what its key accepts. Carries both
/// halves already rendered for humans; the caller attaches its own
/// diagnostic code, context, and span - this type deliberately knows
/// nothing about any of those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    /// What was expected, from [`describe`].
    pub expected: String,
    /// What was written, from [`ValueForm`]'s [`fmt::Display`].
    pub found: String,
}

impl TypeMismatch {
    /// The one phrasing of a type mismatch, so every surface reports one
    /// the same way.
    pub fn message(&self, key: &str) -> String {
        format!("'{}' must be {}, got {}", key, self.expected, self.found)
    }
}

/// Check a written value against what its key accepts. The single place
/// this question is answered - previously four independent `match` arms
/// with four incompatible error shapes.
pub fn check(expected: &ValueType, found: ValueForm<'_>) -> Result<(), TypeMismatch> {
    let ok = match (expected, found) {
        (ValueType::Marker, ValueForm::Marker) => true,
        (ValueType::Bool, ValueForm::Bool(_)) => true,
        (ValueType::Integer, ValueForm::Integer(_)) => true,
        // An f-string evaluates to a string, so it satisfies `Str`
        // wherever a string is wanted. Not folded into `ValueForm::Str`
        // itself, because a closed `StrEnum` genuinely cannot accept one:
        // its value isn't known until interpolation.
        (ValueType::Str, ValueForm::Str(_) | ValueForm::FString(_)) => true,
        (ValueType::StrEnum(variants), ValueForm::Str(s)) => {
            variants.iter().any(|v| v.accepts(s))
        }
        (ValueType::Keyword(allowed), ValueForm::Name(n)) => allowed.contains(&n),
        (ValueType::Name, ValueForm::Name(_)) => true,
        (ValueType::BoolOrStr, ValueForm::Bool(_) | ValueForm::Str(_)) => true,
        (ValueType::ValidatorList, ValueForm::List) => true,
        (ValueType::AnyOf(types), found) => types.iter().any(|ty| check(ty, found).is_ok()),
        (ValueType::Dict(_), ValueForm::Dict) => true,
        _ => false,
    };

    if ok {
        Ok(())
    } else {
        Err(TypeMismatch { expected: describe(expected), found: found.to_string() })
    }
}

/// The human sentence for what a key accepts, used both in diagnostics and
/// in editor hover. Generated rather than retyped, so a closed value set
/// can't drift between the checker, the error message, and the docs - the
/// severity-level set previously existed in four places, one executable
/// and three prose.
pub fn describe(ty: &ValueType) -> String {
    match ty {
        ValueType::Marker => "a bare marker with no `= value`".to_string(),
        ValueType::Bool => "a boolean (`true` or `false`)".to_string(),
        ValueType::Integer => "an integer".to_string(),
        ValueType::Str => "a string".to_string(),
        ValueType::StrEnum(variants) => {
            let spellings: Vec<String> =
                variants.iter().map(|v| format!("`{:?}`", v.canonical)).collect();
            format!("one of {}", join_or(&spellings))
        }
        ValueType::Keyword(allowed) => {
            let spellings: Vec<String> = allowed.iter().map(|k| format!("`{k}`")).collect();
            if allowed.len() == 1 {
                format!("the bare keyword {}", spellings[0])
            } else {
                format!("one of the bare keywords {}", join_or(&spellings))
            }
        }
        ValueType::Name => "a bare name".to_string(),
        ValueType::BoolOrStr => "a boolean or a string message".to_string(),
        ValueType::ValidatorList => "a list of validator calls".to_string(),
        // `, or` keeps each type's own `a or b` readable.
        ValueType::AnyOf(types) => types.iter().map(describe).collect::<Vec<_>>().join(", or "),
        ValueType::Dict(keys) => {
            let keys: Vec<String> = keys
                .iter()
                .map(|k| if k.required { format!("`{}`", k.key) } else { format!("optional `{}`", k.key) })
                .collect();
            format!("a dictionary of {}", join_and(&keys))
        }
    }
}

/// The keys `ty` accepts as a dictionary, itself or one of its
/// [`ValueType::AnyOf`]'s types.
pub fn dict_keys(ty: &ValueType) -> Option<&'static [DictKey]> {
    match ty {
        ValueType::Dict(keys) => Some(keys),
        ValueType::AnyOf(types) => types.iter().find_map(dict_keys),
        _ => None,
    }
}

/// Check a dictionary's entries against the keys its [`ValueType::Dict`]
/// accepts: no unknown or repeated key, every required one present, and
/// each value of its key's type. One sentence per problem, naming the key.
pub fn check_entries<'a>(
    keys: &[DictKey],
    entries: impl IntoIterator<Item = (&'a str, ValueForm<'a>)>,
) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (key, found) in entries {
        if seen.contains(&key) {
            problems.push(format!("`{key}` is written twice"));
            continue;
        }
        seen.push(key);
        match keys.iter().find(|k| k.key == key) {
            Some(k) => {
                if let Err(mismatch) = check(&k.value, found) {
                    problems.push(mismatch.message(key));
                }
            }
            None => {
                let known: Vec<String> = keys.iter().map(|k| format!("`{}`", k.key)).collect();
                problems.push(format!("there's no key `{key}`: expected {}", join_or(&known)));
            }
        }
    }
    for k in keys.iter().filter(|k| k.required && !seen.contains(&k.key)) {
        problems.push(format!("`{}` is missing", k.key));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems) }
}

/// Every spelling a value of this type may be written as, canonical and
/// aliases alike - what an editor offers as completions. Empty for a type
/// with no closed set.
pub fn spellings(ty: &ValueType) -> Vec<&'static str> {
    match ty {
        ValueType::StrEnum(variants) => variants.iter().map(|v| v.canonical).collect(),
        ValueType::Keyword(allowed) => allowed.to_vec(),
        ValueType::AnyOf(types) => {
            let mut all: Vec<&'static str> = Vec::new();
            for spelling in types.iter().flat_map(spellings) {
                if !all.contains(&spelling) {
                    all.push(spelling);
                }
            }
            all
        }
        _ => Vec::new(),
    }
}

/// Fold an accepted spelling onto the canonical one. The single alias
/// table: every consumer that needs to act on a closed-set value calls
/// this instead of keeping its own copy. `None` if `text` isn't an
/// accepted spelling, or if `ty` has no closed set.
pub fn canonicalize(ty: &ValueType, text: &str) -> Option<&'static str> {
    match ty {
        ValueType::StrEnum(variants) => {
            variants.iter().find(|v| v.accepts(text)).map(|v| v.canonical)
        }
        ValueType::Keyword(allowed) => allowed.iter().copied().find(|k| *k == text),
        ValueType::AnyOf(types) => types.iter().find_map(|ty| canonicalize(ty, text)),
        _ => None,
    }
}

/// `a`, `b` or `c` - Oxford-comma-free, matching the existing diagnostic
/// prose style.
fn join_or(items: &[String]) -> String {
    join_with(items, "or")
}

/// `a`, `b` and `c`.
fn join_and(items: &[String]) -> String {
    join_with(items, "and")
}

fn join_with(items: &[String], word: &str) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} {word} {}", rest.join(", "), last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMING: &[StrVariant] = &[
        StrVariant::with_aliases("jsonrpc", &["json-rpc", "jsonrpc-2.0"]),
        StrVariant::with_aliases("datagram", &["comline.datagram"]),
    ];

    #[test]
    fn bool_accepts_only_a_bool() {
        assert!(check(&ValueType::Bool, ValueForm::Bool(true)).is_ok());
        assert!(check(&ValueType::Bool, ValueForm::Bool(false)).is_ok());
        assert!(check(&ValueType::Bool, ValueForm::Name("Tru")).is_err());
        assert!(check(&ValueType::Bool, ValueForm::Str("true")).is_err());
        assert!(check(&ValueType::Bool, ValueForm::Integer(1)).is_err());
    }

    #[test]
    fn a_mistyped_bool_reports_what_was_written_not_a_debug_dump() {
        // The regression this module exists to prevent: `Identifier("Tru")`
        // used to reach the user verbatim.
        let err = check(&ValueType::Bool, ValueForm::Name("Tru")).unwrap_err();
        assert_eq!(err.found, "Tru");
        assert_eq!(
            err.message("validators.allowed"),
            "'validators.allowed' must be a boolean (`true` or `false`), got Tru"
        );
    }

    #[test]
    fn str_enum_accepts_canonical_and_aliases_but_not_nonsense() {
        for spelling in ["jsonrpc", "json-rpc", "jsonrpc-2.0", "datagram", "comline.datagram"] {
            assert!(
                check(&ValueType::StrEnum(FRAMING), ValueForm::Str(spelling)).is_ok(),
                "{spelling} should be accepted"
            );
        }
        assert!(check(&ValueType::StrEnum(FRAMING), ValueForm::Str("nonsense")).is_err());
    }

    #[test]
    fn str_enum_rejects_a_bare_name_spelled_like_a_member() {
        // `@framing = datagram` (unquoted) is not `@framing = "datagram"` -
        // the distinction the IR previously could not represent.
        assert!(check(&ValueType::StrEnum(FRAMING), ValueForm::Name("datagram")).is_err());
    }

    #[test]
    fn canonicalize_folds_every_alias() {
        let ty = ValueType::StrEnum(FRAMING);
        assert_eq!(canonicalize(&ty, "json-rpc"), Some("jsonrpc"));
        assert_eq!(canonicalize(&ty, "jsonrpc-2.0"), Some("jsonrpc"));
        assert_eq!(canonicalize(&ty, "jsonrpc"), Some("jsonrpc"));
        assert_eq!(canonicalize(&ty, "comline.datagram"), Some("datagram"));
        assert_eq!(canonicalize(&ty, "nope"), None);
    }

    #[test]
    fn keyword_wants_a_bare_word_not_a_string() {
        let ty = ValueType::Keyword(&["replace"]);
        assert!(check(&ty, ValueForm::Name("replace")).is_ok());
        // `mode = "replace"` is wrong, and the form distinction catches it.
        assert!(check(&ty, ValueForm::Str("replace")).is_err());
        assert!(check(&ty, ValueForm::Name("merge")).is_err());
    }

    #[test]
    fn bool_or_str_accepts_both_but_not_a_bare_name() {
        assert!(check(&ValueType::BoolOrStr, ValueForm::Bool(true)).is_ok());
        assert!(check(&ValueType::BoolOrStr, ValueForm::Str("use NewThing")).is_ok());
        // A mistyped `True` is a bare name - exactly @deprecated's
        // Malformed case.
        assert!(check(&ValueType::BoolOrStr, ValueForm::Name("True")).is_err());
    }

    #[test]
    fn marker_rejects_any_value() {
        assert!(check(&ValueType::Marker, ValueForm::Marker).is_ok());
        assert!(check(&ValueType::Marker, ValueForm::Bool(true)).is_err());
    }

    #[test]
    fn an_fstring_satisfies_str_but_never_a_closed_set() {
        assert!(check(&ValueType::Str, ValueForm::FString("\"{x}\"")).is_ok());
        // A closed set can't accept one: the value isn't known until the
        // placeholders are interpolated.
        assert!(check(&ValueType::StrEnum(FRAMING), ValueForm::FString("\"{x}\"")).is_err());
    }

    #[test]
    fn a_dict_never_satisfies_a_leaf_type() {
        for ty in [ValueType::Bool, ValueType::Str, ValueType::Integer, ValueType::Name] {
            assert!(check(&ty, ValueForm::Dict).is_err());
        }
    }

    const MODES: ValueType = ValueType::Keyword(&["bare", "qualified"]);
    const KEYS: &[DictKey] = &[
        DictKey { key: "name", value: ValueType::StrEnum(FRAMING), required: true },
        DictKey { key: "mode", value: MODES, required: false },
    ];
    const NAME_OR_DICT: ValueType = ValueType::AnyOf(&[ValueType::StrEnum(FRAMING), ValueType::Dict(KEYS)]);

    #[test]
    fn any_of_accepts_each_of_its_types() {
        assert!(check(&NAME_OR_DICT, ValueForm::Str("json-rpc")).is_ok());
        assert!(check(&NAME_OR_DICT, ValueForm::Dict).is_ok());
        let err = check(&NAME_OR_DICT, ValueForm::Integer(1)).unwrap_err();
        assert_eq!(
            err.expected,
            r#"one of `"jsonrpc"` or `"datagram"`, or a dictionary of `name` and optional `mode`"#
        );
        assert_eq!(canonicalize(&NAME_OR_DICT, "json-rpc"), Some("jsonrpc"));
        assert_eq!(spellings(&NAME_OR_DICT), vec!["jsonrpc", "datagram"]);
    }

    #[test]
    fn a_dicts_entries_are_checked_by_key() {
        assert!(check_entries(KEYS, [("name", ValueForm::Str("jsonrpc"))]).is_ok());
        assert!(check_entries(KEYS, [("name", ValueForm::Str("jsonrpc")), ("mode", ValueForm::Name("bare"))]).is_ok());
        assert_eq!(
            check_entries(
                KEYS,
                [("mode", ValueForm::Name("loud")), ("colour", ValueForm::Str("red")), ("mode", ValueForm::Name("bare"))]
            )
            .unwrap_err(),
            vec![
                "'mode' must be one of the bare keywords `bare` or `qualified`, got loud".to_string(),
                "there's no key `colour`: expected `name` or `mode`".to_string(),
                "`mode` is written twice".to_string(),
                "`name` is missing".to_string(),
            ]
        );
    }

    const LEVELS: &[StrVariant] = &[
        StrVariant::new("allow"),
        StrVariant::new("hint"),
        StrVariant::new("info"),
        StrVariant::new("warn"),
        StrVariant::new("error"),
    ];

    #[test]
    fn describe_renders_a_closed_set_without_retyping_it() {
        assert_eq!(
            describe(&ValueType::StrEnum(LEVELS)),
            r#"one of `"allow"`, `"hint"`, `"info"`, `"warn"` or `"error"`"#
        );
    }

    #[test]
    fn describe_a_single_keyword_reads_naturally() {
        assert_eq!(
            describe(&ValueType::Keyword(&["replace"])),
            "the bare keyword `replace`"
        );
    }

    #[test]
    fn spellings_offers_canonical_forms_for_completion() {
        assert_eq!(spellings(&ValueType::StrEnum(FRAMING)), vec!["jsonrpc", "datagram"]);
        assert_eq!(spellings(&ValueType::Keyword(&["replace"])), vec!["replace"]);
        assert!(spellings(&ValueType::Bool).is_empty());
    }
}
