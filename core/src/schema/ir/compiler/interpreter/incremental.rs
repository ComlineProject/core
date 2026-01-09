// Standard Uses

// Local Uses
use crate::schema::idl::ast::unit;
use crate::schema::idl::ast::unit::ASTUnit;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::Compile;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use crate::schema::ir::frozen::unit::FrozenArgument;

// External Uses


#[allow(unused)]
pub struct IncrementalInterpreter {
    context: crate::schema::ir::context::Context
}

#[allow(unused)]
impl Compile for IncrementalInterpreter {
    type Output = ();

    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output {
        println!("Processing {} declarations...", declarations.len());
        
        let mut frozen_units: Vec<FrozenUnit> = vec![];
        
        for decl in declarations {
            match decl {
                Declaration::Import(import) => {
                    // Import(Identifier) - access via pattern matching
                    let crate::schema::idl::grammar::Import(id) = import;
                    let crate::schema::idl::grammar::Identifier(name) = id;
                    frozen_units.push(FrozenUnit::Import(name));
                }
                Declaration::Const(const_decl) => {
                    // Const((), Identifier, (), Type, (), Expression)
                    let crate::schema::idl::grammar::Const(_, name, _, type_def, _, value) = const_decl;
                    let crate::schema::idl::grammar::Identifier(name_str) = name;
                    
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
                        crate::schema::idl::grammar::Type::Bool(_) => "bool",
                        crate::schema::idl::grammar::Type::Str(_) => "str",
                        crate::schema::idl::grammar::Type::String(_) => "string",
                        crate::schema::idl::grammar::Type::Custom(id) => {
                            let crate::schema::idl::grammar::Identifier(s) = id;
                            s.as_str()
                        }
                        crate::schema::idl::grammar::Type::Array(_) => "array",
                    };
                    
                    // Parse value
                    let kind_value = match (type_name, value) {
                        ("u8" | "u16" | "u32" | "u64", crate::schema::idl::grammar::Expression::Integer(int_lit)) => {
                            let crate::schema::idl::grammar::IntegerLiteral(val) = int_lit;
                            KindValue::Primitive(Primitive::U64(Some(val as u64)))
                        }
                        ("i8" | "i16" | "i32" | "i64", crate::schema::idl::grammar::Expression::Integer(int_lit)) => {
                            let crate::schema::idl::grammar::IntegerLiteral(val) = int_lit;
                            KindValue::Primitive(Primitive::S64(Some(val)))
                        }
                        ("bool", _) => KindValue::Primitive(Primitive::Boolean(Some(false))),
                        ("str" | "string", crate::schema::idl::grammar::Expression::String(str_lit)) => {
                            let crate::schema::idl::grammar::StringLiteral(s) = str_lit;
                            KindValue::Primitive(Primitive::String(Some(s)))
                        }
                        _ => KindValue::Namespaced(type_name.to_string(), None),
                    };
                    
                    frozen_units.push(FrozenUnit::Constant {
                        docstring: None,
                        name: name_str,
                        kind_value,
                    });
                }
                Declaration::Struct(struct_def) => {
                    // Struct((), Identifier, (), Vec<Field>, ())
                    let crate::schema::idl::grammar::Struct(_, name, _, fields, _) = struct_def;
                    let crate::schema::idl::grammar::Identifier(struct_name) = name;
                    
                    let field_units: Vec<FrozenUnit> = fields
                        .into_iter()
                        .map(|field| {
                            // Field(Identifier, (), Type)
                            let crate::schema::idl::grammar::Field(field_name, _, field_type) = field;
                            let crate::schema::idl::grammar::Identifier(fname) = field_name;
                            
                            let type_str = match field_type {
                                crate::schema::idl::grammar::Type::U8(_) => "u8".to_string(),
                                crate::schema::idl::grammar::Type::U16(_) => "u16".to_string(),
                                crate::schema::idl::grammar::Type::U32(_) => "u32".to_string(),
                                crate::schema::idl::grammar::Type::U64(_) => "u64".to_string(),
                                crate::schema::idl::grammar::Type::I8(_) => "i8".to_string(),
                                crate::schema::idl::grammar::Type::I16(_) => "i16".to_string(),
                                crate::schema::idl::grammar::Type::I32(_) => "i32".to_string(),
                                crate::schema::idl::grammar::Type::I64(_) => "i64".to_string(),
                                crate::schema::idl::grammar::Type::Bool(_) => "bool".to_string(),
                                crate::schema::idl::grammar::Type::Str(_) => "str".to_string(),
                                crate::schema::idl::grammar::Type::String(_) => "string".to_string(),
                                crate::schema::idl::grammar::Type::Custom(id) => {
                                    let crate::schema::idl::grammar::Identifier(s) = id;
                                    s
                                }
                                crate::schema::idl::grammar::Type::Array(arr) => {
                                    let crate::schema::idl::grammar::ArrayType(inner, _size) = arr;
                                    format!("{}[]", match *inner {
                                        crate::schema::idl::grammar::Type::U64(_) => "u64",
                                        crate::schema::idl::grammar::Type::Str(_) => "str",
                                        crate::schema::idl::grammar::Type::Custom(ref id) => {
                                            let crate::schema::idl::grammar::Identifier(ref s) = id;
                                            s.as_str()
                                        }
                                        _ => "unknown",
                                    })
                                }
                            };
                            
                            FrozenUnit::Field {
                                docstring: None,
                                parameters: vec![],
                                optional: false,
                                name: fname,
                                kind_value: KindValue::Namespaced(type_str, None),
                            }
                        })
                        .collect();
                    
                    frozen_units.push(FrozenUnit::Struct {
                        docstring: None,
                        name: struct_name,
                        fields: field_units,
                    });
                }
                Declaration::Enum(enum_def) => {
                    // Enum((), Identifier, (), Vec<Identifier>, ())
                    let crate::schema::idl::grammar::Enum(_, name, _, variants, _) = enum_def;
                    let crate::schema::idl::grammar::Identifier(enum_name) = name;
                    
                    let variant_units: Vec<FrozenUnit> = variants
                        .into_iter()
                        .map(|variant| {
                            let crate::schema::idl::grammar::Identifier(variant_name) = variant;
                            FrozenUnit::EnumVariant(KindValue::EnumVariant(
                                variant_name,
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
                    // Protocol((), Identifier, (), Vec<Function>, ())
                    let crate::schema::idl::grammar::Protocol(_, name, _, functions, _) = protocol;
                    let crate::schema::idl::grammar::Identifier(protocol_name) = name;
                    
                    let function_units: Vec<FrozenUnit> = functions
                        .into_iter()
                        .map(|func| {
                            // Function((), Identifier, (), Option<ArgumentList>, (), Option<Type>)
                            let crate::schema::idl::grammar::Function(_, fname, _, args_opt, _, ret_opt) = func;
                            let crate::schema::idl::grammar::Identifier(func_name) = fname;
                            
                            let arguments = if let Some(arg_list) = args_opt {
                                // ArgumentList(Type, Vec<CommaArgument>)
                                let crate::schema::idl::grammar::ArgumentList(first_arg, rest_args) = arg_list;
                                
                                let mut args = vec![FrozenArgument {
                                    name: "arg0".to_string(),
                                    kind_value: type_to_kind_value(first_arg),
                                }];
                                
                                for (i, comma_arg) in rest_args.into_iter().enumerate() {
                                    // CommaArgument((), Type)
                                    let crate::schema::idl::grammar::CommaArgument(_, arg_type) = comma_arg;
                                    args.push(FrozenArgument {
                                        name: format!("arg{}", i + 1),
                                        kind_value: type_to_kind_value(arg_type),
                                    });
                                }
                                args
                            } else {
                                vec![]
                            };
                            
                            let return_type = ret_opt.map(type_to_kind_value);
                            
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
                        docstring: None,
                        name: protocol_name,
                        functions: function_units,
                    });
                }
            }
        }
        
        println!("Generated {} IR units", frozen_units.len());
        for unit in &frozen_units {
            println!("  {:?}", unit);
        }
    }

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output {
        // Legacy implementation
        todo!()
    }

    fn from_sourced_whole(sourced: crate::schema::idl::ast::unit::SourcedWholeRc) -> Self::Output {
        // Legacy implementation  
        todo!()
    }
}

// Helper function to convert Type to KindValue
fn type_to_kind_value(type_def: crate::schema::idl::grammar::Type) -> KindValue {
    match type_def {
        crate::schema::idl::grammar::Type::U8(_) => KindValue::Namespaced("u8".to_string(), None),
        crate::schema::idl::grammar::Type::U16(_) => KindValue::Namespaced("u16".to_string(), None),
        crate::schema::idl::grammar::Type::U32(_) => KindValue::Namespaced("u32".to_string(), None),
        crate::schema::idl::grammar::Type::U64(_) => KindValue::Namespaced("u64".to_string(), None),
        crate::schema::idl::grammar::Type::I8(_) => KindValue::Namespaced("i8".to_string(), None),
        crate::schema::idl::grammar::Type::I16(_) => KindValue::Namespaced("i16".to_string(), None),
        crate::schema::idl::grammar::Type::I32(_) => KindValue::Namespaced("i32".to_string(), None),
        crate::schema::idl::grammar::Type::I64(_) => KindValue::Namespaced("i64".to_string(), None),
        crate::schema::idl::grammar::Type::Bool(_) => KindValue::Namespaced("bool".to_string(), None),
        crate::schema::idl::grammar::Type::Str(_) => KindValue::Namespaced("str".to_string(), None),
        crate::schema::idl::grammar::Type::String(_) => KindValue::Namespaced("string".to_string(), None),
        crate::schema::idl::grammar::Type::Custom(id) => {
            let crate::schema::idl::grammar::Identifier(s) = id;
            KindValue::Namespaced(s, None)
        }
        crate::schema::idl::grammar::Type::Array(arr) => {
            let crate::schema::idl::grammar::ArrayType(inner, _) = arr;
            let inner_str = match *inner {
                crate::schema::idl::grammar::Type::U64(_) => "u64",
                crate::schema::idl::grammar::Type::Str(_) => "str",
                crate::schema::idl::grammar::Type::Custom(ref id) => {
                    let crate::schema::idl::grammar::Identifier(ref s) = id;
                    s.as_str()
                }
                _ => "unknown",
            };
            KindValue::Namespaced(format!("{}[]", inner_str), None)
        }
    }
}
