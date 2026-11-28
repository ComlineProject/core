// The steps between the test-only formats, and the steps that are wrong on
// purpose (core#268, step 1.4; proof.md, "Test-only formats").
//
// `Up01` (80 -> 81) and `Up12` (81 -> 82) are written the way steps.md asks:
// total, deterministic, with a witness (`down`) and the list of paths they
// write as unknown. Each carries a `Defect`, and `Defect::None` is the honest
// step. Every other defect is one mistake and nothing else, so that a test
// can say which check rejects it and that the honest step passes the same
// check: the gate is tested by seeing it reject a bad step.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use comline_core::package::build::cas::migrate::{Registry, Step, UnknownDecl, Version};
use comline_core::package::build::cas::storage::to_cbor;
use comline_ir_formats::v3::types::UnknownRecord;
use eyre::{bail, Result};

use super::formats::*;

/// The one mistake a step is made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    /// The honest step.
    None,
    /// Computes a wrong value for a field N could know (with a `down` that
    /// agrees with it, so that the witness holds: only the square sees it).
    WrongValue,
    /// Writes a value for a field N could not know, where the step declares
    /// it unknown and doesn't write the unknown.
    Invents,
    /// Writes an unknown for a field N could know.
    Needless,
    /// Loses a field: `up` forgets the tags and `down` can't bring them back.
    Drops,
    /// Not total: refuses a value the format can express (a size no real
    /// build has, so a fixture never shows it).
    Partial,
    /// Reads a clock (a counter stands in for one: the same call gives
    /// another answer each time).
    Clock,
    /// Orders by a `HashMap`'s iteration order.
    HashOrder,
}

/// `Defect::Partial` refuses a size above this.
pub const PARTIAL_LIMIT: u32 = 1_000_000;

static CLOCK: AtomicU64 = AtomicU64::new(0);

/// The step 80 -> 81 (`T0` to `T1`).
pub struct Up01(pub Defect);

impl Step for Up01 {
    type From = P0;
    type To = P1;

    fn from_format(&self) -> u32 {
        T0
    }

    /// T0 recorded no width for a float.
    fn unknown(&self) -> Vec<UnknownDecl> {
        vec![UnknownDecl::new(
            "/scalars/*/kind/bits",
            "format 80 recorded no width for a float",
        )]
    }

    fn up(&self, version: Version<P0>) -> Result<Version<P1>> {
        let step = format!("{T0}-{T1}");
        let mut unknown = vec![];
        let mut scalars = Vec::with_capacity(version.payload.scalars.len());
        for (index, scalar) in version.payload.scalars.iter().enumerate() {
            if self.0 == Defect::Partial && scalar.size > PARTIAL_LIMIT {
                bail!("a size of {} is more than this step handles", scalar.size);
            }
            let mut size = u64::from(scalar.size);
            if self.0 == Defect::WrongValue {
                size += 1;
            }
            let base = match scalar.kind {
                Kind0::Int => Base1::Int,
                Kind0::Float => Base1::Float,
                Kind0::Text => Base1::Text,
            };
            let width_path = format!("/scalars/{index}/kind/bits");
            let bits = match (scalar.kind, self.0) {
                // A float's width is not in T0. The honest step keeps a
                // placeholder and says so, with what T0 recorded.
                (Kind0::Float, Defect::Invents) => Some(32),
                (Kind0::Float, _) => {
                    unknown.push(UnknownRecord {
                        path: width_path,
                        step: step.clone(),
                        reason: "format 80 recorded no width for a float".into(),
                        original: Some(to_cbor(&scalar.kind)?),
                    });
                    Some(WIDTH_PLACEHOLDER)
                }
                // What has no width has nothing unknown about it. The path
                // is on the declared list (`/scalars/*/kind/bits`), which
                // covers it, so the engine lets it through.
                (_, Defect::Needless) => {
                    unknown.push(UnknownRecord {
                        path: width_path,
                        step: step.clone(),
                        reason: "format 80 recorded no width for a float".into(),
                        original: Some(to_cbor(&scalar.kind)?),
                    });
                    Some(WIDTH_PLACEHOLDER)
                }
                _ => None,
            };
            scalars.push(S1 {
                name: scalar.name.clone(),
                size,
                kind: Kind1 { base, bits },
                tags: if self.0 == Defect::Drops {
                    vec![]
                } else {
                    scalar.tags.clone()
                },
            });
        }
        let payload = P1 {
            title: version.payload.title.clone(),
            scalars,
            unknown,
        };
        Ok(version.with_payload(payload))
    }

