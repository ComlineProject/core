//! Any text as a `.ids` schema, through the compiler: a schema that parses
//! is checked (aliases, bounds, settings) and compiled to its frozen units
//! without panicking, and a schema the checks refuse comes back as
//! diagnostics with spans. The units it compiles go through the store's CBOR
//! and back unchanged, with an unchanged IR hash (core#172 built a schema it
//! then couldn't read back).

#![no_main]

use comline_core::schema::idl::parse::parse_schema;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::frozen::cas::blob::{
    blob_to_frozen_unit, frozen_unit_to_blob, schema_ir_hash,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core_fuzz::check_spans;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    let Ok(document) = parse_schema(source) else { return };
    match IncrementalInterpreter::check_and_from_declarations(document.0, None) {
        Err(diagnostics) => check_spans(source, &diagnostics),
        Ok(units) => {
            let restored: Vec<FrozenUnit> = units
                .iter()
                .map(|unit| {
                    let blob = frozen_unit_to_blob(unit)
                        .unwrap_or_else(|e| panic!("encode {unit:?}: {e}\nfor {source:?}"));
                    blob_to_frozen_unit(&blob)
                        .unwrap_or_else(|e| panic!("decode {unit:?}: {e}\nfor {source:?}"))
                })
                .collect();
            assert_eq!(restored, units, "a unit changed through CBOR for {source:?}");
            assert_eq!(
                schema_ir_hash(&restored),
                schema_ir_hash(&units),
                "the IR hash changed through CBOR for {source:?}"
            );
        }
    }
});
