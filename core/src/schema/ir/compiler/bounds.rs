//! Sizes written in types: `T[N]` and `T[max N]` (bounds on types, core#98).
//!
//! Checked on the parsed schema, before it's frozen: the frozen form holds
//! sizes resolved to numbers, and a size that doesn't resolve, or a
//! constant's name, is only known here.
//!
//! - A size is at least 1 and at most `u32::MAX`. A maximum of 0 holds
//!   nothing a reader could tell from absent, and an exact length of 0
//!   carries nothing.
//! - A size that names something names an unsigned integer constant.
//! - Where a type is used (a field, an argument, a return, a validator's
//!   property), an alias of a string or of a dynamic array takes no `[…]`
//!   of its own: its limit is changed in the alias, and a list of it is an
//!   alias of its own. `string`, the built-in name, is exempt.

use std::collections::{HashMap, HashSet};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{Declaration, SizeValue, Type};
use crate::schema::ir::compiler::import_resolver::size_constant;
use crate::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use crate::settings::effective::{
    effective_declaration_settings, effective_schema_settings, schema_settings, settings_annotation_of,
};
use crate::settings::keys::{self, DeclPath};
use crate::settings::value::{SettingsDict, SettingsValue};

const SIZE_NOT_POSITIVE: DiagnosticCode = DiagnosticCode::error("bounds::size-not-positive");
const SIZE_TOO_LARGE: DiagnosticCode = DiagnosticCode::error("bounds::size-too-large");
const SIZE_NOT_A_CONSTANT: DiagnosticCode = DiagnosticCode::error("bounds::size-not-a-constant");
const ALIAS_SUFFIX: DiagnosticCode = DiagnosticCode::error("bounds::alias-suffix");

/// The largest size a type can declare.
pub const MAX_SIZE: u64 = u32::MAX as u64;

/// Check every size written in `declarations`' types, and the alias-suffix
/// rule where types are used.
pub fn check_bounds(
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
) -> Result<(), Vec<Diagnostic>> {
    let aliases: HashMap<String, &Type> = declarations
        .iter()
        .filter_map(|decl| match &decl.value {
            Declaration::TypeAlias(alias) => Some((alias.name(), alias.target_type())),
            _ => None,
        })
        .collect();
    let mut check = Check { declarations, use_context, aliases, errors: Vec::new() };

    for decl in declarations {
        match &decl.value {
            Declaration::Struct(s) => {
                for field in s.fields() {
                    let context = format!("Struct '{}', field '{}'", s.name(), field.value.name());
                    check.used(field.value.field_type(), field.value.field_type_span(), &context);
                }
            }
            Declaration::Error(e) => {
                for field in e.fields() {
                    let context = format!("Error '{}', field '{}'", e.name(), field.value.name());
                    check.used(field.value.field_type(), field.value.field_type_span(), &context);
                }
            }
            Declaration::Protocol(p) => {
                for function in p.functions() {
                    let context = format!("Protocol '{}', function '{}'", p.name(), function.value.name());
                    if let Some(args) = function.value.args() {
                        let first = std::iter::once(args.first());
                        for arg in first.chain(args.rest().iter().map(|a| a.arg_type())) {
                            check.used(&arg.arg_type.value, arg.arg_type.span, &context);
                        }
                    }
                    if let Some(ret) = function.value.return_type() {
                        check.used(&ret.return_type.value, ret.return_type.span, &context);
                    }
                }
            }
            Declaration::Validator(v) => {
                for property in v.properties() {
                    let context = format!("Validator '{}', property '{}'", v.name(), property.value.name());
                    check.used(property.value.property_type(), property.span, &context);
                }
                for target in v.targets() {
                    check.sizes(target, decl.span, &format!("Validator '{}'", v.name()));
                }
            }
            Declaration::TypeAlias(a) => {
                // An alias may wrap another in a list: that's its own alias.
                check.sizes(a.target_type(), a.target_type_span(), &format!("Type alias '{}'", a.name()));
            }
            Declaration::Const(c) => {
                check.sizes(c.type_def(), c.type_def_span(), &format!("Constant '{}'", c.name()));
            }
            _ => {}
        }
    }

    if check.errors.is_empty() { Ok(()) } else { Err(check.errors) }
}