    fn down(&self, version: Version<P1>) -> Result<Version<P0>> {
        let mut scalars = Vec::with_capacity(version.payload.scalars.len());
        for scalar in &version.payload.scalars {
            let mut size = u32::try_from(scalar.size)?;
            if self.0 == Defect::WrongValue {
                size = size.saturating_sub(1);
            }
            scalars.push(S0 {
                name: scalar.name.clone(),
                size,
                kind: match scalar.kind.base {
                    Base1::Int => Kind0::Int,
                    Base1::Float => Kind0::Float,
                    Base1::Text => Kind0::Text,
                },
                tags: scalar.tags.clone(),
            });
        }
        let payload = P0 {
            title: version.payload.title.clone(),
            scalars,
        };
        Ok(version.with_payload(payload))
    }
}

/// The step 81 -> 82 (`T1` to `T2`).
pub struct Up12(pub Defect);

impl Step for Up12 {
    type From = P1;
    type To = P2;

    fn from_format(&self) -> u32 {
        T1
    }

    /// T1 recorded no owner. (The total it can compute.)
    fn unknown(&self) -> Vec<UnknownDecl> {
        vec![UnknownDecl::new("/owner", "format 81 recorded no owner")]
    }

    fn up(&self, version: Version<P1>) -> Result<Version<P2>> {
        let step = format!("{T1}-{T2}");
        let from = &version.payload;
        let total = from
            .scalars
            .iter()
            .fold(0u64, |sum, scalar| sum.saturating_add(scalar.size));
        let total_size = match self.0 {
            Defect::Needless => Known::Unknown {
                step: step.clone(),
                reason: "format 81 total not trusted".into(),
            },
            Defect::WrongValue => Known::Known(total.wrapping_add(1)),
            _ => Known::Known(total),
        };
        let owner = match self.0 {
            Defect::Invents => Known::Known("nobody".to_string()),
            _ => Known::Unknown {
                step: step.clone(),
                reason: "format 81 recorded no owner".into(),
            },
        };
        let scalars = from
            .scalars
            .iter()
            .map(|scalar| {
                let mut slug = slug(&scalar.name);
                match self.0 {
                    Defect::Clock => {
                        let tick = CLOCK.fetch_add(1, Ordering::SeqCst);
                        slug.push_str(&format!("-{tick}"));
                    }
                    Defect::HashOrder => {
                        let set: HashMap<&str, ()> =
                            scalar.tags.iter().map(|tag| (tag.as_str(), ())).collect();
                        let order: Vec<&str> = set.keys().copied().collect();
                        slug.push('-');
                        slug.push_str(&order.join("."));
                    }
                    _ => {}
                }
                S2 {
                    name: scalar.name.clone(),
                    slug,
                    bytes: scalar.size,
                    kind: Kind2 {
                        base: match scalar.kind.base {
                            Base1::Int => Base2::Int,
                            Base1::Float => Base2::Float,
                            Base1::Text => Base2::Text,
                        },
                        bits: scalar.kind.bits,
                    },
                    tags: if self.0 == Defect::Drops {
                        vec![]
                    } else {
                        scalar.tags.clone()
                    },
                }
            })
            .collect();
        let payload = P2 {
            title: from.title.clone(),
            owner,
            total_size,
            scalars,
            unknown: from.unknown.clone(),
        };
        Ok(version.with_payload(payload))
    }

    fn down(&self, version: Version<P2>) -> Result<Version<P1>> {
        let from = &version.payload;
        let payload = P1 {
            title: from.title.clone(),
            scalars: from
                .scalars
                .iter()
                .map(|scalar| S1 {
                    name: scalar.name.clone(),
                    size: scalar.bytes,
                    kind: Kind1 {
                        base: match scalar.kind.base {
                            Base2::Int => Base1::Int,
                            Base2::Float => Base1::Float,
                            Base2::Text => Base1::Text,
                        },
                        bits: scalar.kind.bits,
                    },
                    tags: scalar.tags.clone(),
                })
                .collect(),
            unknown: from.unknown.clone(),
        };
        Ok(version.with_payload(payload))
    }
}

/// The registry of an engine that writes format `target`: the formats up to
/// it and the steps between them, each with its defect.
pub fn registry(target: u32, first: Defect, second: Defect) -> Registry {
    let mut registry = Registry::new(target).with_format(Format0);
    if target >= T1 {
        registry = registry.with_format(Format1).with_step(Up01(first));
    }
    if target >= T2 {
        registry = registry.with_format(Format2).with_step(Up12(second));
    }
    registry
}

/// The honest registry.
pub fn honest(target: u32) -> Registry {
    registry(target, Defect::None, Defect::None)
}
