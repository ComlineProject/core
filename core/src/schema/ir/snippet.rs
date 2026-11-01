//! One line of IDL for the declaration or member a changelog entry is about,
//! printed from the frozen IR: what the documentation's changelog shows as
//! the entry's old and new lines (`function greet(name: string) -> Greeting;`).
//!
//! A line is the declaration's or member's own: a field
//! (`optional email: string`), an enum variant (`Cash`), a function
//! signature, an annotation (`@timeout_ms = 500`, `@validators = [...]`), or
//! a whole declaration kept to one line (`struct Customer { id: u64, ... }`,
//! `const LIMIT: u8 = 10`, `type Handle = string`). A kind with nothing to
//! print as IDL (a docstring change, an error's ordinal, guarantees, a
//! reorder of functions, a namespace rename) gets no lines: a renderer words
//! those itself.

use crate::schema::ir::diff::{BreakingChange, Modification, NewFeature};
use crate::schema::ir::frozen::unit::{ConfigValue, DeprecatedValue, FrozenUnit};

/// An entry's old and new lines; either is `None` where that side has none
/// (an addition has no old line, a removal no new one).
pub type Lines = (Option<String>, Option<String>);

type Schemas<'a> = &'a [Vec<FrozenUnit>];

/// The lines for a breaking change.
pub fn breaking(change: &BreakingChange, old: Schemas, new: Schemas) -> Lines {
    use BreakingChange as B;
    match change {
        B::RemovedStruct { name }
        | B::RemovedEnum { name }
        | B::RemovedProtocol { name }
        | B::RemovedError { name }
        | B::RemovedConstant { name }
        | B::RemovedTypeAlias { name }
        | B::RemovedValidator { name } => (declaration(old, name), None),
        B::RemovedField { type_name, field_name } => (member(old, type_name, field_name), None),
        B::AddedRequiredField { type_name, field_name, .. } | B::AddedOptionalField { type_name, field_name, .. } => {
            (None, member(new, type_name, field_name))
        }
        B::ChangedFieldType { type_name, field_name, .. }
        | B::ChangedFieldOptionality { type_name, field_name, .. } => both(old, new, |s| member(s, type_name, field_name)),
        B::ReorderedFields { type_name: name }
        | B::ReorderedEnumVariants { enum_name: name }
        | B::ChangedConstantType { name, .. }
        | B::ChangedTypeAliasTarget { name, .. }
        | B::ChangedValidator { name } => both(old, new, |s| declaration(s, name)),
        B::RemovedEnumVariant { enum_name, variant } => (variant_line(old, enum_name, variant), None),
        B::RenamedEnumVariant { enum_name, old_name, new_name } => {
            (variant_line(old, enum_name, old_name), variant_line(new, enum_name, new_name))
        }
        B::RemovedFunction { protocol_name, function_name } => (member(old, protocol_name, function_name), None),
        B::ChangedFunctionSignature { protocol_name, function_name, .. }
        | B::RemovedThrownError { protocol_name, function_name, .. } => {
            both(old, new, |s| member(s, protocol_name, function_name))
        }
        B::RenamedProtocol { old_name, new_name } => (declaration(old, old_name), declaration(new, new_name)),
        B::ValidationTightened { target } => validators(old, new, target),
        B::ChangedWireAnnotation { target, key, .. } => both(old, new, |s| annotation(s, target, key)),
        B::ReorderedFunctions { .. }
        | B::RenamedNamespace { .. }
        | B::ChangedErrorOrdinal { .. }
        | B::GaveUpGuarantees { .. } => (None, None),
    }
}

/// The lines for a new feature.
pub fn feature(change: &NewFeature, old: Schemas, new: Schemas) -> Lines {
    use NewFeature as F;
    match change {
        F::AddedStruct { name, .. }
        | F::AddedEnum { name, .. }
        | F::AddedProtocol { name, .. }
        | F::AddedError { name, .. }
        | F::AddedConstant { name, .. }
        | F::AddedTypeAlias { name, .. }
        | F::AddedValidator { name } => (None, declaration(new, name)),
        F::AddedField { type_name, field_name, .. } => (None, member(new, type_name, field_name)),
        F::AddedEnumVariant { enum_name, variant } => (None, variant_line(new, enum_name, variant)),
        F::AddedFunction { protocol_name, function_name, .. } => (None, member(new, protocol_name, function_name)),
        F::AddedThrownError { protocol_name, function_name, .. } => {
            both(old, new, |s| member(s, protocol_name, function_name))
        }
        F::ChangedConstantValue { name, .. } => both(old, new, |s| declaration(s, name)),
        F::ValidationLoosened { target } => validators(old, new, target),
        F::Deprecated { target } => both(old, new, |s| annotation(s, target, "deprecated")),
        F::RestoredGuarantees { .. } => (None, None),
    }
}

