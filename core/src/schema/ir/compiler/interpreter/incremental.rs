// Standard Uses

// Crate Uses
// use crate::schema::idl::ast::unit;
// use crate::schema::idl::ast::unit::ASTUnit;
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{Annotation, Declaration, UsePath};
use crate::schema::ir::compiler::import_resolver::{
    declared_symbol_names, resolve_use_to_schema, schema_declares_symbol, ImportResolver,
};
use crate::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use crate::schema::ir::compiler::Compile;
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses

#[allow(unused)]
pub struct IncrementalInterpreter {}

impl IncrementalInterpreter {
    /// Compile declarations with awareness of the rest of the project, so that
    /// `use`/`import` declarations pointing at other schemas in the same
    /// package resolve to real, verified references instead of raw path
    /// strings.
    pub fn from_declarations_with_context(
        declarations: Vec<rust_sitter::Spanned<Declaration>>,
        current_namespace: &[String],
        project_context: &ProjectContext,
    ) -> Vec<FrozenUnit> {
        Self::compile_declarations(declarations, Some((current_namespace, project_context)))
    }
}

#[allow(unused)]
impl Compile for IncrementalInterpreter {
    type Output = Vec<FrozenUnit>;

    fn from_declarations(declarations: Vec<rust_sitter::Spanned<Declaration>>) -> Self::Output {
        Self::compile_declarations(declarations, None)
    }
}

