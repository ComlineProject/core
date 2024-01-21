// Standard Uses
use std::rc::Rc;
use std::cell::RefCell;

// Crate Uses
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::compiler::interpreter::semi_frozen;
use crate::package::config::ir::context::ProjectContext;

// External Uses


pub fn compile_schema_metadata(
    schema_context: Rc<RefCell<SchemaContext>>,
    project_context: &ProjectContext
) -> Result<(), Box<dyn snafu::Error>> {
    let schema_ctx = schema_context.borrow();

    let namespace = schema_ctx.namespace_joined();
    {
        let Some(_) = project_context.find_schema_by_import(&namespace) else {
            // TODO: Fix missing information on error, and also we have no span since its
            //       info from namespace(file name and location on source tree)
            /*
            Err(CompileError::NamespaceCollision {
                origin: "TODO ORIGIN".to_string(), target: "TODO TARGET".to_string()
            }).context(report::CompileSnafu {
                details: ReportDetails::fetch(&schema_ctx, span).unwrap()
            })?
            */
            todo!()
        };

        let mut compile_state = schema_ctx.compile_state.borrow_mut();
        if let Some(name) = &compile_state.namespace {
            panic!("Namespace '{}' was already previously set on schema context", name)
        } else {
            compile_state.namespace = Some(namespace)
        }
    }

    for i in 0..schema_ctx.schema.1.len() {
        let spanned_unit = &schema_ctx.schema.1[i];

        use crate::schema::idl::ast::unit::ASTUnit::*;
        match &spanned_unit.1 {
            // TODO: ASTs are not supposed to have a namespace unit, because its automatically
            //       decided based on source folder structure, remove this later
            Namespace { .. } => { todo!() },
            Docstring {..} => {
                todo!()
            }
            Import(span, import) => {
                let spanned = Rc::clone(spanned_unit);

                schema_ctx.compile_state.borrow_mut().imports.entry(spanned).or_insert(
                    semi_frozen::Import {
                        namespace: (*span, import.clone()),
                        // alias: alias.clone
                    }
                );
            }
            // Docstring { .. } => {}
            #[allow(unused)]
            Constant { name, ..} => {
                let spanned_unit = Rc::clone(spanned_unit);

                schema_ctx.compile_state.borrow_mut().consts
                    .entry(spanned_unit).or_insert(
                    semi_frozen::Constant { name: name.clone() }
                );
            }
            Struct { name, .. } => {
                let spanned_unit = Rc::clone(spanned_unit);

                schema_ctx.compile_state.borrow_mut().structures
                    .entry(spanned_unit).or_insert(
                    semi_frozen::Structure { name: name.clone() }
                );
            }
            Property { .. } => {}
            Parameter { .. } => {}
            ExpressionBlock { .. } => {}
            Enum { .. } => {}
            EnumVariant { .. } => {}
            Settings { .. } => {}
            Protocol { name, .. } => {
                let spanned_unit = Rc::clone(spanned_unit);

                schema_ctx.compile_state.borrow_mut().protocols
                    .entry(spanned_unit).or_insert(
                    semi_frozen::Protocol { name: name.clone() }
                );
            }
            Function { .. } => {}
            Argument { .. } => {}
            Error { .. } => {}
            Validator { .. } => {}
            Field { .. } => {}
        }
    }

    Ok(())
}