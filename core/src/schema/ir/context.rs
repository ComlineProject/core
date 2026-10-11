// Standard Uses
use std::cell::RefCell;
use std::path::PathBuf;

// Crate Uses
// use crate::package::config::idl::grammar::Congregation;
// use crate::package::config::ir::frozen::FrozenUnit;
// use crate::schema::idl::ast::unit::{ASTUnit as SchemaASTUnit, Details};
use crate::diagnostics::Diagnostic;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::utils::codemap::CodeMap;

// External Uses


#[derive(Debug, Clone, Default)]
pub struct CompileState {
    pub complete: bool,
    pub namespace: Option<String>,
}

#[allow(unused)]
impl CompileState {
    pub(crate) fn to_frozen(&self) -> Vec<FrozenUnit> {
        let interpreted = vec![
            FrozenUnit::Namespace(self.namespace.clone().unwrap_or_default())
        ];

        interpreted
    }
}

#[derive(Debug, Clone)]
pub struct SchemaContext {
    //pub name: String,
    pub namespace: Vec<String>,
    // stored raw declarations from rust-sitter
    pub declarations: Vec<rust_sitter::Spanned<Declaration>>,
    // mutable frozen schema storage
    pub frozen_schema: RefCell<Option<Vec<FrozenUnit>>>,
    // source map for reporting
    pub codemap: CodeMap,
    // pub project_context: Option<&'a RefCell<ProjectContext<'a>>>,
    pub compile_state: RefCell<CompileState>,
    /// Non-fatal diagnostics accumulated while compiling this schema (e.g. a
    /// reference to a `@deprecated` struct/error) - kept structured, not
    /// pre-rendered, since rendering one needs `codemap` (below), only
    /// available here. Set by
    /// `package::config::ir::compiler::interpret::interpret_context`.
    pub warnings: RefCell<Vec<Diagnostic>>,
}

impl SchemaContext {
    pub fn with_declarations(declarations: Vec<rust_sitter::Spanned<Declaration>>, namespace: Vec<String>, codemap: CodeMap) -> Self {
        Self {
            namespace, declarations, frozen_schema: RefCell::new(None), codemap,
            compile_state: Default::default(), warnings: RefCell::new(Vec::new()),
        }
    }

    pub fn namespace_snake(&self) -> String { self.namespace.join("_") }
    pub fn namespace_joined(&self) -> String { self.namespace.join("::") }
    pub fn namespace_as_path(&self) -> PathBuf { PathBuf::from(&self.namespace.join("/")) }

    /// Render a diagnostic against this schema's own source, falling back
    /// to a flat message when this context has no registered file to
    /// render a snippet against. Moved here (from a private helper in
    /// `package::config::ir::compiler::interpret`) so both the fatal-error
    /// path and non-fatal `warnings` share one implementation.
    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let Some(file) = self.codemap.files().first() else {
            return match diagnostic.context.is_empty() {
                true => diagnostic.message.clone(),
                false => format!("{}\n  {}", diagnostic.message, diagnostic.context),
            };
        };
        crate::diagnostics::render(diagnostic, file.filename(), file.contents())
    }
}

