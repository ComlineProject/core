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

use comline_core_oracle::check;
use comline_core_oracle::gen;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(),
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn a_build_succeeds_exactly_when_the_oracle_says(spec in gen::spec()) {
        check::run(&spec, harness::build).map_err(TestCaseError::fail)?;
    }
}
