//! proptest strategies for a [`Spec`]: one to three packages in a dependency
//! chain or DAG, one or two schemas each, a few structs, up to three protocols
//! in a package.
//!
//! Alternatives are listed simplest first, which is where proptest shrinks to
//! (`Nothing`, bounded, default handshake, no change), so a shrunk graph has
//! only what the failure needs. The weights keep most graphs near the
//! interesting edge: about half build, so both sides of the oracle's decisions
//! (and the frozen sets of the ones that do) are exercised.

use proptest::collection::vec;
use proptest::prelude::*;

use crate::model::Handshake;
use crate::spec::*;

fn handshake() -> impl Strategy<Value = Handshake> {
    prop_oneof![8 => Just(Handshake::Default), 2 => Just(Handshake::None), 1 => Just(Handshake::Comline1)]
}

/// A protocol's marker: mostly what an author who got it right writes, and
/// the rest what a careless one does.
fn marker() -> impl Strategy<Value = AckSpec> {
    prop_oneof![
        1 => Just(AckSpec::Nothing),
        3 => Just(AckSpec::Plain),
        2 => Just(AckSpec::Needed),
        10 => Just(AckSpec::Auto),
        // Any keys, a misspelt one among them.
        1 => (0u8..64).prop_map(AckSpec::Keys),
    ]
}

/// The package's `unverified`: mostly absent.
fn package_ack() -> impl Strategy<Value = AckSpec> {
    prop_oneof![
        8 => Just(AckSpec::Nothing),
        1 => Just(AckSpec::Plain),
        1 => Just(AckSpec::Needed),
        4 => Just(AckSpec::Auto),
        1 => (0u8..64).prop_map(AckSpec::Keys),
    ]
}

/// A field or an argument: mostly bounded, mostly a plain struct where one
/// can be named, a list of it (which closes cycles) less often.
fn ty() -> impl Strategy<Value = TySpec> {
    let kind = prop_oneof![
        6 => Just(TyKind::Str { bounded: true }),
        5 => Just(TyKind::Bytes { bounded: true }),
        8 => Just(TyKind::Struct { array: false }),
        2 => Just(TyKind::Struct { array: true }),
        1 => Just(TyKind::Str { bounded: false }),
        1 => Just(TyKind::Bytes { bounded: false }),
    ];
    (kind, any::<u8>()).prop_map(|(kind, pick)| TySpec { kind, pick })
}

fn accept() -> impl Strategy<Value = AcceptSpec> {
    prop_oneof![
        10 => Just(AcceptSpec::Exact),
        1 => Just(AcceptSpec::Omit),
        1 => (0u8..32).prop_map(AcceptSpec::Set),
        1 => (1u8..32).prop_map(AcceptSpec::Plus),
    ]
}

fn dep() -> impl Strategy<Value = DepSpec> {
    (any::<u8>(), vec(accept(), 0..6), prop_oneof![5 => Just(false), 1 => Just(true)])
        .prop_map(|(on, accepts, reversed)| DepSpec { on, accepts, reversed })
}

fn structure() -> impl Strategy<Value = StructSpec> {
    (prop_oneof![1 => Just(false), 2 => Just(true)], vec(ty(), 1..4)).prop_map(|(max_depth, fields)| StructSpec { max_depth, fields })
}

fn protocol() -> impl Strategy<Value = ProtocolSpec> {
    (handshake(), marker(), vec(ty(), 1..3)).prop_map(|(handshake, marker, args)| ProtocolSpec { handshake, marker, args })
}

fn schema() -> impl Strategy<Value = SchemaSpec> {
    (vec(structure(), 1..4), vec(protocol(), 1..3)).prop_map(|(structs, protocols)| SchemaSpec { structs, protocols })
}

fn package() -> impl Strategy<Value = PackageSpec> {
    (
        prop_oneof![19 => Just(true), 1 => Just(false)],
        handshake(),
        package_ack(),
        vec(schema(), 1..3),
        vec(dep(), 1..3),
    )
        .prop_map(|(gate, handshake, unverified, schemas, deps)| PackageSpec { gate, handshake, unverified, schemas, deps })
}

fn change() -> impl Strategy<Value = Option<Change>> {
    let (p, s, t) = (any::<u8>(), any::<u8>(), any::<u8>());
    prop_oneof![
        3 => Just(None::<Change>),
        // Mostly what the issue names: a field that holds no maximum, and a
        // type swapped.
        3 => (p, s, t, ty()).prop_map(|(pkg, schema, strukt, ty)| Some(Change::AddField { pkg, schema, strukt, ty })),
        3 => (p, s, t, any::<u8>(), ty()).prop_map(|(pkg, schema, strukt, field, ty)| Some(Change::SwapField { pkg, schema, strukt, field, ty })),
        1 => (p, handshake()).prop_map(|(pkg, handshake)| Some(Change::SetHandshake { pkg, handshake })),
        1 => (p, s, t, handshake()).prop_map(|(pkg, schema, protocol, handshake)| Some(Change::SetProtocolHandshake { pkg, schema, protocol, handshake })),
    ]
}

/// Any graph.
pub fn spec() -> impl Strategy<Value = Spec> {
    (vec(package(), 1..4), change()).prop_map(|(packages, change)| Spec { packages, change })
}
