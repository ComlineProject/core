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
//! (next to this file), where proptest keeps a failure's seed to replay first,
//! is committed when there is one; the chains the issue names are written out
//! as fixed graphs below, and a graph core is wrong about is kept there as an
//! `#[ignore]`d test with the rule it breaks.
//!
//! The nightly fuzz run drives the same check from arbitrary bytes
//! (`fuzz/fuzz_targets/unverified_oracle.rs`).

#[path = "../../oracle/harness.rs"]
mod harness;

use std::collections::BTreeMap;
use std::sync::Mutex;

use comline_core_oracle::check::{self, Stats};
use comline_core_oracle::gen;
use comline_core_oracle::model::*;
use comline_core_oracle::oracle::predict;
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, TestRunner};

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)
}

#[test]
fn a_build_succeeds_exactly_when_the_oracle_says() {
    let cases = cases();
    let config = ProptestConfig {
        cases,
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

    // Not vacuous: graphs the oracle sends both ways, across packages, with
    // each kind of reason to refuse and each guarantee frozen, and changes
    // that flip a build. Only at a case count where that is a certainty.
    let stats = stats.into_inner().unwrap();
    if cases >= 200 {
        let enough = |n: u32, of: u32| n * of >= stats.graphs;
        assert!(enough(stats.built, 10), "fewer than 1 in 10 graphs built: {stats:#?}");
        assert!(enough(stats.refused, 10), "fewer than 1 in 10 graphs were refused: {stats:#?}");
        assert!(stats.built_with_dependencies >= 5, "{stats:#?}");
        assert!(stats.dependency_protocols_compared >= 5, "{stats:#?}");
        assert!(stats.change_flipped >= 1, "{stats:#?}");
        for kind in ["gate", "package-unverified", "bounds", "marker", "acceptance"] {
            assert!(stats.refused_for.get(kind).copied().unwrap_or(0) >= 1, "nothing refused for {kind}: {stats:#?}");
        }
        for guarantee in ["schema_agreement", "encoding_agreement", "bounded_size"] {
            assert!(stats.frozen.get(guarantee).copied().unwrap_or(0) >= 1, "nothing froze {guarantee}: {stats:#?}");
        }
    }
}

// ---- Fixed graphs ----
//
// The chains the issue names, written out as graphs (`chain/*` and
// `acceptance/*` in the specification's cases are their small forms). Each is
// built and has to be what the oracle says, and what the oracle says is
// asserted here too, so a change to the oracle that makes it agree with a
// wrong core, or the reverse, is seen.

const BLOB: TypeId = TypeId { pkg: 0, schema: 0, idx: 0 };

fn package(gate: bool, deps: Vec<Dep>, structs: Vec<Struct>, protocols: Vec<Protocol>) -> Package {
    Package { gate, handshake: Handshake::Default, unverified: None, schemas: vec![Schema { structs, protocols }], deps }
}

fn dep(on: usize, accepts: &[(&str, &[G])]) -> Dep {
    Dep { on, accepts: accepts.iter().map(|(name, set)| (name.to_string(), set.to_vec())).collect::<BTreeMap<_, _>>() }
}

fn protocol(marker: Option<Ack>, args: Vec<Ty>) -> Protocol {
    Protocol { handshake: Handshake::Default, marker, args }
}

fn holding(of: TypeId) -> Struct {
    Struct { max_depth: false, fields: vec![Ty::Struct { of, array: false }] }
}

/// `p0` holds a string without a maximum in `Blob`, its `Upload` marked; `p1`
/// wraps `Blob` and marks `Wrap`; `p2` sends `Wrapper`. `p1` accepts `p0`'s,
/// and `p2` what is given as `accepted`; `p2`'s protocol is marked or not.
fn chain(accepted: &[(&str, &[G])], p2_marker: Option<Ack>) -> Model {
    let bs: &[G] = &[G::BoundedSize];
    let wrapper = TypeId { pkg: 1, schema: 0, idx: 0 };
    Model {
        packages: vec![
            package(
                true,
                vec![],
                vec![Struct { max_depth: false, fields: vec![Ty::Str { bounded: false }] }],
                vec![protocol(Some(Ack::Plain), vec![Ty::Struct { of: BLOB, array: false }])],
            ),
            package(
                true,
                vec![dep(0, &[("P0S0Q0", bs)])],
                vec![holding(BLOB)],
                vec![protocol(Some(Ack::Plain), vec![Ty::Struct { of: wrapper, array: false }])],
            ),
            package(
                true,
                vec![dep(1, accepted)],
                vec![],
                vec![protocol(p2_marker, vec![Ty::Struct { of: wrapper, array: false }])],
            ),
        ],
    }
}

fn expect(m: &Model, builds: bool) {
    assert_eq!(predict(m, m.root()).is_builds(), builds, "the oracle on:\n{}", m.describe());
    check::run_model(m, "the fixed graph", &mut harness::build).unwrap_or_else(|why| panic!("{why}"));
}

const BS: &[G] = &[G::BoundedSize];

#[test]
fn a_chain_builds_when_every_link_is_accepted_and_the_last_says_why() {
    expect(&chain(&[("P1S0Q0", BS), ("P0S0Q0", BS)], Some(Ack::Plain)), true);
}

#[test]
fn accepting_the_middle_is_not_enough_the_first_link_is_accepted_too() {
    expect(&chain(&[("P1S0Q0", BS)], Some(Ack::Plain)), false);
}

#[test]
fn a_consumer_that_reaches_an_unbounded_type_has_to_say_why() {
    expect(&chain(&[("P1S0Q0", BS), ("P0S0Q0", BS)], None), false);
}

#[test]
fn an_acceptance_for_less_or_more_than_is_given_up_stops() {
    let both: &[G] = &[G::BoundedSize, G::SchemaAgreement];
    expect(&chain(&[("P1S0Q0", BS), ("P0S0Q0", &[])], Some(Ack::Plain)), false);
    expect(&chain(&[("P1S0Q0", BS), ("P0S0Q0", both)], Some(Ack::Plain)), false);
}

#[test]
fn an_upstream_field_swapped_for_a_bounded_one_leaves_the_acceptances_unread() {
    let mut m = chain(&[("P1S0Q0", BS), ("P0S0Q0", BS)], Some(Ack::Plain));
    m.packages[0].schemas[0].structs[0].fields[0] = Ty::Str { bounded: true };
    // Nothing is given up anywhere now: the acceptances name protocols that
    // give up nothing, which no one reads.
    expect(&m, true);
}

#[test]
fn an_upstream_field_made_unbounded_is_not_accepted_by_a_consumer_that_accepted_nothing() {
    let mut m = chain(&[], Some(Ack::Plain));
    for p in m.packages.iter_mut() {
        p.deps.iter_mut().for_each(|d| d.accepts.clear());
    }
    m.packages[0].schemas[0].structs[0].fields[0] = Ty::Str { bounded: true };
    expect(&m, true);
    m.packages[0].schemas[0].structs[0].fields[0] = Ty::Str { bounded: false };
    expect(&m, false);
}

/// Regression test for core#245: a dependency's `use s0::A`, naming a
/// sibling schema of its own package, resolves within that package, not
/// against the consumer's schema of the same name. Two packages that each have
/// an `s0` and import across them used to be reported as an import cycle
/// (`p0::s1 -> s0 -> p0::s1`). The graphs here share schema names across
/// packages, so random ones cover this too.
#[test]
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