/// The lines for a modification.
pub fn modification(change: &Modification, old: Schemas, new: Schemas) -> Lines {
    use Modification as M;
    match change {
        M::DocstringChanged { .. } => (None, None),
        M::TypeRespelled { target, .. } => both(old, new, |s| target_line(s, target)),
        M::ChangedConstantValue { name, .. } | M::ChangedErrorMessage { name } => {
            both(old, new, |s| declaration(s, name))
        }
        M::ChangedFieldDefault { type_name, field_name } => both(old, new, |s| member(s, type_name, field_name)),
        M::ChangedAnnotation { target, key, .. } => both(old, new, |s| annotation(s, target, key)),
    }
}

fn both(old: Schemas, new: Schemas, line: impl Fn(Schemas) -> Option<String>) -> Lines {
    (line(old), line(new))
}

/// A validators change: the `@validators` lines, or, where those didn't
/// change (a constant a validator argument uses), the target's own line.
fn validators(old: Schemas, new: Schemas, target: &str) -> Lines {
    let lines = both(old, new, |s| annotation(s, target, "validators"));
    if lines.0 != lines.1 {
        return lines;
    }
    both(old, new, |s| target_line(s, target))
}

/// The declaration `name` and the schema it's in.
fn find<'a>(schemas: Schemas<'a>, name: &str) -> Option<(&'a [FrozenUnit], &'a FrozenUnit)> {
    schemas.iter().find_map(|schema| {
        schema.iter().find(|unit| declared_name(unit) == Some(name)).map(|unit| (schema.as_slice(), unit))
    })
}

fn declared_name(unit: &FrozenUnit) -> Option<&str> {
    match unit {
        FrozenUnit::Struct { name, .. }
        | FrozenUnit::Enum { name, .. }
        | FrozenUnit::Protocol { name, .. }
        | FrozenUnit::Constant { name, .. }
        | FrozenUnit::TypeAlias { name, .. }
        | FrozenUnit::Validator { name, .. } => Some(name),
        FrozenUnit::Error { name, imported_from: None, .. } => Some(name),
        _ => None,
    }
}

/// `S.f` is a member's line, `S` the declaration's.
fn target_line(schemas: Schemas, target: &str) -> Option<String> {
    match target.split_once('.') {
        Some((parent, name)) => member(schemas, parent, name),
        None => declaration(schemas, target),
    }
}

/// The member `name` of the declaration `parent`: a field, a validator's
/// parameter or a function.
fn member(schemas: Schemas, parent: &str, name: &str) -> Option<String> {
    let (schema, unit) = find(schemas, parent)?;
    let members = match unit {
        FrozenUnit::Struct { fields, .. } | FrozenUnit::Error { fields, .. } => fields,
        FrozenUnit::Validator { properties, .. } => properties,
        FrozenUnit::Protocol { functions, .. } => functions,
        _ => return None,
    };
    members.iter().find_map(|m| match m {
        FrozenUnit::Field { name: n, .. } if n == name => Some(field(m)),
        FrozenUnit::Function { name: n, .. } if n == name => Some(function(schema, m)),
        _ => None,
    })
}

fn variant_line(schemas: Schemas, enum_name: &str, variant: &str) -> Option<String> {
    let (_, FrozenUnit::Enum { variants, .. }) = find(schemas, enum_name)? else { return None };
    variants
        .iter()
        .any(|v| matches!(v, FrozenUnit::EnumVariant(n, _) if n == variant))
        .then(|| variant.to_string())
}

/// The annotation `key` on `target` (`S`, `S.f`, `P.f`), as written.
fn annotation(schemas: Schemas, target: &str, key: &str) -> Option<String> {
    let parameters = parameters(schemas, target)?;
    if key == "validators" {
        let calls: Vec<String> = parameters
            .iter()
            .filter_map(|p| match p {
                FrozenUnit::ValidatorRef { name, args, .. } => Some(call(name, args)),
                _ => None,
            })
            .collect();
        return (!calls.is_empty()).then(|| format!("@validators = [{}]", calls.join(", ")));
    }
    if key == "deprecated" {
        if let Some(line) = parameters.iter().find_map(|p| match p {
            FrozenUnit::Deprecated(DeprecatedValue::False) => None,
            FrozenUnit::Deprecated(DeprecatedValue::Message(m)) if !m.is_empty() => {
                Some(format!("@deprecated = {m:?}"))
            }
            FrozenUnit::Deprecated(_) => Some("@deprecated".to_string()),
            _ => None,
        }) {
            return Some(line);
        }
    }
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == key => Some(property(name, value)),
        _ => None,
    })
}

