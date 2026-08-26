// Standard Uses
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;

// Crate Uses
use crate::package::config::idl::grammar::Congregation;
use crate::package::config::ir::frozen::FrozenUnit;
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
    pub config: Congregation,
    pub config_frozen: Option<Vec<FrozenUnit>>,
    pub schema_contexts: Vec<Rc<RefCell<SchemaContext>>>,
    pub relative_projects: Vec<ProjectContext>,
}


#[allow(unused)]
impl ProjectContext {
    pub fn with_config_from_origin(origin: Origin, config: Congregation) -> Self {
        Self {
            origin,
            config, config_frozen: None,
            relative_projects: vec![],
            schema_contexts: vec![],
        }
    }

    pub fn with_config(config: Congregation) -> Self {
        Self {
            origin: Origin::Virtual,
            config, config_frozen: None,
            relative_projects: vec![],
            schema_contexts: vec![],
        }
    }

    pub(crate) fn add_relative_project(&mut self, sourced: Congregation) {
        self.relative_projects.push(
            Self::with_config(sourced)
        )
    }

    pub(crate) fn add_relative_project_context(&mut self, context: ProjectContext) {
        self.relative_projects.push(context)
    }

    pub(crate) fn add_schema_context(&mut self, context: Rc<RefCell<SchemaContext>>) {
        self.schema_contexts.push(context);
    }
    
    /*
    pub(crate) fn sanitize_units(self) {
        todo!()
    }
    */
    
    pub(crate) fn find_schema_by_import(
        &self, import: &str
    ) -> Option<&Rc<RefCell<SchemaContext>>> {
        for schema_context in &self.schema_contexts {
            let schema_ctx = schema_context.borrow();
            let target_namespace = schema_ctx.namespace_joined();

            if target_namespace == import {
                return Some(&schema_context)
            }
        }

        None
    }

    /// Resolve a `::`-separated namespace path to the schema that declares it,
    /// trying the longest prefix first so that e.g. `["pkg", "types", "User"]`
    /// matches schema `pkg::types` with `["User"]` left over as the symbol name.
    pub(crate) fn find_schema_by_import_namespace_parts(
        &self, parts: &[String]
    ) -> Option<(Rc<RefCell<SchemaContext>>, Vec<String>)> {
        if parts.is_empty() {
            return None;
        }

        for split_at in (1..=parts.len()).rev() {
            let candidate = parts[..split_at].join("::");
            if let Some(schema) = self.find_schema_by_import(&candidate) {
                return Some((Rc::clone(schema), parts[split_at..].to_vec()));
            }
        }

        None
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
        let first_segment = import.split("::").next().unwrap_or(import);

        self.relative_projects.iter()
            .find(|project| project.config.name.value == first_segment)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::codemap::CodeMap;

    fn project_with_schemas(namespaces: &[&[&str]]) -> ProjectContext {
        let congregation = crate::package::config::idl::grammar::parse(
            "congregation test\nspecification_version = 1"
        ).expect("congregation should parse");

        let mut project = ProjectContext::with_config(congregation);

        for namespace in namespaces {
            let namespace: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();
            let schema = SchemaContext::with_declarations(vec![], namespace, CodeMap::new());
            project.schema_contexts.push(Rc::new(RefCell::new(schema)));
        }

        project
    }

    #[test]
    fn find_schema_by_import_namespace_parts_exact_match() {
        let project = project_with_schemas(&[&["pkg", "types"]]);

        let parts = vec!["pkg".to_string(), "types".to_string()];
        let (schema, remaining) = project
            .find_schema_by_import_namespace_parts(&parts)
            .expect("should find schema");

        assert_eq!(schema.borrow().namespace_joined(), "pkg::types");
        assert!(remaining.is_empty());
    }

    #[test]
    fn find_schema_by_import_namespace_parts_prefix_match_with_remaining_symbol() {
        let project = project_with_schemas(&[&["pkg", "types"]]);

        let parts = vec!["pkg".to_string(), "types".to_string(), "User".to_string()];
        let (schema, remaining) = project
            .find_schema_by_import_namespace_parts(&parts)
            .expect("should find schema via prefix match");

        assert_eq!(schema.borrow().namespace_joined(), "pkg::types");
        assert_eq!(remaining, vec!["User".to_string()]);
    }

    #[test]
    fn find_schema_by_import_namespace_parts_no_match() {
        let project = project_with_schemas(&[&["pkg", "types"]]);

        let parts = vec!["other".to_string(), "thing".to_string()];
        assert!(project.find_schema_by_import_namespace_parts(&parts).is_none());
    }
}

