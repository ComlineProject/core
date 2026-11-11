//! Guarantee safety across package graphs (core#223): random graphs of
//! packages, schemas, types, protocols, markers and acceptances, built with
//! `compile_package`, against an oracle that works out from the rules alone
//! what core must do (`core/oracle`, which shares no code with the compiler).
//!
//! The invariant: if a package compiles, every guarantee given up anywhere a
//! protocol reaches, through any chain of types, schemas and packages, has
//! been acknowledged in that package and accepted by every consumer of that
//! protocol. So a build succeeds exactly when the oracle says it must, and
//! when it does, each protocol's frozen given-up set is the oracle's. Each
//! graph is also built again after an upstream change (an added field, a
//! swapped type, a changed handshake) with every consumer's acceptance as it
//! was: the consumer has to fail exactly when the oracle says.
//!
//! The cases are 256 by default; `PROPTEST_CASES` changes it. A failure is
//! shrunk and printed as the packages' files. `unverified_oracle.regressions`
//! (next to this file) keeps what proptest found, replayed first; `MINIMIZED`
//! below are graphs written out as fixed tests.
//!
//! The nightly fuzz run drives the same check from arbitrary bytes
//! (`fuzz/fuzz_targets/unverified_oracle.rs`).

#[path = "../../oracle/harness.rs"]
mod harness;

use std::sync::Mutex;

use comline_core_oracle::check::{self, Stats};
use comline_core_oracle::gen;
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, TestRunner};

fn cases() -> u32 {
    std::env::var("PROBE_CASES").ok().or_else(|| std::env::var("PROPTEST_CASES").ok()).and_then(|n| n.parse().ok()).unwrap_or(2000)
}

#[test]
fn a_build_succeeds_exactly_when_the_oracle_says() {
    let config = ProptestConfig {
        cases: cases(),
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    };
    let stats = Mutex::new(Stats::default());
    let result = TestRunner::new(config).run(&gen::spec(), |spec| {
        let seen = check::run(&spec, harness::build).map_err(TestCaseError::fail)?;
        stats.lock().unwrap().add(&seen);
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    panic!("PROBE {:#?}", stats.lock().unwrap());
}

/// Not a rule of `unverified`, a finding the generator works around: a
/// dependency's `use s0::T`, naming a sibling schema of its own package, is
/// resolved against the consumer's schema `s0` where it has one, so two
/// packages that each have an `s0` and import across them are reported as an
/// import cycle (`p0::s1 -> s0 -> p0::s1`). The graphs here name schemas per
/// package (`s00`, `s10`), which doesn't happen.
#[test]
#[ignore = "core: a dependency's `use` of its own sibling schema resolves to the consumer's schema of the same name (a false import cycle)"]
fn a_dependencys_use_of_its_sibling_schema_is_not_the_consumers() {
    let files = [
        ("p0/config.idp", "congregation p0\nspecification_version = 1\n"),
        ("p0/src/s0.ids", "struct A {\n    f0: char[max 64]\n}\n"),
        ("p0/src/s1.ids", "use s0::A\n\nstruct B {\n    f0: A\n}\n"),
        (
            "p1/config.idp",
            "congregation p1\nspecification_version = 1\ndependencies = {\n    p0 = {\n        path = \"../p0\"\n    }\n}\n",
        ),
        ("p1/src/s0.ids", "use p0::s1::B\n\nprotocol Q {\n    function f(a0: B);\n}\n"),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = tmp.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    comline_core::package::build::compile_package(&tmp.path().join("p1")).expect("p1 builds");
}