/// The annotations on `target`.
fn parameters<'a>(schemas: Schemas<'a>, target: &str) -> Option<&'a [FrozenUnit]> {
    let (parent, member) = match target.split_once('.') {
        Some((parent, member)) => (parent, Some(member)),
        None => (target, None),
    };
    let (_, unit) = find(schemas, parent)?;
    let Some(member) = member else {
        return match unit {
            FrozenUnit::Struct { parameters, .. }
            | FrozenUnit::Protocol { parameters, .. }
            | FrozenUnit::Error { parameters, .. }
            | FrozenUnit::TypeAlias { parameters, .. } => Some(parameters),
            _ => None,
        };
    };
    let members = match unit {
        FrozenUnit::Struct { fields, .. } | FrozenUnit::Error { fields, .. } => fields,
        FrozenUnit::Validator { properties, .. } => properties,
        FrozenUnit::Protocol { functions, .. } => functions,
        _ => return None,
    };
    members.iter().find_map(|m| match m {
        FrozenUnit::Field { name, parameters, .. } | FrozenUnit::Function { name, parameters, .. }
            if name == member =>
        {
            Some(parameters.as_slice())
        }
        _ => None,
    })
}

fn property(name: &str, value: &ConfigValue) -> String {
    match value {
        ConfigValue::Marker => format!("@{name}"),
        ConfigValue::FString(s) => format!("@{name} = f{s:?}"),
        other => format!("@{name} = {}", other.source_text()),
    }
}

/// A validator call: `StringBounds(min_chars = 2, max_chars = 8)`.
fn call(name: &str, args: &[FrozenUnit]) -> String {
    let args: Vec<String> = args
        .iter()
        .filter_map(|a| match a {
            FrozenUnit::Property { name, value } => Some(format!("{name} = {}", value.source_text())),
            _ => None,
        })
        .collect();
    if args.is_empty() {
        name.to_string()
    } else {
        format!("{name}({})", args.join(", "))
    }
}

/// A field: `optional nickname: Handle = "x"`.
fn field(unit: &FrozenUnit) -> String {
    let FrozenUnit::Field { optional, name, ty, default, .. } = unit else { return String::new() };
    let mut out = String::new();
    if *optional {
        out.push_str("optional ");
    }
    out.push_str(&format!("{name}: {ty}"));
    if let Some(default) = default {
        out.push_str(&format!(" = {default}"));
    }
    out
}

/// A function: `function send(text: string) -> Message ! Rejected, TooLong;`.
fn function(schema: &[FrozenUnit], unit: &FrozenUnit) -> String {
    let FrozenUnit::Function { name, arguments, _return, throws, .. } = unit else { return String::new() };
    let args: Vec<String> = arguments.iter().map(|a| format!("{}: {}", a.name, a.ty)).collect();
    let mut out = format!("function {name}({})", args.join(", "));
    if let Some(ret) = _return {
        out.push_str(&format!(" -> {ret}"));
    }
    if !throws.is_empty() {
        let names: Vec<String> = throws
            .iter()
            .map(|ordinal| {
                schema
                    .iter()
                    .find_map(|u| match u {
                        FrozenUnit::Error { ordinal: o, name, .. } if o == ordinal => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("#{ordinal}"))
            })
            .collect();
        out.push_str(&format!(" ! {}", names.join(", ")));
    }
    out.push(';');
    out
}

fn fields(units: &[FrozenUnit]) -> String {
    let fields: Vec<String> =
        units.iter().filter(|u| matches!(u, FrozenUnit::Field { .. })).map(field).collect();
    fields.join(", ")
}

/// A declaration kept to one line.
fn declaration(schemas: Schemas, name: &str) -> Option<String> {
    let (_, unit) = find(schemas, name)?;
    Some(match unit {
        FrozenUnit::Struct { name, fields: fs, .. } => format!("struct {name} {{ {} }}", fields(fs)),
        FrozenUnit::Enum { name, variants, .. } => {
            let variants: Vec<&str> = variants
                .iter()
                .filter_map(|v| match v {
                    FrozenUnit::EnumVariant(n, _) => Some(n.as_str()),
                    _ => None,
                })
                .collect();
            format!("enum {name} {{ {} }}", variants.join(", "))
        }
        FrozenUnit::Protocol { name, functions, .. } => {
            let count = functions.iter().filter(|f| matches!(f, FrozenUnit::Function { .. })).count();
            format!("protocol {name} {{ /* {count} function{} */ }}", if count == 1 { "" } else { "s" })
        }
        FrozenUnit::Error { name, message, fields: fs, .. } => {
            let message = if message.contains('{') { format!("f{message:?}") } else { format!("{message:?}") };
            let fs = fields(fs);
            if fs.is_empty() {
                format!("error {name} {{ message = {message} }}")
            } else {
                format!("error {name} {{ message = {message}, {fs} }}")
            }
        }
        FrozenUnit::Constant { name, ty, value, .. } => match value {
            Some(value) => format!("const {name}: {ty} = {value}"),
            None => format!("const {name}: {ty}"),
        },
        FrozenUnit::TypeAlias { name, target, .. } => format!("type {name} = {target}"),
        FrozenUnit::Validator { name, targets, properties, .. } => {
            let targets: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
            let params = fields(properties);
            if params.is_empty() {
                format!("validator {name} for {} {{ … }}", targets.join(", "))
            } else {
                format!("validator {name} for {} {{ {params} … }}", targets.join(", "))
            }
        }
        _ => return None,
    })
}
