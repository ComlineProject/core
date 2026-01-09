pub mod symbols;
pub mod validator;

use crate::schema::ir::frozen::unit::FrozenUnit;
// use crate::schema::ir::compiler::report::CompileError;

#[derive(Debug, PartialEq, Clone)]
pub struct ValidationError {
    // For now we don't have spans in FrozenUnit, so we just use a message
    pub message: String,
    pub context: String, // e.g. "Struct 'User'"
}

/// Validate a set of declarations (FrozenUnits)
pub fn validate(units: &[FrozenUnit]) -> Result<(), Vec<ValidationError>> {
    validator::validate(units)
}
