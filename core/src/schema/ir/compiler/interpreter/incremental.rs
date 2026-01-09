// Standard Uses

// Local Uses
use crate::schema::idl::ast::unit;
use crate::schema::idl::ast::unit::ASTUnit;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::Compile;
use crate::schema::ir::compiler::interpreted::frozen_unit::FrozenUnit;
use crate::schema::ir::compiler::interpreted::primitive;
use crate::schema::ir::compiler::interpreted::primitive::KindValue;
use crate::schema::ir::compiler::interpreted::report::ReportDetails;
use crate::schema::ir::context::Context;

// External Uses


#[allow(unused)]
pub struct IncrementalInterpreter {
    context: Context
}

#[allow(unused)]
impl Compile for IncrementalInterpreter {
    type Output = ();

    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output {
        use crate::schema::ir::frozen::unit::FrozenUnit;
        use crate::schema::ir::compiler::interpreted::kind_search::KindValue;
        use crate::schema::ir::compiler::interpreted::primitive::Primitive;
        
        let mut frozen_units: Vec<FrozenUnit> = Vec::new();
        
        for decl in declarations {
            match decl {
                Declaration::Import(import) => {
                    frozen_units.push(FrozenUnit::Import(import.get_path()));
                }
                Declaration::Const(const_decl) => {
                    // Parse the type and value into KindValue
                    let type_name = const_decl.get_type_name();
                    let value_str = const_decl.get_value();
                    
                    let kind_value = match type_name.as_str() {
                        "u8" => KindValue::Primitive(Primitive::U8(value_str.parse().ok())),
                        "u16" => KindValue::Primitive(Primitive::U16(value_str.parse().ok())),
                        "u32" => KindValue::Primitive(Primitive::U32(value_str.parse().ok())),
                        "u64" => KindValue::Primitive(Primitive::U64(value_str.parse().ok())),
                        "i8" => KindValue::Primitive(Primitive::S8(value_str.parse().ok())),
                        "i16" => KindValue::Primitive(Primitive::S16(value_str.parse().ok())),
                        "i32" => KindValue::Primitive(Primitive::S32(value_str.parse().ok())),
                        "i64" => KindValue::Primitive(Primitive::S64(value_str.parse().ok())),
                        "f32" | "f64" => KindValue::Namespaced(type_name, None), // Floats not in Primitive enum
                        "bool" => KindValue::Primitive(Primitive::Boolean(value_str.parse().ok())),
                        "str" | "string" => {
                            // Remove quotes if present
                            let cleaned = value_str.trim_matches('"');
                            KindValue::Primitive(Primitive::String(Some(cleaned.to_string())))
                        }
                        _ => KindValue::Namespaced(type_name, None), // Custom type
                    };
                    
                    frozen_units.push(FrozenUnit::Constant {
                        docstring: None,
                        name: const_decl.get_name(),
                        kind_value,
                    });
                }
                Declaration::Struct(struct_def) => {
                    let field_units: Vec<FrozenUnit> = struct_def.get_fields()
                        .into_iter()
                        .map(|(field_name, field_type)| {
                            FrozenUnit::Field {
                                docstring: None,
                                parameters: vec![],
                                optional: false,
                                name: field_name,
                                kind_value: KindValue::Named(field_type, None),
                            }
                        })
                        .collect();
                    
                    frozen_units.push(FrozenUnit::Struct {
                        docstring: None,
                        parameters: vec![],
                        name: struct_def.get_name(),
                        fields: field_units,
                    });
                }
                Declaration::Enum(enum_def) => {
                    let variant_units: Vec<FrozenUnit> = enum_def.get_variants()
                        .into_iter()
                        .map(|variant_name| {
                            FrozenUnit::EnumVariant(
                                KindValue::EnumVariant(variant_name, None)
                            )
                        })
                        .collect();
                    
                    frozen_units.push(FrozenUnit::Enum {
                        docstring: None,
                        name: enum_def.get_name(),
                        variants: variant_units,
                    });
                }
                Declaration::Protocol(protocol) => {
                    // TODO: Implement function conversion
                    // For now, just create empty protocol
                    frozen_units.push(FrozenUnit::Protocol {
                        docstring: String::new(),
                        parameters: vec![],
                        name: protocol.get_name(),
                        functions: vec![], // TODO: Convert functions
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

#[allow(unused)]
impl IncrementalInterpreter {
    pub fn interpret_unit(&self) -> Result<Vec<FrozenUnit>, ReportDetails> {
        let mut interpreted: Vec<FrozenUnit> = vec![];

        for unit in &self.context.main.1 {
            use crate::schema::idl::ast::unit::ASTUnit::*;
            match unit {
                Namespace(n) => {
                    // let namespace = n;
                    interpreted.push(FrozenUnit::Namespace(n.clone()));
                },
                Import(_) => {
                    let import = self.interpret_node( unit)?;
                    interpreted.push(import);
                }
                Constant { .. } => {
                    let constant = self.interpret_node(unit)?;
                    interpreted.push(constant);
                }
                Enum { .. } => {
                    let r#enum = self.interpret_node( unit)?;
                    interpreted.push(r#enum);
                }
                /*
                Unit::Settings { .. } => {}
                Unit::Struct { .. } => {}
                Unit::Protocol { .. } => {}
                Unit::Error { .. } => {}
                Unit::Validator { .. } => {}
                */
                //r => panic!("Left to impl: {:?}", r)
                _ => {}
            }
        }


        Ok(interpreted)
    }

    pub fn interpret_node(&self, node: &ASTUnit) -> Result<FrozenUnit, ReportDetails> {
        use crate::schema::idl::ast::unit::ASTUnit::*;
        match node {
            Tag(_) => {

            }
            Namespace(n) => {
                let mut found: Option<&Context> = None;

                for relative_ctx in &self.context.relative_contexts {
                    if unit::namespace(&relative_ctx.main.1) == n {
                        if found.is_some() {
                            return Err(ReportDetails {
                                kind: "namespace".to_string(),
                                message: format!(
                                    "Found namespace {} when its already declared in {}",
                                    &n, &relative_ctx.main.0.filename()
                                ),
                                start: 0, end: 0,
                            })
                        }

                        found = Some(relative_ctx)
                    }
                }
            }
            Import(i) => {
                let relative_unit = self.context.find_whole_unit_by_import(&i);

                if relative_unit.is_none() {
                    let relative_unit = relative_unit.unwrap();

                    return Err(ReportDetails {
                        kind: "import".to_string(),
                        message: format!("Could not find namespace of {}", relative_unit.0.filename()),
                        start: 0, end: 0,
                    })
                }

                return Ok(FrozenUnit::Import(i.clone()))
            },
            Constant { name, kind, default_value, .. } => {
                let kind_value = primitive::to_kind_value(kind, default_value);

                return Ok(FrozenUnit::Constant {
                    docstring: None,
                    name: name.clone(), kind_value
                })
            }
            Enum { name, variants, .. } => {
                let mut frozen_variants: Vec<FrozenUnit> = vec![];

                for variant in variants {
                    pub(crate) fn to_variant(variant: &ASTUnit) -> KindValue {
                        match variant {
                            EnumVariant { name, kind } => {
                                if kind.is_none() {
                                    return KindValue::EnumVariant(
                                        name.clone(),None
                                    )
                                }

                                return KindValue::EnumVariant(
                                    name.clone(), None
                                )
                            },
                            _ => panic!("Should not be here")
                        }
                    }

                    frozen_variants.push(FrozenUnit::EnumVariant(
                        to_variant(variant, )
                    ));
                }

                return Ok(FrozenUnit::Enum {
                    docstring: None,
                    name: name.clone(), variants: frozen_variants
                })
            }
            /*
            EnumVariant { .. } => {}
            Settings { .. } => {}
            Struct { .. } => {}
            Protocol { .. } => {}
            Function { .. } => {}
            Error { .. } => {}
            Validator { .. } => {}
            Field { .. } => {}
            Parameter { .. } => {}
            Property { .. } => {}
            ExpressionBlock { .. } => {}
            */
            _ => {}
        }

        panic!()
    }

}

pub fn into_frozen_unit() -> FrozenUnit {
    todo!()
}