struct Check<'a> {
    declarations: &'a [rust_sitter::Spanned<Declaration>],
    use_context: Option<(&'a [String], &'a ProjectContext)>,
    aliases: HashMap<String, &'a Type>,
    errors: Vec<Diagnostic>,
}

impl Check<'_> {
    /// A type where it's used: its sizes, and no `[…]` on an alias of a
    /// string or a dynamic array.
    fn used(&mut self, ty: &Type, span: (usize, usize), context: &str) {
        self.sizes(ty, span, context);
        self.suffixes(ty, span, context);
    }

    fn suffixes(&mut self, ty: &Type, span: (usize, usize), context: &str) {
        match ty {
            Type::Array(array) => {
                if let Type::Named(name) = array.elem_type() {
                    if let Some(kind) = self.sized_alias(&name.text) {
                        self.errors.push(
                            Diagnostic::new(
                                ALIAS_SUFFIX,
                                format!("`{}` is an alias of {kind}, which takes no `[…]` where it's used", name.text),
                            )
                            .with_context(context.to_string())
                            .with_help(format!(
                                "change the limit in the alias, or declare a list of it as an alias of its own: \
                                 `type {}s = {}`",
                                name.text,
                                // The suffix as written, for the suggestion.
                                Self::written(ty)
                            ))
                            .with_span(span),
                        );
                    }
                }
                self.suffixes(array.elem_type(), span, context);
            }
            Type::Union(union) => {
                for member in union.members() {
                    self.suffixes(member, span, context);
                }
            }
            _ => {}
        }
    }

    /// What a local alias stands for, when it's a string or a dynamic
    /// array, followed through other aliases: `a string`, `a list`.
    fn sized_alias(&self, name: &str) -> Option<&'static str> {
        let mut target = *self.aliases.get(name)?;
        for _ in 0..32 {
            match target {
                Type::String(_) => return Some("a string"),
                Type::Array(array) if matches!(array.elem_type(), Type::Char(_)) => return Some("a string"),
                Type::Array(array) if array.exact().is_none() => return Some("a list"),
                Type::Named(next) => target = self.aliases.get(&next.text)?,
                _ => return None,
            }
        }
        None
    }

    /// `ty` as the schema spells it, for a message.
    fn written(ty: &Type) -> String {
        match ty {
            Type::Array(array) => {
                let inner = Self::written(array.elem_type());
                match (array.exact(), array.max()) {
                    (Some(n), _) => format!("{inner}[{}]", n.text()),
                    (_, Some(n)) => format!("{inner}[max {}]", n.text()),
                    _ => format!("{inner}[]"),
                }
            }
            Type::Named(name) => name.text.clone(),
            other => crate::schema::idl::vocabulary::primitive_name_of(other).unwrap_or("…").to_string(),
        }
    }

    /// Every size written in `ty`.
    fn sizes(&mut self, ty: &Type, span: (usize, usize), context: &str) {
        match ty {
            Type::Array(array) => {
                if let Some(n) = array.exact() {
                    self.size(n, "length", span, context);
                }
                if let Some(n) = array.max() {
                    self.size(n, "maximum", span, context);
                }
                self.sizes(array.elem_type(), span, context);
            }
            Type::Union(union) => {
                for member in union.members() {
                    self.sizes(member, span, context);
                }
            }
            _ => {}
        }
    }

    fn size(&mut self, value: &SizeValue, what: &str, span: (usize, usize), context: &str) {
        let n = match value {
            SizeValue::Integer(i) => i.value().into(),
            SizeValue::Constant(c) => match size_constant(self.declarations, self.use_context, &c.text) {
                Some(n) => i128::from(n),
                None => {
                    self.errors.push(
                        Diagnostic::new(
                            SIZE_NOT_A_CONSTANT,
                            format!("the {what} `{}` isn't an unsigned integer constant", c.text),
                        )
                        .with_context(context.to_string())
                        .with_help("name a `u8`, `u16`, `u32` or `u64` constant, or write the number")
                        .with_span(span),
                    );
                    return;
                }
            },
        };
        let named = match value {
            SizeValue::Constant(c) => format!(" (`{}`)", c.text),
            SizeValue::Integer(_) => String::new(),
        };
        if n < 1 {
            self.errors.push(
                Diagnostic::new(SIZE_NOT_POSITIVE, format!("a {what} of {n}{named}: a size is at least 1"))
                    .with_context(context.to_string())
                    .with_help(if what == "maximum" {
                        "a maximum of 0 holds only an empty value; leave the field out, or allow at least 1"
                    } else {
                        "an exact length of 0 carries nothing; leave the field out"
                    })
                    .with_span(span),
            );
        } else if n > i128::from(MAX_SIZE) {
            self.errors.push(
                Diagnostic::new(SIZE_TOO_LARGE, format!("a {what} of {n}{named} is over {MAX_SIZE}"))
                    .with_context(context.to_string())
                    .with_help(format!("a size is at most {MAX_SIZE} (`u32::MAX`)"))
                    .with_span(span),
            );
        }
    }
}

