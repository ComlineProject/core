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

use std::collections::HashMap;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{Declaration, SizeValue, Type};
use crate::schema::ir::compiler::import_resolver::size_constant;

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
