// Relative Modules
pub mod grammar;  // Rust-sitter generated parser
pub mod diagnostics;  // Beautiful error reporting
pub mod size;  // Static wire-size estimate (hover)
pub mod vocabulary;  // Primitive types & keywords, for editors/highlighters/docs



pub mod constants {
    pub const SCHEMA_EXTENSION: &str = "ids";
    // pub const UNIT_EXTENSION: &str = "idu";
}