/// Fill in the default bounds of `units`, a schema of a package whose
/// settings are `package_settings` (core#98): every string or dynamic array
/// a type leaves without a maximum takes the `bounds.string.max` or
/// `bounds.array.max` in effect where it's written, and a struct that can
/// contain itself, written without `@max_depth`, takes `bounds.depth.max`.
/// Each is stored resolved, so the frozen schema says what every position
/// holds and a change to a default is a change wherever it lands.
///
/// Settings layer as everywhere else: the package's, the schema's own
/// block, then a declaration's `@settings` preset, a key scoped to the
/// position (`struct.field.bounds.string.max`) before a global one. A
/// maximum written in the type always wins. Another package's types are
/// named, not rewritten here: they keep the maxima their own package gave
/// them.
pub fn apply_default_bounds(units: &mut [FrozenUnit], package_settings: &SettingsDict) {
    let schema = schema_settings(units);
    let schema_effective = effective_schema_settings(package_settings, &schema);
    let effective = |parameters: &[FrozenUnit]| {
        effective_declaration_settings(&schema_effective, &schema, settings_annotation_of(parameters))
    };
    let recursive = self_containing_structs(units);

    for unit in units.iter_mut() {
        match unit {
            FrozenUnit::Struct { name, parameters, fields, .. } => {
                let decl = effective(parameters);
                if recursive.contains(name.as_str()) && !has_max_depth(parameters) {
                    if let Some(depth) = default_for(&decl, DeclPath::Struct, keys::BOUND_DEPTH) {
                        parameters.push(FrozenUnit::Property {
                            name: "max_depth".to_string(),
                            value: ConfigValue::Integer(depth as i64),
                        });
                    }
                }
                fill_fields(fields, DeclPath::StructField, &effective);
            }
            FrozenUnit::Error { fields, imported_from: None, .. } => {
                fill_fields(fields, DeclPath::ErrorField, &effective);
            }
            FrozenUnit::Protocol { functions, .. } => {
                for function in functions {
                    if let FrozenUnit::Function { parameters, arguments, _return, .. } = function {
                        let decl = effective(parameters);
                        let defaults = Defaults::of(&decl, DeclPath::ProtocolFunction);
                        for argument in arguments {
                            defaults.fill(&mut argument.ty);
                        }
                        if let Some(ret) = _return {
                            defaults.fill(ret);
                        }
                    }
                }
            }
            // An alias is filled where it's declared, so wherever it's used
            // holds the same type. A validator's properties are its call's
            // arguments, bounded like any other value.
            FrozenUnit::TypeAlias { parameters, target, .. } => {
                Defaults::global(&effective(parameters)).fill(target);
            }
            FrozenUnit::Validator { properties, .. } => {
                let defaults = Defaults::global(&schema_effective);
                for property in properties {
                    if let FrozenUnit::Field { ty, .. } = property {
                        defaults.fill(ty);
                    }
                }
            }
            _ => {}
        }
    }
}

