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
    Ok(match below(u, 9)? {
        0..=5 => Handshake::Default,
        6 | 7 => Handshake::None,
        _ => Handshake::Comline1,
    })
}

fn ack(u: &mut Unstructured) -> Result<AckSpec> {
    Ok(match below(u, 12)? {
        0..=2 => AckSpec::Nothing,
        3..=6 => AckSpec::Plain,
        7..=10 => AckSpec::Needed,
        _ => AckSpec::Keys(below(u, 64)?),
    })
}

fn ty(u: &mut Unstructured) -> Result<TySpec> {
    let kind = match below(u, 13)? {
        0..=3 => TyKind::Str { bounded: true },
        4..=6 => TyKind::Bytes { bounded: true },
        7..=10 => TyKind::Struct { array: u.arbitrary()? },
        11 => TyKind::Str { bounded: false },
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
    Ok(StructSpec { max_depth: u.arbitrary()?, fields: tys(u, 1, 3)? })
}

fn protocol(u: &mut Unstructured) -> Result<ProtocolSpec> {
    Ok(ProtocolSpec { handshake: handshake(u)?, marker: ack(u)?, args: tys(u, 0, 2)? })
}

fn schema(u: &mut Unstructured) -> Result<SchemaSpec> {
    let structs = (0..count(u, 1, 3)?).map(|_| structure(u)).collect::<Result<_>>()?;
    let protocols = (0..count(u, 0, 2)?).map(|_| protocol(u)).collect::<Result<_>>()?;
    Ok(SchemaSpec { structs, protocols })
}

fn package(u: &mut Unstructured) -> Result<PackageSpec> {
    Ok(PackageSpec {
        gate: below(u, 6)? != 0,
        handshake: handshake(u)?,
        unverified: ack(u)?,
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
