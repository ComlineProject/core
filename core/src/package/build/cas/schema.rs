// Standard Uses

// Crate Uses
use crate::schema::ir::frozen::unit::FrozenUnit;


// External Uses
use eyre::Result;


#[allow(unused)]
pub(crate) fn check_difference(
    previous: &[FrozenUnit], latest: &[FrozenUnit]
) -> Result<Vec<FrozenUnit>>{
    todo!()
}