fn fill_fields(fields: &mut [FrozenUnit], decl: DeclPath, effective: &dyn Fn(&[FrozenUnit]) -> SettingsDict) {
    for field in fields {
        if let FrozenUnit::Field { parameters, ty, .. } = field {
            Defaults::of(&effective(parameters), decl).fill(ty);
        }
    }
}

/// The default maxima in effect at one position.
struct Defaults {
    string: Option<u64>,
    array: Option<u64>,
}

impl Defaults {
    fn of(settings: &SettingsDict, decl: DeclPath) -> Self {
        Defaults {
            string: default_for(settings, decl, keys::BOUND_STRING),
            array: default_for(settings, decl, keys::BOUND_ARRAY),
        }
    }

    /// Where no key can be scoped to a declaration: the global form only.
    fn global(settings: &SettingsDict) -> Self {
        let get = |kind| positive(settings.get_path(&keys::bounds_max().key(None, Some(kind))));
        Defaults { string: get(keys::BOUND_STRING), array: get(keys::BOUND_ARRAY) }
    }

    fn fill(&self, ty: &mut TypeRef) {
        match ty {
            TypeRef::Primitive(PrimitiveType::String) => {
                if let Some(max) = self.string {
                    *ty = TypeRef::Array {
                        element: Box::new(TypeRef::Primitive(PrimitiveType::Char)),
                        length: None,
                        max: Some(max),
                    };
                }
            }
            // A string already bounded, or fixed: nothing to fill.
            TypeRef::Array { element, .. } if **element == TypeRef::Primitive(PrimitiveType::Char) => {}
            TypeRef::Array { element, length, max } => {
                if length.is_none() && max.is_none() {
                    *max = self.array;
                }
                self.fill(element);
            }
            TypeRef::Union(members) => members.iter_mut().for_each(|m| self.fill(m)),
            _ => {}
        }
    }
}

/// The `bounds.<kind>.max` in effect for `decl`, its scoped key first.
fn default_for(settings: &SettingsDict, decl: DeclPath, kind: &str) -> Option<u64> {
    keys::bounds_max()
        .candidates(decl, kind)
        .iter()
        .find_map(|key| positive(settings.get_path(key)))
}

fn positive(value: Option<&SettingsValue>) -> Option<u64> {
    match value {
        Some(SettingsValue::Integer(n)) if *n >= 1 => u64::try_from(*n).ok(),
        _ => None,
    }
}

fn has_max_depth(parameters: &[FrozenUnit]) -> bool {
    parameters.iter().any(|p| matches!(p, FrozenUnit::Property { name, .. } if name == "max_depth"))
}

/// The local structs that can contain themselves: one reaches itself
/// through its fields, however they nest (arrays, unions, optionals,
/// aliases, other structs).
fn self_containing_structs(units: &[FrozenUnit]) -> HashSet<String> {
    let aliases: HashMap<&str, &TypeRef> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
        .collect();
    let edges: HashMap<&str, HashSet<&str>> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Struct { name, fields, .. } => {
                let mut reached = HashSet::new();
                for field in fields {
                    if let FrozenUnit::Field { ty, .. } = field {
                        named(ty, &aliases, &mut reached, 0);
                    }
                }
                Some((name.as_str(), reached))
            }
            _ => None,
        })
        .collect();
    let mut recursive = HashSet::new();
    for &start in edges.keys() {
        let mut seen = HashSet::new();
        let mut work: Vec<&str> = edges[start].iter().copied().collect();
        while let Some(next) = work.pop() {
            if next == start {
                recursive.insert(start.to_string());
                break;
            }
            if seen.insert(next) {
                work.extend(edges.get(next).into_iter().flatten().copied());
            }
        }
    }
    recursive
}

