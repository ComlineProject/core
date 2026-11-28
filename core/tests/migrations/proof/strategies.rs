// Proptest strategies for the proof harness: random sources and histories
// (what the cores of the test-only formats build from), and random values of
// the formats' own types, which no build produces and which totality and
// the witness have to hold for all the same (proof.md, "Laws").

use comline_core::package::build::cas::VersionBump;
use comline_ir_formats::v3::types::UnknownRecord;
use proptest::prelude::*;

use super::formats::*;
use super::histories::{Event, History};

pub fn kind() -> impl Strategy<Value = SourceKind> {
    prop_oneof![
        Just(SourceKind::Int),
        Just(SourceKind::Float32),
        Just(SourceKind::Float64),
        Just(SourceKind::Text),
    ]
}

fn scalar() -> impl Strategy<Value = SourceScalar> {
    (
        "[a-zA-Z ]{1,8}",
        any::<u32>(),
        kind(),
        prop::collection::vec("[a-z]{1,4}", 0..4),
    )
        .prop_map(|(name, size, kind, tags)| SourceScalar {
            name,
            size,
            kind,
            tags,
        })
}

pub fn source() -> impl Strategy<Value = Source> {
    (
        "[a-zA-Z ]{1,8}",
        "[a-z]{1,6}",
        prop::collection::vec(scalar(), 0..5),
    )
        .prop_map(|(title, owner, scalars)| Source {
            title,
            owner,
            scalars,
        })
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        3 => source().prop_map(Event::Build),
        1 => Just(Event::Rebuild),
        1 => Just(Event::Release { bump: VersionBump::Minor, reason: None }),
        1 => Just(Event::Release {
            bump: VersionBump::Patch,
            reason: Some("raised".to_string()),
        }),
    ]
}

/// A script: a first build, then builds, rebuilds and releases, and
/// sometimes two heads.
pub fn history() -> impl Strategy<Value = History> {
    (
        source(),
        prop::collection::vec(event(), 0..5),
        prop::option::of(source()),
    )
        .prop_map(|(first, rest, fork)| {
            let mut events = vec![Event::Build(first)];
            events.extend(rest);
            History {
                name: "random",
                events,
                fork,
            }
        })
}

fn kind0() -> impl Strategy<Value = Kind0> {
    prop_oneof![Just(Kind0::Int), Just(Kind0::Float), Just(Kind0::Text)]
}

/// Any value format 80's types can express, not only what a build writes.
pub fn p0() -> impl Strategy<Value = P0> {
    (
        any::<String>(),
        prop::collection::vec(
            (
                any::<String>(),
                any::<u32>(),
                kind0(),
                prop::collection::vec(any::<String>(), 0..4),
            )
                .prop_map(|(name, size, kind, tags)| S0 {
                    name,
                    size,
                    kind,
                    tags,
                }),
            0..5,
        ),
    )
        .prop_map(|(title, scalars)| P0 { title, scalars })
}

fn base1() -> impl Strategy<Value = Base1> {
    prop_oneof![Just(Base1::Int), Just(Base1::Float), Just(Base1::Text)]
}

fn record() -> impl Strategy<Value = UnknownRecord> {
    (
        any::<String>(),
        any::<String>(),
        any::<String>(),
        prop::option::of(prop::collection::vec(any::<u8>(), 0..8)),
    )
        .prop_map(|(path, step, reason, original)| UnknownRecord {
            path,
            step,
            reason,
            original,
        })
}

/// Any value format 81's types can express: a width on anything, unknown
/// entries at any path.
pub fn p1() -> impl Strategy<Value = P1> {
    (
        any::<String>(),
        prop::collection::vec(
            (
                any::<String>(),
                any::<u64>(),
                base1(),
                prop::option::of(any::<u8>()),
                prop::collection::vec(any::<String>(), 0..4),
            )
                .prop_map(|(name, size, base, bits, tags)| S1 {
                    name,
                    size,
                    kind: Kind1 { base, bits },
                    tags,
                }),
            0..5,
        ),
        prop::collection::vec(record(), 0..4),
    )
        .prop_map(|(title, scalars, unknown)| P1 {
            title,
            scalars,
            unknown,
        })
}
