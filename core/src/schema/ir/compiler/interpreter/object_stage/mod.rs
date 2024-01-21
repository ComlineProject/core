// Relative Modules
mod report;

// Standard Uses
use std::rc::Rc;
use std::cell::{Ref, RefCell};

// Crate Uses
use crate::schema::ir::context::SchemaContext;
use crate::schema::idl::ast::unit::{ASTUnit, SpannedUnit};
use crate::schema::ir::compiler::interpreted::kind_search;
use crate::schema::ir::compiler::interpreted::kind_search::{KindValue};
use crate::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};
use crate::package::config::ir::context::ProjectContext;

// External Uses


pub fn compile_schema(
    schema_context: Rc<RefCell<SchemaContext>>,
    project_context: &ProjectContext
) -> Result<(), Box<dyn snafu::Error>> {
    let schema_ctx = schema_context.borrow();
    let mut interpreted: Vec<FrozenUnit> = vec![];

    let namespace = schema_ctx.namespace_joined();
    {
        // TODO: Goal here is to check if there are namespaces colliding within schemas
        //       which should be improbable if its source tree based, so think about if
        //       its necessary to check
        /*
        let relative_schema =
            project_context.find_schema_by_import(&namespace).unwrap();

        if relative_schema.is_some() {
            return report::colliding_namespace_err(relative_schema)
        }
        */
        interpreted.push(FrozenUnit::Namespace(namespace));
    }

    for spanned_unit in &schema_ctx.schema.1 {
        use crate::schema::idl::ast::unit::ASTUnit::*;
        match &spanned_unit.1 {
            // TODO: ASTs are not supposed to have a namespace unit, because its automatically
            //       decided based on source folder structure, remove this later
            Namespace { .. } => { todo!() },
            Docstring {..} => { todo!() }
            Import(s, i) => {
                let Some(import_ctx)
                    = project_context.find_schema_by_import(i) else {
                    panic!("No schema found by import '{}' at '{}'", i, s.0)
                };

                let relative_unit = project_context.find_schema_by_import(i);

                /*
                if relative_unit.is_none() {
                    let relative_unit = relative_unit.unwrap();

                    return Err(ReportDetails {
                        kind: "import".to_string(),
                        message: format!("Could not find namespace of {}", relative_unit.0.filename()),
                        start: 0, end: 0,
                    })
                }
                */
            }
            Constant {
                docstring, name,
                kind, default_value
            } => {
                let state = schema_ctx.compile_state.borrow();

                let kind_value = kind_search::resolve_kind_value(
                    &schema_ctx, project_context, kind, default_value
                )?;

                interpreted.push(FrozenUnit::Constant {
                    docstring: None,
                    name: name.1.clone(),
                    kind_value,
                });
            }
            Property { .. } => {}
            Parameter { .. } => {}
            ExpressionBlock { .. } => {}
            Enum { .. } => {}
            EnumVariant { .. } => {}
            Settings { .. } => {}
            Struct { .. } => {}
            Protocol {
                docstring, parameters,
                name, functions
            } => {
                let state = schema_ctx.compile_state.borrow();

                let mut params = vec![];
                let mut fns = vec![];
                for function in functions {
                    fns.push(to_function_frozen(
                        &schema_ctx, project_context, function
                    )?);
                }

                interpreted.push(FrozenUnit::Protocol {
                    docstring: "".to_string(),
                    parameters: params,
                    name: name.1.clone(),
                    functions: fns,
                });
            }
            Function { name, .. } => {
            }
            Argument { .. } => {}
            Error { .. } => {}
            Validator { .. } => {}
            Field { .. } => {}
        }
    }

    drop(schema_ctx);
    let mut schema_ctx = RefCell::borrow_mut(&schema_context);
    schema_ctx.frozen_schema = Some(interpreted);

    schema_ctx.compile_state.borrow_mut().complete = true;

    Ok(())
}

