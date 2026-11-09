//! Any text as a `config.idp`: parsing never panics, a config it refuses
//! comes back as a diagnostic with a span, and one that parses is
//! interpreted (its keys checked and frozen) without panicking.

#![no_main]

use comline_core::diagnostics;
use comline_core::package::config::idl::grammar;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::package::config::ir::interpreter::interpret::interpret_context;
use comline_core_fuzz::check_spans;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    match grammar::parse(source) {
        // What `ProjectInterpreter::from_config_source` reports.
        Err(errors) => {
            let diagnostic = diagnostics::find_unclosed_bracket(source)
                .unwrap_or_else(|| diagnostics::from_parse_errors(&errors));
            check_spans(source, &[diagnostic]);
        }
        Ok(congregation) => {
            let _ = interpret_context(&ProjectContext::with_config(congregation), None);
        }
    }
});
