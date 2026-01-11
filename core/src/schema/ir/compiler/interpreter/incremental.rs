// Standard Uses

// Local Uses
// use crate::schema::idl::ast::unit;
// use crate::schema::idl::ast::unit::ASTUnit;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use crate::schema::ir::compiler::Compile;
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses

#[allow(unused)]
pub struct IncrementalInterpreter {}

#[allow(unused)]
impl Compile for IncrementalInterpreter {
    type Output = Vec<FrozenUnit>;

    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output {
        tracing::debug!("Processing {} declarations...", declarations.len());

        let mut frozen_units: Vec<FrozenUnit> = vec![];

        for decl in declarations {
            match decl {
                Declaration::Import(import) => {
                    // Legacy import support
                    frozen_units.push(FrozenUnit::Import(import.path()));
                }
                Declaration::Use(use_stmt) => {
                    // New use statement - for now, just extract path
                    // TODO: Implement full use resolution with parent::, glob, multi, alias
                    let path_str = extract_use_path(&use_stmt.path);
                    frozen_units.push(FrozenUnit::Import(path_str));
                }
                Declaration::Const(const_decl) => {
                    let name = const_decl.name();
                    let type_def = const_decl.type_def();
                    let value = const_decl.value();

                    // Determine type name
                    let type_name = match type_def {
                        crate::schema::idl::grammar::Type::U8(_) => "u8",
                        crate::schema::idl::grammar::Type::U16(_) => "u16",
                        crate::schema::idl::grammar::Type::U32(_) => "u32",
                        crate::schema::idl::grammar::Type::U64(_) => "u64",
                        crate::schema::idl::grammar::Type::I8(_) => "i8",
                        crate::schema::idl::grammar::Type::I16(_) => "i16",
                        crate::schema::idl::grammar::Type::I32(_) => "i32",
                        crate::schema::idl::grammar::Type::I64(_) => "i64",
                        crate::schema::idl::grammar::Type::F32(_)
                        | crate::schema::idl::grammar::Type::F64(_) => "float",
                        crate::schema::idl::grammar::Type::Bool(_) => "bool",
                        crate::schema::idl::grammar::Type::Str(_) => "str",
                        crate::schema::idl::grammar::Type::String(_) => "string",
                        crate::schema::idl::grammar::Type::Named(id) => id.as_str(),
                        crate::schema::idl::grammar::Type::Array(_) => "array",
                    };

                    // Parse value
                    let kind_value = match (type_name, value) {
                        (
                            "u8" | "u16" | "u32" | "u64",
                            crate::schema::idl::grammar::Expression::Integer(int_lit),
                        ) => KindValue::Primitive(Primitive::U64(Some(int_lit.value() as u64))),
                        (
                            "i8" | "i16" | "i32" | "i64",
                            crate::schema::idl::grammar::Expression::Integer(int_lit),
                        ) => KindValue::Primitive(Primitive::S64(Some(int_lit.value()))),
                        ("bool", _) => KindValue::Primitive(Primitive::Boolean(Some(false))),
                        (
                            "str" | "string",
                            crate::schema::idl::grammar::Expression::String(str_lit),
                        ) => KindValue::Primitive(Primitive::String(Some(
                            str_lit.value().to_string(),
                        ))),
                        _ => KindValue::Namespaced(type_name.to_string(), None),
                    };

                    frozen_units.push(FrozenUnit::Constant {
                        docstring: None,
                        name,
                        kind_value,
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

                            let type_str = type_to_string(field_type);

                            FrozenUnit::Field {
                                docstring: None,
                                parameters: vec![],
                                optional: field.optional(),
                                name: fname,
                                kind_value: KindValue::Namespaced(type_str, None),
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Struct {
                        docstring: None,
                        parameters: vec![],
                        name: struct_name,
                        fields: field_units,
                    });
                }
                Declaration::Enum(enum_def) => {
                    let enum_name = enum_def.name();
                    let variants = enum_def.variants();

                    let variant_units: Vec<FrozenUnit> = variants
                        .iter()
                        .map(|variant| {
                            FrozenUnit::EnumVariant(KindValue::EnumVariant(
                                variant.identifier().to_string(),
                                None,
                            ))
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Enum {
                        docstring: None,
                        name: enum_name,
                        variants: variant_units,
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
                                        name: "arg0".to_string(),
                                        kind: type_to_kind_value(first_arg.arg_type()),
                                    }];

                                for (i, comma_arg) in rest_args.iter().enumerate() {
                                    let arg = comma_arg.arg_type();
                                    args.push(crate::schema::ir::frozen::unit::FrozenArgument {
                                        name: format!("arg{}", i + 1),
                                        kind: type_to_kind_value(arg.arg_type()),
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
                                docstring: String::new(),
                                throws: vec![],
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Protocol {
                        docstring: String::new(),
                        name: protocol_name,
                        functions: function_units,
                        parameters: vec![],
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
fn type_to_kind_value(type_def: &crate::schema::idl::grammar::Type) -> KindValue {
    KindValue::Namespaced(type_to_string(type_def), None)
}

fn type_to_string(type_def: &crate::schema::idl::grammar::Type) -> String {
    match type_def {
        crate::schema::idl::grammar::Type::U8(_) => "u8".to_string(),
        crate::schema::idl::grammar::Type::U16(_) => "u16".to_string(),
        crate::schema::idl::grammar::Type::U32(_) => "u32".to_string(),
        crate::schema::idl::grammar::Type::U64(_) => "u64".to_string(),
        crate::schema::idl::grammar::Type::I8(_) => "i8".to_string(),
        crate::schema::idl::grammar::Type::I16(_) => "i16".to_string(),
        crate::schema::idl::grammar::Type::I32(_) => "i32".to_string(),
        crate::schema::idl::grammar::Type::I64(_) => "i64".to_string(),
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
