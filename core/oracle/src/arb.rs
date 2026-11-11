//! A [`Spec`] from the fuzzer's bytes, of the same shapes as `gen` draws:
//! counts in the same ranges, every pick a number resolved later. An input
//! that runs out reads as zeros, which is the simplest graph.

use arbitrary::{Arbitrary, Result, Unstructured};

use crate::model::Handshake;
use crate::spec::*;

fn below(u: &mut Unstructured, n: u8) -> Result<u8> {
    u.int_in_range(0..=n - 1)
}

fn count(u: &mut Unstructured, from: usize, to: usize) -> Result<usize> {
    u.int_in_range(from..=to)
}

fn handshake(u: &mut Unstructured) -> Result<Handshake> {
    Ok(match below(u, 11)? {
        0..=7 => Handshake::Default,
        8 | 9 => Handshake::None,
        _ => Handshake::Comline1,
    })
}

fn marker(u: &mut Unstructured) -> Result<AckSpec> {
    Ok(match below(u, 17)? {
        0 => AckSpec::Nothing,
        1..=3 => AckSpec::Plain,
        4 | 5 => AckSpec::Needed,
        6..=15 => AckSpec::Auto,
        _ => AckSpec::Keys(below(u, 64)?),
    })
}

fn package_ack(u: &mut Unstructured) -> Result<AckSpec> {
    Ok(match below(u, 15)? {
        0..=7 => AckSpec::Nothing,
        8 => AckSpec::Plain,
        9 => AckSpec::Needed,
        10..=13 => AckSpec::Auto,
        _ => AckSpec::Keys(below(u, 64)?),
    })
}

fn ty(u: &mut Unstructured) -> Result<TySpec> {
    let kind = match below(u, 23)? {
        0..=5 => TyKind::Str { bounded: true },
        6..=10 => TyKind::Bytes { bounded: true },
        11..=18 => TyKind::Struct { array: false },
        19 | 20 => TyKind::Struct { array: true },
        21 => TyKind::Str { bounded: false },
        _ => TyKind::Bytes { bounded: false },
    };
    Ok(TySpec { kind, pick: u.arbitrary()? })
}

fn tys(u: &mut Unstructured, from: usize, to: usize) -> Result<Vec<TySpec>> {
    (0..count(u, from, to)?).map(|_| ty(u)).collect()
}

fn accept(u: &mut Unstructured) -> Result<AcceptSpec> {
    Ok(match below(u, 13)? {
        0..=9 => AcceptSpec::Exact,
        10 => AcceptSpec::Omit,
        11 => AcceptSpec::Set(below(u, 32)?),
        _ => AcceptSpec::Plus(1 + below(u, 31)?),
    })
}

fn dep(u: &mut Unstructured) -> Result<DepSpec> {
    let accepts = (0..count(u, 0, 5)?).map(|_| accept(u)).collect::<Result<_>>()?;
    Ok(DepSpec { on: u.arbitrary()?, accepts, reversed: below(u, 6)? == 0 })
}

fn structure(u: &mut Unstructured) -> Result<StructSpec> {
    Ok(StructSpec { max_depth: below(u, 3)? != 0, fields: tys(u, 1, 3)? })
}

fn protocol(u: &mut Unstructured) -> Result<ProtocolSpec> {
    Ok(ProtocolSpec { handshake: handshake(u)?, marker: marker(u)?, args: tys(u, 1, 2)? })
}

fn schema(u: &mut Unstructured) -> Result<SchemaSpec> {
    let structs = (0..count(u, 1, 3)?).map(|_| structure(u)).collect::<Result<_>>()?;
    let protocols = (0..count(u, 1, 2)?).map(|_| protocol(u)).collect::<Result<_>>()?;
    Ok(SchemaSpec { structs, protocols })
}

fn package(u: &mut Unstructured) -> Result<PackageSpec> {
    Ok(PackageSpec {
        gate: below(u, 20)? != 0,
        handshake: handshake(u)?,
        unverified: package_ack(u)?,
        schemas: (0..count(u, 1, 2)?).map(|_| schema(u)).collect::<Result<_>>()?,
        deps: (0..count(u, 1, 2)?).map(|_| dep(u)).collect::<Result<_>>()?,
    })
}

fn change(u: &mut Unstructured) -> Result<Option<Change>> {
    let (pkg, schema, strukt) = (u.arbitrary()?, u.arbitrary()?, u.arbitrary()?);
    Ok(match below(u, 11)? {
        0..=2 => None,
        3..=5 => Some(Change::AddField { pkg, schema, strukt, ty: ty(u)? }),
        6..=8 => Some(Change::SwapField { pkg, schema, strukt, field: u.arbitrary()?, ty: ty(u)? }),
        9 => Some(Change::SetHandshake { pkg, handshake: handshake(u)? }),
        _ => Some(Change::SetProtocolHandshake { pkg, schema, protocol: strukt, handshake: handshake(u)? }),
    })
}

impl<'a> Arbitrary<'a> for Spec {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let packages = (0..count(u, 1, 3)?).map(|_| package(u)).collect::<Result<_>>()?;
        Ok(Spec { packages, change: change(u)? })
    }
}