#[allow(unused)]
impl IncrementalInterpreter {
    fn compile_declarations(
        declarations: Vec<rust_sitter::Spanned<Declaration>>,
        use_context: Option<(&[String], &ProjectContext)>,
    ) -> Vec<FrozenUnit> {
        tracing::debug!("Processing {} declarations...", declarations.len());

        let mut frozen_units: Vec<FrozenUnit> = vec![];

        for spanned_decl in declarations {
            let span = spanned_decl.span;

            match spanned_decl.value {
                Declaration::Import(import) => {
                    // Legacy import support
                    frozen_units.push(FrozenUnit::Import(import.path(), span));
                }
                Declaration::Use(use_stmt) => {
                    let units = match use_context {
                        Some((current_namespace, project_context)) => resolve_use_declaration(
                            project_context,
                            current_namespace,
                            &use_stmt.path,
                            span,
                        ),
                        None => vec![FrozenUnit::Import(extract_use_path(&use_stmt.path), span)],
                    };
                    frozen_units.extend(units);
                }
                Declaration::Const(const_decl) => {
                    let name = const_decl.name();
                    let type_def = const_decl.type_def();
                    let value = const_decl.value();

                    let kind_value = build_kind_value(type_def, Some(value));

                    frozen_units.push(FrozenUnit::Constant {
                        docstring: const_decl.docstring(),
                        name,
                        kind_value,
                        span,
                    });
                }
                Declaration::Struct(struct_def) => {
                    let struct_name = struct_def.name();
                    let fields = struct_def.fields();

                    let field_units: Vec<FrozenUnit> = fields
                        .iter()
                        .map(|field| {
                            let fname = field.name();
                            let field_type = field.field_type();

                            let kind_value = build_kind_value(field_type, field.default_value());

                            FrozenUnit::Field {
                                docstring: field.docstring(),
                                parameters: annotation_units(&field.annotations()),
                                optional: field.optional(),
                                name: fname,
                                kind_value,
                                span: field.span,
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Struct {
                        docstring: struct_def.docstring(),
                        parameters: annotation_units(&struct_def.annotations()),
                        name: struct_name,
                        fields: field_units,
                        span,
                    });
                }
                Declaration::Enum(enum_def) => {
                    let enum_name = enum_def.name();
                    let variants = enum_def.variants();

                    let variant_units: Vec<FrozenUnit> = variants
                        .iter()
                        .map(|variant| {
                            FrozenUnit::EnumVariant(
                                KindValue::EnumVariant(variant.identifier().to_string(), None),
                                variant.span,
                            )
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Enum {
                        docstring: enum_def.docstring(),
                        name: enum_name,
                        variants: variant_units,
                        span,
                    });
                }
                Declaration::Protocol(protocol) => {
                    let protocol_name = protocol.name();
                    let functions = protocol.functions();

                    let function_units: Vec<FrozenUnit> = functions
                        .iter()
                        .map(|func| {
                            let func_name = func.name();
                            let args_opt = func.args();
                            let ret_opt = func.return_type();

                            let arguments = if let Some(arg_list) = args_opt {
                                let first_arg = arg_list.first();
                                let rest_args = arg_list.rest();

                                let mut args =
                                    vec![crate::schema::ir::frozen::unit::FrozenArgument {
                                        name: first_arg
                                            .name()
                                            .map(|n| n.as_str().to_string())
                                            .unwrap_or_else(|| "arg0".to_string()),
                                        kind: type_to_kind_value(first_arg.arg_type()),
                                        span: first_arg.arg_type_span(),
                                    }];

                                for (i, comma_arg) in rest_args.iter().enumerate() {
                                    let arg = comma_arg.arg_type();
                                    args.push(crate::schema::ir::frozen::unit::FrozenArgument {
                                        name: arg
                                            .name()
                                            .map(|n| n.as_str().to_string())
                                            .unwrap_or_else(|| format!("arg{}", i + 1)),
                                        kind: type_to_kind_value(arg.arg_type()),
                                        span: arg.arg_type_span(),
                                    });
                                }
                                args
                            } else {
                                vec![]
                            };

                            let return_type = ret_opt
                                .as_ref()
                                .map(|rt| type_to_kind_value(rt.return_type()));

                            FrozenUnit::Function {
                                name: func_name,
                                arguments,
                                _return: return_type,
                                synchronous: true,
                                docstring: func.docstring().unwrap_or_default(),
                                throws: func.throws().into_iter().collect(),
                                span: func.span,
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Protocol {
                        docstring: protocol.docstring().unwrap_or_default(),
                        name: protocol_name,
                        functions: function_units,
                        parameters: annotation_units(&protocol.annotations()),
                        span,
                    });
                }
                Declaration::Error(error_decl) => {
                    let error_name = error_decl.name();
                    let message = error_decl.message();
                    let fields = error_decl.fields();

                    let field_units: Vec<FrozenUnit> = fields
                        .iter()
                        .map(|field| {
                            let fname = field.name();
                            let field_type = field.field_type();

                            let kind_value = build_kind_value(field_type, field.default_value());

                            FrozenUnit::Field {
                                docstring: field.docstring(),
                                parameters: annotation_units(&field.annotations()),
                                optional: field.optional(),
                                name: fname,
                                kind_value,
                                span: field.span,
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Error {
                        docstring: error_decl.docstring(),
                        parameters: vec![],
                        name: error_name,
                        message,
                        fields: field_units,
                    });
                }
                Declaration::Settings(settings_def) => {
                    let parameters: Vec<FrozenUnit> = settings_def
                        .entries()
                        .iter()
                        .map(|entry| FrozenUnit::Parameter {
                            name: entry.key(),
                            default_value: entry.value_string(),
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Settings {
                        docstring: settings_def.docstring(),
                        name: settings_def.name(),
                        parameters,
                    });
                }
                Declaration::Validator(validator_def) => {
                    let properties: Vec<FrozenUnit> = validator_def
                        .properties()
                        .iter()
                        .map(|prop| FrozenUnit::Field {
                            docstring: prop.docstring(),
                            parameters: vec![],
                            optional: false,
                            name: prop.name(),
                            kind_value: build_kind_value(
                                prop.property_type(),
                                prop.default_value(),
                            ),
                            span: prop.span,
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Validator {
                        docstring: validator_def.docstring(),
                        properties,
                        name: validator_def.name(),
                        // Each `assert(...)` from the `validate` block, as text
                        // (validators phase 1b — not yet an evaluated AST).
                        expression_block: Box::new(FrozenUnit::ExpressionBlock {
                            function_calls: validator_def.validate_asserts(),
                        }),
                    });
                }
            }
        }

        tracing::debug!("Generated {} IR units", frozen_units.len());
        for unit in &frozen_units {
            tracing::trace!("  {:?}", unit);
        }
        // Return the generated IR units for testing/validation
        frozen_units
    }

    /*
    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output {
        // Legacy implementation
        todo!()
    }

    fn from_sourced_whole(sourced: crate::schema::idl::ast::unit::SourcedWholeRc) -> Self::Output {
        // Legacy implementation
        todo!()
    }
    */
}

/// Turn a declaration's `@key = value` annotations into frozen units — one
/// `FrozenUnit::Property { name, expression: Some(value) }` per annotation, in
/// source order. Empty when there are none.
fn annotation_units(annotations: &[&Annotation]) -> Vec<FrozenUnit> {
    annotations
        .iter()
        .map(|a| FrozenUnit::Property {
            name: a.key(),
            expression: Some(a.value()),
        })
        .collect()
}

fn type_to_kind_value(type_def: &crate::schema::idl::grammar::Type) -> KindValue {
    build_kind_value(type_def, None)
}

/// Build a `KindValue` for a type, optionally carrying a literal default
/// value (used by both `const NAME: TYPE = VALUE` and a struct field's
/// own optional `= VALUE` default - `None` for anything with no default,
/// e.g. function arguments/return types).
///
/// A default is only captured when it's a simple literal matching the
/// type's own primitive kind (an integer for a sized int, a string for
/// `str`/`string`, etc.) - anything else (an identifier/const reference,
/// a non-primitive type) falls back to the same unvalued `Namespaced`
/// representation as if no default had been given at all. This mirrors
/// `const`'s pre-existing behavior exactly; it isn't a new limitation
/// introduced for fields.
fn build_kind_value(
    type_def: &crate::schema::idl::grammar::Type,
    value: Option<&crate::schema::idl::grammar::Expression>,
) -> KindValue {
    if let crate::schema::idl::grammar::Type::Union(union_type) = type_def {
        return KindValue::Union(
            union_type
                .members()
                .iter()
                .map(|member| build_kind_value(member, None))
                .collect(),
        );
    }

    let type_name = type_to_string(type_def);

    match (type_name.as_str(), value) {
        (
            "u8" | "u16" | "u32" | "u64",
            Some(crate::schema::idl::grammar::Expression::Integer(int_lit)),
        ) => KindValue::Primitive(Primitive::U64(Some(int_lit.value() as u64))),
        (
            "s8" | "s16" | "s32" | "s64",
            Some(crate::schema::idl::grammar::Expression::Integer(int_lit)),
        ) => KindValue::Primitive(Primitive::S64(Some(int_lit.value()))),
        ("bool", Some(_)) => KindValue::Primitive(Primitive::Boolean(Some(false))),
        (
            "str" | "string",
            Some(crate::schema::idl::grammar::Expression::String(str_lit)),
        ) => KindValue::Primitive(Primitive::String(Some(str_lit.value().to_string()))),
        _ => KindValue::Namespaced(type_name, None),
    }
}

fn type_to_string(type_def: &crate::schema::idl::grammar::Type) -> String {
    match type_def {
        crate::schema::idl::grammar::Type::U8(_) => "u8".to_string(),
        crate::schema::idl::grammar::Type::U16(_) => "u16".to_string(),
        crate::schema::idl::grammar::Type::U32(_) => "u32".to_string(),
        crate::schema::idl::grammar::Type::U64(_) => "u64".to_string(),
        crate::schema::idl::grammar::Type::S8(_) => "s8".to_string(),
        crate::schema::idl::grammar::Type::S16(_) => "s16".to_string(),
        crate::schema::idl::grammar::Type::S32(_) => "s32".to_string(),
        crate::schema::idl::grammar::Type::S64(_) => "s64".to_string(),
        crate::schema::idl::grammar::Type::F32(_) | crate::schema::idl::grammar::Type::F64(_) => {
            "float".to_string()
        }
        crate::schema::idl::grammar::Type::Bool(_) => "bool".to_string(),
        crate::schema::idl::grammar::Type::Str(_) => "str".to_string(),
        crate::schema::idl::grammar::Type::String(_) => "string".to_string(),
        crate::schema::idl::grammar::Type::Named(id) => id.to_string(),
        crate::schema::idl::grammar::Type::Array(arr) => {
            format!("{}[]", type_to_string(arr.elem_type()))
        }
        crate::schema::idl::grammar::Type::Union(union_type) => {
            let members: Vec<String> = union_type.members().iter().map(type_to_string).collect();
            format!("union({})", members.join(" "))
        }
    }
}

/// Extract path string from UsePath enum
/// TODO: This is a placeholder - should integrate with full resolver
fn extract_use_path(use_path: &crate::schema::idl::grammar::UsePath) -> String {
    use crate::schema::idl::grammar::UsePath;

    match use_path {
        UsePath::Absolute(scoped) => scoped.to_string(),
        UsePath::Relative(rel) => {
            // Convert parent::path to absolute later in resolver
            format!("{:?}::{}", rel.prefix, rel.path.to_string())
        }
        UsePath::Glob(glob) => {
            format!("{}::*", glob.path.to_string())
        }
        UsePath::Multi(multi) => {
            // For now, just use the base path
            // TODO: Handle multi-imports properly
            multi.path.to_string()
        }
    }
}

/// Resolve a `use` declaration against the rest of the project, emitting one
/// `FrozenUnit::Import` per resolved target (multiple for a multi-import).
///
/// Only same-package targets are actually located and symbol-checked today;
/// external dependency/stdlib paths still resolve to a namespace string (via
/// `ImportResolver`) but aren't loaded/merged into this schema's context yet.
fn resolve_use_declaration(
    project_context: &ProjectContext,
    current_namespace: &[String],
    use_path: &UsePath,
    span: (usize, usize),
) -> Vec<FrozenUnit> {
    let resolver = ImportResolver::new(vec![], Default::default(), None);

    let target = match resolve_use_to_schema(project_context, &resolver, current_namespace, use_path) {
        Ok(target) => target,
        Err(message) => {
            tracing::warn!("Failed to resolve use path: {}", message);
            return vec![FrozenUnit::Import(format!("<unresolved: {}>", message), span)];
        }
    };

    let joined_namespace = target.resolved.absolute_namespace.join("::");

    // Glob import: `use ns::*;`
    if target.resolved.symbols == ["*".to_string()] {
        let mut units = vec![FrozenUnit::Import(format!("{}::*", joined_namespace), span)];
        if let Some(schema) = &target.schema {
            units.extend(declared_symbol_names(&schema.borrow()).into_iter().map(|name| {
                FrozenUnit::Import(format!("{}::{}", joined_namespace, name), span)
            }));
        }
        return units;
    }

    // Item imports: `use ns::{A, B};`
    if !target.resolved.symbols.is_empty() {
        return target
            .resolved
            .symbols
            .iter()
            .map(|item| {
                if !matches!(&target.schema, Some(schema) if schema_declares_symbol(&schema.borrow(), item))
                {
                    tracing::warn!("Symbol '{}' not found in schema '{}'", item, joined_namespace);
                }
                FrozenUnit::Import(format!("{}::{}", joined_namespace, item), span)
            })
            .collect();
    }

    // Whole-namespace or single-symbol import (`use ns;` / `use ns::Symbol;`)
    match &target.schema {
        Some(schema) if target.remaining.is_empty() => {
            let ns = schema.borrow().namespace_joined();
            let mut units = vec![FrozenUnit::Import(ns.clone(), span)];
            units.extend(
                declared_symbol_names(&schema.borrow())
                    .into_iter()
                    .map(|name| FrozenUnit::Import(format!("{}::{}", ns, name), span)),
            );
            units
        }
        Some(schema) => {
            let symbol = target.remaining.join("::");
            let schema_namespace = schema.borrow().namespace_joined();

            if !schema_declares_symbol(&schema.borrow(), &symbol) {
                tracing::warn!("Symbol '{}' not found in schema '{}'", symbol, schema_namespace);
            }

            vec![FrozenUnit::Import(format!("{}::{}", schema_namespace, symbol), span)]
        }
        // Not part of this project (external dependency, stdlib, or genuinely
        // unresolved) - fall back to the raw resolved namespace.
        None => vec![FrozenUnit::Import(joined_namespace, span)],
    }
}
