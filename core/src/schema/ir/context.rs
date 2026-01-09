// Standard Uses
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

// Crate Uses
use crate::package::config::idl::grammar::Congregation;
// use crate::package::config::ir::frozen::FrozenUnit;
// use crate::schema::idl::ast::unit::{ASTUnit as SchemaASTUnit, Details};
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::interpreter::semi_frozen;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::utils::codemap::{Span, CodeMap};

// External Uses


#[derive(Debug, Clone, Default)]
pub struct CompileState {
    pub complete: bool,
    pub namespace: Option<String>,
    // pub imports: HashMap<Rc<SpannedUnit>, semi_frozen::Import>,
    // pub consts: HashMap<Rc<SpannedUnit>, semi_frozen::Constant>,
    // pub structures: HashMap<Rc<SpannedUnit>, semi_frozen::Structure>,
    // pub protocols: HashMap<Rc<SpannedUnit>, semi_frozen::Protocol>,
}

impl CompileState {
    pub(crate) fn to_frozen(&self) -> Vec<FrozenUnit> {
        let interpreted = vec![
            FrozenUnit::Namespace(self.namespace.clone().unwrap_or_default())
        ];

        interpreted
    }

    pub(crate) fn get_any_object(&self, name: &str) -> Option<&(Span, String)> {
        todo!()
    }

    pub(crate) fn get_const(&self, name: &str) -> Option<semi_frozen::Constant> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct SchemaContext {
    //pub name: String,
    pub namespace: Vec<String>,
    // stored raw declarations from rust-sitter
    pub declarations: Vec<Declaration>,
    // mutable frozen schema storage
    pub frozen_schema: RefCell<Option<Vec<FrozenUnit>>>,
    // source map for reporting
    pub codemap: CodeMap,
    // pub project_context: Option<&'a RefCell<ProjectContext<'a>>>,
    pub compile_state: RefCell<CompileState>
}

impl SchemaContext {
    pub fn with_declarations(declarations: Vec<Declaration>, namespace: Vec<String>, codemap: CodeMap) -> Self {
        Self { namespace, declarations, frozen_schema: RefCell::new(None), codemap, compile_state: Default::default() }
    }

    pub fn namespace_snake(&self) -> String { self.namespace.join("_") }
    pub fn namespace_joined(&self) -> String { self.namespace.join("::") }
    pub fn namespace_as_path(&self) -> PathBuf { PathBuf::from(&self.namespace.join("/")) }


    pub(crate) fn sanitize_units(self) {
        todo!()
    }
}

