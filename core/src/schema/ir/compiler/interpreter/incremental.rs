// Standard Uses

// Local Uses
use crate::schema::idl::ast::unit;
use crate::schema::idl::ast::unit::ASTUnit;
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

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output {
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
