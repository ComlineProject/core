// Relative Modules
#[cfg(feature = "compiler")]
pub mod grammar;  // Rust-sitter generated parser
#[cfg(feature = "compiler")]
pub mod size;  // Static wire-size estimate (hover)
#[cfg(feature = "compiler")]
pub mod vocabulary;  // Primitive types & keywords, for editors/highlighters/docs
pub mod annotations;  // Known @key=value metadata, for editors/hover
#[cfg(feature = "compiler")]
pub mod module_docs;  // `//!` module/package docstrings, for editors and docs



pub mod constants {
    pub const SCHEMA_EXTENSION: &str = "ids";
    // pub const UNIT_EXTENSION: &str = "idu";
}
