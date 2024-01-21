// Standard Uses
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;

// Crate Uses
use crate::package::config::idl::ast::{SourcedWhole as ProjectSourcedWhole};
use crate::package::config::ir::frozen::FrozenUnit;
// use crate::schema::idl::ast::unit::{ASTUnit as SchemaASTUnit, Details};
use crate::schema::ir::context::SchemaContext;

// External Uses


#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    Virtual,
    Disk(PathBuf)
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub origin: Origin,
    pub config: ProjectSourcedWhole,
    pub config_frozen: Option<Vec<FrozenUnit>>,
    pub schema_contexts: Vec<Rc<RefCell<SchemaContext>>>,
    pub relative_projects: Vec<ProjectContext>,
}


impl ProjectContext {
    pub fn with_config_from_origin(origin: Origin, config: ProjectSourcedWhole) -> Self {
        Self {
            origin,
            config, config_frozen: None,
            relative_projects: vec![],
            schema_contexts: vec![],
        }
    }

    pub fn with_config(config: ProjectSourcedWhole) -> Self {
        Self {
            origin: Origin::Virtual,
            config, config_frozen: None,
            relative_projects: vec![],
            schema_contexts: vec![],
        }
    }

    pub(crate) fn add_relative_project(mut self, sourced: ProjectSourcedWhole) {
        self.relative_projects.push(
            Self::with_config(sourced)
        )
    }

    pub(crate) fn add_relative_project_context(mut self, context: Rc<ProjectContext>) {
        todo!()
    }

    pub(crate) fn add_schema_context(&mut self, context: Rc<RefCell<SchemaContext>>) {
        self.schema_contexts.push(context);
    }

    pub(crate) fn sanitize_units(self) {
        todo!()
    }

    pub(crate) fn find_schema_by_import(
        &self, import: &str
    ) -> Option<&Rc<RefCell<SchemaContext>>> {
        // TODO: At AST parsing or compilation meta stage a namespace is not present
        //       so the namespace should be checked on schema context (schema_context.namespace)
        /*
        for schema_context in self.schema_contexts.iter() {
            let units = &schema_context.borrow().schema.1;
            if let Some(unit) = units.find_namespace() {
                if let SchemaASTUnit::Namespace(_, namespace) = &unit.1 {
                    if namespace == import {
                        return Some(schema_context)
                    }
                }
            }
        }
        */

        for schema_context in &self.schema_contexts {
            let schema_ctx = schema_context.borrow();
            let target_namespace = schema_ctx.namespace_joined();

            if target_namespace == import {
                return Some(&schema_context)
            }
        }

        None
    }

    // TODO: Might not be necessary a parts finder, depending on how the above fits
    pub(crate) fn find_schema_by_import_namespace_parts(
        &self, import: &str
    ) {
        todo!()
    }

    /*
    pub(crate) fn find_schema_by_import(&self, import: &str)
        -> Option<&Rc<RefCell<SchemaContext<'a>>>>
    {
        for schema_context in self.schema_contexts.iter() {
            let schema_ctx = schema_context.borrow();
            let state = schema_ctx.compile_state.borrow();

            if let Some(state) = &state.namespace {
                return Some(schema_context)
            }
        }

        None
    }
    */

    /*
    pub(crate) fn find_whole_unit_by_import(&self, import: &str) -> Option<&WholeUnit> {
        if self.include_stdlib {
            if let Some(stdlib_unit) = lang_lib::find_unit(import) {
                return Some(stdlib_unit)
            }
        }

        None
    }
    */

    /*
    pub(crate) fn find_schema_context(&self, sub_namespace: &str) -> Option<Rc<SchemaContext>> {
        todo!()
    }
    */

    pub(crate) fn find_relative_project_context(
        &self, import: &str
    ) -> Option<&ProjectContext> {
        todo!()
    }

    pub(crate) fn find_schema_by_filename(&self, filename: &String) -> Option<PathBuf> {
        let Origin::Disk(origin) = &self.origin else {
            panic!("Only disk lookups are supported at the moment")
        };

        let schema_location = origin.with_file_name(filename);

        if schema_location.exists() { return Some(schema_location) }

        None
    }
}

