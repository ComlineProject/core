// Relative Modules
pub mod grammar;  // Rust-sitter generated parser
pub mod size;  // Static wire-size estimate (hover)
pub mod vocabulary;  // Primitive types & keywords, for editors/highlighters/docs
pub mod annotations;  // Known @key=value metadata, for editors/hover
pub mod module_docs;  // `//!` module/package docstrings, for editors and docs



pub mod constants {
    pub const SCHEMA_EXTENSION: &str = "ids";
    // pub const UNIT_EXTENSION: &str = "idu";
}