/// Every name `ty` refers to, through local aliases.
fn named<'a>(ty: &'a TypeRef, aliases: &HashMap<&str, &'a TypeRef>, out: &mut HashSet<&'a str>, depth: usize) {
    if depth > 32 {
        return;
    }
    match ty {
        TypeRef::Named(name) => match aliases.get(name.as_str()) {
            Some(target) => named(target, aliases, out, depth + 1),
            None => {
                out.insert(name.as_str());
            }
        },
        TypeRef::Array { element, .. } => named(element, aliases, out, depth + 1),
        TypeRef::Union(members) => members.iter().for_each(|m| named(m, aliases, out, depth + 1)),
        _ => {}
    }
}

const UNBOUNDED: DiagnosticCode = DiagnosticCode::error("bounds::unbounded");
const UNBOUNDED_DEPTH: DiagnosticCode = DiagnosticCode::error("bounds::unbounded-depth");

/// No unbounded value (core#98): every string and dynamic array, at every
/// position, has a maximum, and every struct that can contain itself has a
/// depth limit, so every message has a worst-case size. Run on a schema's
/// frozen units once their default bounds are filled in
/// ([`apply_default_bounds`]).
///
/// The one way past it is giving up bounded size, per protocol: a position
/// may go without a maximum when every protocol that reaches it is marked
/// `@unverified`, and each of those then gives up `bounded_size`, frozen
/// beside what else it gives up, for its consumers to accept. A position
/// no protocol reaches is a type another package can use, and has to be
/// bounded.
pub fn check_bounded(units: &mut [FrozenUnit]) -> Vec<Diagnostic> {
    use crate::schema::ir::frozen::unit::Guarantee;
    use crate::schema::ir::unverified;

    let recursive = self_containing_structs(units);

    // Every unbounded position, by the declaration it's written in.
    struct Position {
        decl: String,
        at: String,
        ty: String,
        span: Option<(usize, usize)>,
        depth: bool,
    }
    let mut positions: Vec<Position> = Vec::new();
    let mut depths: Vec<Position> = Vec::new();
    let mut push = |decl: &str, at: String, ty: &TypeRef, span: Option<(usize, usize)>| {
        if let Some(unbounded) = first_unbounded(ty) {
            positions.push(Position { decl: decl.to_string(), at, ty: unbounded.to_string(), span, depth: false });
        }
    };
    for unit in units.iter() {
        match unit {
            FrozenUnit::Struct { name, fields, parameters, span, .. } => {
                for field in fields {
                    if let FrozenUnit::Field { name: f, ty, span: fs, .. } = field {
                        push(name, format!("struct `{name}`, field `{f}`"), ty, Some(*fs));
                    }
                }
                if recursive.contains(name.as_str()) && !has_max_depth(parameters) {
                    depths.push(Position {
                        decl: name.clone(),
                        at: format!("struct `{name}`"),
                        ty: String::new(),
                        span: Some(*span),
                        depth: true,
                    });
                }
            }
            FrozenUnit::Error { name, fields, imported_from: None, .. } => {
                for field in fields {
                    if let FrozenUnit::Field { name: f, ty, span: fs, .. } = field {
                        push(name, format!("error `{name}`, field `{f}`"), ty, Some(*fs));
                    }
                }
            }
            FrozenUnit::TypeAlias { name, target, span, .. } => {
                push(name, format!("type `{name}`"), target, Some(*span));
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                for function in functions {
                    if let FrozenUnit::Function { name: f, arguments, _return, span, .. } = function {
                        for argument in arguments {
                            push(name, format!("protocol `{name}`, function `{f}`, argument `{}`", argument.name), &argument.ty, Some(argument.span));
                        }
                        if let Some(ret) = _return {
                            push(name, format!("protocol `{name}`, function `{f}`'s return"), ret, Some(*span));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    positions.extend(depths);

    // Which declarations each protocol reaches, itself included.
    let declared: HashMap<&str, Vec<&TypeRef>> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Struct { name, fields, .. } | FrozenUnit::Error { name, fields, imported_from: None, .. } => {
                let types = fields
                    .iter()
                    .filter_map(|f| match f {
                        FrozenUnit::Field { ty, .. } => Some(ty),
                        _ => None,
                    })
                    .collect();
                Some((name.as_str(), types))
            }
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), vec![target])),
            _ => None,
        })
        .collect();
    let mut reached_by: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    for unit in units.iter() {
        let FrozenUnit::Protocol { name, functions, parameters, .. } = unit else { continue };
        let marked = unverified::marker(parameters).is_some();
        let mut reached: HashSet<&str> = HashSet::from([name.as_str()]);
        let mut work: Vec<&TypeRef> = functions
            .iter()
            .flat_map(|f| match f {
                FrozenUnit::Function { arguments, _return, .. } => {
                    arguments.iter().map(|a| &a.ty).chain(_return.iter()).collect::<Vec<_>>()
                }
                _ => vec![],
            })
            .collect();
        while let Some(ty) = work.pop() {
            let mut names = HashSet::new();
            named(ty, &HashMap::new(), &mut names, 0);
            for n in names {
                if let Some((key, types)) = declared.get_key_value(n) {
                    if reached.insert(key) {
                        work.extend(types.iter().copied());
                    }
                }
            }
        }
        for decl in reached {
            reached_by.entry(decl.to_string()).or_default().push((name.clone(), marked));
        }
    }

    let mut errors = Vec::new();
    let mut giving_up: HashSet<String> = HashSet::new();
    for p in &positions {
        let reaching = reached_by.get(&p.decl).cloned().unwrap_or_default();
        if !reaching.is_empty() && reaching.iter().all(|(_, marked)| *marked) {
            giving_up.extend(reaching.into_iter().map(|(protocol, _)| protocol));
            continue;
        }
        let verified: Vec<String> =
            reaching.iter().filter(|(_, marked)| !marked).map(|(n, _)| format!("`{n}`")).collect();
        let (code, message, help) = if p.depth {
            (
                UNBOUNDED_DEPTH,
                format!("{} can contain itself, and has no depth limit", p.at),
                "write `@max_depth = N` on it, or set `bounds.depth.max` in settings".to_string(),
            )
        } else {
            let kind = if p.ty == "string" || p.ty.starts_with("char") { "string" } else { "array" };
            let written = if kind == "string" { "char[max N]".to_string() } else { format!("{}[max N]", p.ty.trim_end_matches("[]")) };
            (
                UNBOUNDED,
                format!("{}: `{}` has no maximum", p.at, p.ty),
                format!("write `{written}`, or set `bounds.{kind}.max` in settings"),
            )
        };
        // Where protocols reach it, they could give bounded size up.
        let help = if verified.is_empty() {
            help
        } else {
            format!("{help}; or give up bounded size with `@unverified = \"…\"` on {}", verified.join(", "))
        };
        errors.push(Diagnostic::new(code, message).with_help(help).maybe_span(p.span));
    }

    // Each protocol giving up bounded size says so, beside the rest.
    for unit in units.iter_mut() {
        let FrozenUnit::Protocol { name, parameters, .. } = unit else { continue };
        if !giving_up.contains(name.as_str()) {
            continue;
        }
        match parameters.iter_mut().find_map(|p| match p {
            FrozenUnit::Unverified(set) => Some(set),
            _ => None,
        }) {
            Some(set) if !set.contains(&Guarantee::BoundedSize) => set.push(Guarantee::BoundedSize),
            Some(_) => {}
            None => parameters.push(FrozenUnit::Unverified(vec![Guarantee::BoundedSize])),
        }
    }
    errors
}

/// The first string or dynamic array in `ty` without a maximum, as it's
/// written. A named type is its own declaration's position, checked there.
fn first_unbounded(ty: &TypeRef) -> Option<&TypeRef> {
    match ty {
        TypeRef::Primitive(PrimitiveType::String) => Some(ty),
        TypeRef::Array { length: None, max: None, .. } => Some(ty),
        TypeRef::Array { element, .. } => first_unbounded(element),
        TypeRef::Union(members) => members.iter().find_map(first_unbounded),
        _ => None,
    }
}
