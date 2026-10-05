pub mod symbols;
pub mod validator;

use crate::diagnostics::Diagnostic;
use crate::schema::ir::frozen::unit::FrozenUnit;

/// Validate a set of declarations (FrozenUnits)
pub fn validate(units: &[FrozenUnit]) -> Result<(), Vec<Diagnostic>> {
    validator::validate(units)
}
