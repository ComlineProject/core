//! Any text as a `.ids` schema: parsing never panics or overflows the
//! stack, and a schema it refuses comes back as diagnostics with spans.

#![no_main]

use comline_core::schema::idl::parse::parse_schema;
use comline_core_fuzz::check_spans;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    if let Err(diagnostics) = parse_schema(source) {
        check_spans(source, &diagnostics);
    }
});
