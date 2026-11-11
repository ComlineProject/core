//! Arbitrary bytes as a package graph (`core/oracle`: packages, schemas,
//! types, protocols with handshakes, markers and acceptances, and an
//! upstream change), built with `compile_package`, and checked against the
//! oracle (core#223): a build has to succeed exactly when the oracle, working
//! from the rules alone, says every guarantee given up anywhere a protocol
//! reaches was acknowledged and accepted, and then each protocol's frozen
//! given-up set has to be the oracle's. The same check as the property test
//! in `tests/package/unverified_oracle.rs`, from what the fuzzer finds.
//!
//! A finding is a graph (printed as its files) core and the oracle disagree
//! about, or a panic building one. Whichever of the two is wrong by the rules
//! is fixed; a graph core is wrong about goes to `tests/package/` as a
//! minimized test.

#![no_main]

#[path = "../../oracle/harness.rs"]
mod harness;

use comline_core_oracle::check;
use comline_core_oracle::spec::Spec;
use libfuzzer_sys::arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(spec) = Spec::arbitrary(&mut Unstructured::new(data)) else { return };
    if let Err(disagreement) = check::run(&spec, harness::build) {
        panic!("{disagreement}\n\nfrom {spec:?}");
    }
});