#[allow(unused)]
pub fn interpret_node<'a>(
    schema_context: &'a SchemaContext,
    project_context: &'a ProjectContext,
    node: &ASTUnit
) -> Result<FrozenUnit, Box<dyn snafu::Error>> {
    use crate::schema::idl::ast::unit::ASTUnit::*;
    match node {
        Namespace(_, n) => {

        }
        Import(_, i) => {
            return Ok(FrozenUnit::Import(i.clone()))
        }
        Constant {
            name, kind: (_, kind),
            default_value, ..
        } => {
            let state = schema_context.compile_state.borrow();
            /*
            if let Some(other) = state.get_const(&name.1) {
                return report::const_already_defined(name, &other.name)
            }

            if let Some(other) = state.get_any_object(&name.1) {
                return report::object_name_collision(name, other)
            }
            */

            /*
            let kind_value = primitive::to_kind_value(kind, default_value);

            return Ok(FrozenUnit::Constant {
                docstring: None,
                name: name.clone(), kind_value
            })
            */
        }
        Enum { name, variants, .. } => {
            let mut frozen_variants: Vec<FrozenUnit> = vec![];

            for variant in variants {
                pub(crate) fn to_variant(variant: &ASTUnit) -> KindValue {
                    match variant {
                        EnumVariant { name, kind } => {
                            if kind.is_none() {
                                return KindValue::EnumVariant(
                                    name.1.clone(),None
                                )
                            }

                            KindValue::EnumVariant(
                                name.1.clone(), None
                            )
                        },
                        _ => panic!("Should not be here")
                    }
                }

                frozen_variants.push(FrozenUnit::EnumVariant(
                    to_variant(&variant.1)
                ));
            }

            return Ok(FrozenUnit::Enum {
                docstring: None,
                name: name.1.clone(),
                variants: frozen_variants
            })
        }
        /*
        EnumVariant { .. } => {}
        Settings { .. } => {}
        Struct { .. } => {}
        Protocol { .. } => {}
        Function { .. } => {}
        Error { .. } => {}
        */
        #[allow(unused)]
        Validator {
            docstring, properties,
            name, expression_block
        } => {
            let properties = to_properties(properties);
            let expr_block = Box::new(FrozenUnit::ExpressionBlock { function_calls: vec![] });


            return Ok(FrozenUnit::Validator {
                docstring: Some("".to_owned()), // TODO: Docstrings should likely be a vector of units
                properties: vec![],
                name: name.1.clone(),
                expression_block: expr_block
            })
        }
        /*
        Field { .. } => {}
        Parameter { .. } => {}
        Property { .. } => {}
        ExpressionBlock { .. } => {}
        */
        missing => panic!("Missing implementation for node '{:?}'", missing)
    }

    panic!()
}

#[allow(unused)]
fn to_properties(nodes: &Vec<SpannedUnit>) -> Vec<FrozenUnit> {
    let properties = vec![];

    for node in nodes {

    }

    properties
}


fn to_function_frozen(
    schema_ctx: &Ref<'_, SchemaContext>, project_context: &'_ ProjectContext,
    spanned_unit: &SpannedUnit
) -> Result<FrozenUnit, Box<dyn snafu::Error>> {
    #[allow(unused)]
    let ASTUnit::Function {
        docstring, parameters, name,
        asynchronous,
        arguments, _return, throws
    } = &spanned_unit.1 else { panic!() };
    let sync = || { !asynchronous.is_some() };

    let mut args = vec![];

    for (_, unit) in arguments {
        let ASTUnit::Argument { name, kind } = unit else { panic!() };

        let Some(kind) = kind_search::to_kind_only(
            schema_ctx, project_context, kind
        ) else { panic!() };

        args.push(FrozenArgument {
            name: name.1.clone(),
            kind
        });
    }

    let mut frozen_return = None;

    if let Some(_return) = _return {
        if let Some(ret) = kind_search::to_kind_only(
            schema_ctx, project_context, _return
        ) { frozen_return = Some(ret); } else { panic!() };
    }

    Ok(FrozenUnit::Function {
        docstring: "".to_string(),
        name: name.1.clone(),
        synchronous: sync(),
        // direction: Box::new(()),
        arguments: args,
        _return: frozen_return,
        throws: vec![],
    })
}


pub fn into_frozen_unit() -> FrozenUnit {
    todo!()
}
