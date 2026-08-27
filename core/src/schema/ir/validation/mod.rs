pub mod symbols;
pub mod validator;

use crate::schema::ir::frozen::unit::FrozenUnit;
// use crate::schema::ir::compiler::report::CompileError;

#[derive(Debug, PartialEq, Clone)]
pub struct ValidationError {
    pub message: String,
    pub context: String, // e.g. "Struct 'User'"
    /// Byte range of the declaration/type-usage this error is about, taken
    /// straight from the offending `FrozenUnit`'s own `span` field.
    pub span: Option<(usize, usize)>,
}

/// Validate a set of declarations (FrozenUnits)
pub fn validate(units: &[FrozenUnit]) -> Result<(), Vec<ValidationError>> {
    validator::validate(units)
}
