//! A dynamic array of a type that carries no data.
//!
//! `packed@1`, the default wire format, writes `T[]` as a varint count and
//! then the elements, and its decoder refuses a count larger than the bytes
//! left - so a two-byte frame can't claim millions of elements. An element
//! that encodes to zero bytes defeats that guard: `vec![Empty {}]` encodes
//! to `01`, which the decoder then refuses. Rather than relax the decoder,
//! such an array is refused here, in the schema, whatever the format.
//!
//! A type carries no data when it is `()`, a struct whose every field
//! carries none (an empty struct included; an optional field carries its
//! presence byte), a fixed array `T[0]` or `T[N]` of such a `T`, or an
//! alias of one of these. Every primitive, enum (an index or a name),
//! union (a tag) and dynamic array (its count) carries data. `T[N]` of a
//! data-less `T` is allowed: no count is written.

use std::collections::HashMap;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::FrozenUnit;
use super::validator::ImportLookup;

const ARRAY_OF_DATALESS_TYPE: DiagnosticCode =
    DiagnosticCode::error("validation::array-of-dataless-type");

/// Whether `ty` puts at least one byte on the wire. `named` answers for a
/// name: it resolves the struct or alias behind it, and has to treat a name
/// it is already working out as carrying data, so a type that refers to
/// itself ends the walk (it can only be finite through something that
/// carries data: an option or a dynamic array).
pub(crate) fn carries_data(ty: &TypeRef, named: &mut dyn FnMut(&str) -> bool) -> bool {
    match ty {
        TypeRef::Unit => false,
        TypeRef::Primitive(_) | TypeRef::Union(_) => true,
        // A dynamic array writes its count.
        TypeRef::Array { length: None, .. } => true,
        TypeRef::Array { length: Some(0), .. } => false,
        TypeRef::Array { element, .. } => carries_data(element, named),
        TypeRef::Named(name) => named(name),
    }
}

/// The answers [`carries_data`] has found, by name: `None` while a name is
/// still being worked out (the cycle guard), `Some` once it is known.
pub(crate) type Answers = HashMap<String, Option<bool>>;

/// [`carries_data`] for one schema's frozen units, asking `imports` for a
/// name the schema doesn't declare.
struct Local<'a> {
    declared: HashMap<&'a str, &'a FrozenUnit>,
    imports: &'a dyn ImportLookup,
    answers: Answers,
}

impl<'a> Local<'a> {
    fn new(units: &'a [FrozenUnit], imports: &'a dyn ImportLookup) -> Self {
        let declared = units
            .iter()
            .filter_map(|unit| match unit {
                FrozenUnit::Struct { name, .. }
                | FrozenUnit::Enum { name, .. }
                | FrozenUnit::TypeAlias { name, .. } => Some((name.as_str(), unit)),
                _ => None,
            })
            .collect();
        Self { declared, imports, answers: Answers::new() }
    }

    fn carries(&mut self, ty: &TypeRef) -> bool {
        carries_data(ty, &mut |name| self.named(name))
    }

    fn named(&mut self, name: &str) -> bool {
        match self.answers.get(name) {
            Some(Some(known)) => return *known,
            // Already being worked out: it refers to itself.
            Some(None) => return true,
            None => {}
        }
        self.answers.insert(name.to_string(), None);
        let unit: Option<&'a FrozenUnit> = self.declared.get(name).copied();
        let carries = match unit {
            Some(FrozenUnit::Struct { fields, .. }) => fields.iter().any(|field| match field {
                FrozenUnit::Field { optional, ty, .. } => *optional || self.carries(ty),
                _ => false,
            }),
            Some(FrozenUnit::TypeAlias { target, .. }) => self.carries(target),
            // An enum: an index or a name.
            Some(_) => true,
            // Not declared here: what the declaring schema says, and
            // carrying data when nothing can say (an unknown type is
            // reported on its own).
            None => self.imports.carries_data(name).unwrap_or(true),
        };
        self.answers.insert(name.to_string(), Some(carries));
        carries
    }
}

/// Refuse every `T[]` whose `T` carries no data, wherever a type is
/// written: a struct or error field, a function's argument or return, an
/// alias's target, and nested in an array or a union.
pub(super) fn check_dataless_arrays(
    units: &[FrozenUnit],
    imports: &dyn ImportLookup,
    errors: &mut Vec<Diagnostic>,
) {
    let mut data = Local::new(units, imports);
    let mut check = |ty: &TypeRef, context: String, span: Option<(usize, usize)>| {
        refuse_dataless_arrays(ty, &mut data, &context, span, errors);
    };
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields, .. } => {
                for field in fields {
                    if let FrozenUnit::Field { name: field, ty, span, .. } = field {
                        check(ty, format!("Struct '{name}', field '{field}'"), Some(*span));
                    }
                }
            }
            // A re-export slot is checked where it is declared.
            FrozenUnit::Error { name, fields, imported_from: None, .. } => {
                for field in fields {
                    if let FrozenUnit::Field { name: field, ty, span, .. } = field {
                        check(ty, format!("Error '{name}', field '{field}'"), Some(*span));
                    }
                }
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                for function in functions {
                    let FrozenUnit::Function { name: function, arguments, _return, span, .. } =
                        function
                    else {
                        continue;
                    };
                    for argument in arguments {
                        check(
                            &argument.ty,
                            format!(
                                "Protocol '{name}', function '{function}', arg '{}'",
                                argument.name
                            ),
                            Some(argument.span),
                        );
                    }
                    if let Some(returned) = _return {
                        check(
                            returned,
                            format!("Protocol '{name}', function '{function}' return"),
                            Some(*span),
                        );
                    }
                }
            }
            FrozenUnit::TypeAlias { name, target, span, .. } => {
                check(target, format!("Type alias '{name}'"), Some(*span));
            }
            _ => {}
        }
    }
}

fn refuse_dataless_arrays(
    ty: &TypeRef,
    data: &mut Local,
    context: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    match ty {
        TypeRef::Array { element, length } => {
            if length.is_none() && !data.carries(element) {
                let help = match &**element {
                    TypeRef::Named(name) => {
                        format!("give `{name}` a field, or use a count (`u32`) instead")
                    }
                    _ => "use a count (`u32`) instead".to_string(),
                };
                errors.push(
                    Diagnostic::new(
                        ARRAY_OF_DATALESS_TYPE,
                        format!(
                            "an array of `{element}` carries only its length: `{element}` has \
                             no data, so each element would take no bytes"
                        ),
                    )
                    .with_context(context.to_string())
                    .with_help(help)
                    .maybe_span(span),
                );
            }
            refuse_dataless_arrays(element, data, context, span, errors);
        }
        TypeRef::Union(members) => {
            for member in members {
                refuse_dataless_arrays(member, data, context, span, errors);
            }
        }
        TypeRef::Named(_) | TypeRef::Primitive(_) | TypeRef::Unit => {}
    }
}
