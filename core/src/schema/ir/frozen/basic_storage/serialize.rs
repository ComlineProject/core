// Standard Uses

// Crate Uses
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses
use rmp_serde::Serializer;


pub(crate) fn to_processed(units: &[FrozenUnit]) -> Vec<u8> {
    let mut processed = vec![];

    serde::Serialize::serialize(&units, &mut Serializer::new(&mut processed))
        .unwrap();

    processed
}
