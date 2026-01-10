// Relative Modules
pub mod interpret;
pub mod report;

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::idl::grammar::Congregation;

pub trait Compile {
    type Output;

    /// Compile from the parsed AST (Congregation)
    fn from_congregation(congregation: Congregation) -> Self::Output;

    /// Compile from a raw configuration string
    fn from_source(source: &str) -> Self::Output {
        match crate::package::config::idl::grammar::parse(source) {
            Ok(congregation) => Self::from_congregation(congregation),
            Err(e) => panic!("Parse error: {:?}", e), // TODO: Better error handling
        }
    }

    /// Compile from a file path
    fn from_origin(origin: &Path) -> Self::Output;
}

