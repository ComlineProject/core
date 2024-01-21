// Relative Modules
pub mod interpret;
pub mod report;

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::idl::ast::{ASTUnit, SourcedWhole};

// External Uses


pub trait Compile {
    type Output;

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output;

    fn from_sourced_whole(sourced: SourcedWhole) -> Self::Output;

    fn from_source(source: &str) -> Self::Output;

    fn from_origin(origin: &Path) -> Self::Output;
}

