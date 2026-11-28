// The test-only formats (core#268, step 1.4; proof.md, "Test-only formats").
//
// Three formats that exist only here, never written by a release: `T0`
// (format 80), `T1` (81) and `T2` (82), with real frozen types, codecs and
// the objects a store holds, so the engine drives them exactly as it drives
// a real format. They hold a toy package, `Source`: a title, an owner and
// a list of scalars (a name, a size, a kind and tags), and each format
// records a different amount of it:
//
//   T0 (80)  title, scalars with `size: u32` and `kind: Int | Float | Text`.
//            A float's width (32 or 64) is not recorded: it is the thing
//            the old text of real format 2 lost (`f32` and `f64` as `float`).
//   T1 (81)  `size: u64`, and a kind that has a width (`bits`), which a
//            migrated float doesn't know: the new type has no unknown
//            variant, so the step writes a placeholder (`bits: 64`) and an
//            entry in the root `unknown` blob that keeps the original and
//            says the width is not known (the untyped carrier).
//   T2 (82)  `size` renamed `bytes`, a derived `slug`, an `owner` that T1
//            never recorded (`Known<String>`: the typed carrier) and a
//            `total_size` that T1 could compute (`Known<u64>`, known).
//
// The model a step works on is the payload; the paths of an unknown are JSON
// pointers into the payload serialized by serde (`/scalars/2/kind/bits`),
// which is why every payload type derives it.

use comline_core::package::build::cas::migrate::{Format, Objects, Sink};
use comline_core::package::build::cas::objects::{EntryMode, Tree};
use comline_core::package::build::cas::storage::{from_cbor, to_cbor, Hash};
use comline_ir_formats::v3::types::UnknownRecord;
use eyre::{eyre, Result};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value;

pub const T0: u32 = 80;
pub const T1: u32 = 81;
pub const T2: u32 = 82;

/// The width T1 and T2 hold in place of one T0 never recorded. A placeholder
/// the `unknown` entry marks as one: without the entry it would read as a
/// float of 64 bits, which is exactly what a step must never write.
pub const WIDTH_PLACEHOLDER: u8 = 64;

// ---------- the source: what a build reads ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Int,
    Float32,
    Float64,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceScalar {
    pub name: String,
    pub size: u32,
    pub kind: SourceKind,
    pub tags: Vec<String>,
}

/// What a build of any format reads: the package's source, which every
/// format's "core" turns into its own payload (`fresh0`, `fresh1`, `fresh2`).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub title: String,
    pub owner: String,
    pub scalars: Vec<SourceScalar>,
}

// ---------- a value that may be unknown ----------

/// The typed carrier of an unknown (steps.md, "Unknown"): a value, or the
/// statement that the format it was migrated from never recorded it, with the
/// step that said so. (The step is here because the engine labels every new
/// unknown with the step that wrote it, and a typed field is the only place
/// this format can keep the label.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Known<T> {
    Known(T),
    Unknown { step: String, reason: String },
}

// ---------- T0 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind0 {
    Int,
    Float,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct S0 {
    pub name: String,
    pub size: u32,
    pub kind: Kind0,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct P0 {
    pub title: String,
    pub scalars: Vec<S0>,
}

// ---------- T1 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Base1 {
    Int,
    Float,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kind1 {
    pub base: Base1,
    /// A float's width. Absent for what has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct S1 {
    pub name: String,
    pub size: u64,
    pub kind: Kind1,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct P1 {
    pub title: String,
    pub scalars: Vec<S1>,
    /// The root `unknown` blob: the untyped carrier. Absent from the tree
    /// when empty, so a version without unknowns keeps its bytes.
    pub unknown: Vec<UnknownRecord>,
}

// ---------- T2 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Base2 {
    Int,
    Float,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kind2 {
    pub base: Base2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct S2 {
    pub name: String,
    pub slug: String,
    pub bytes: u64,
    pub kind: Kind2,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct P2 {
    pub title: String,
    pub owner: Known<String>,
    pub total_size: Known<u64>,
    pub scalars: Vec<S2>,
    pub unknown: Vec<UnknownRecord>,
}

/// The payload of any of the three formats: how the harness handles them
/// without being generic over each.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyPayload {
    T0(P0),
    T1(P1),
    T2(P2),
}

impl AnyPayload {
    pub fn number(&self) -> u32 {
        match self {
            Self::T0(_) => T0,
            Self::T1(_) => T1,
            Self::T2(_) => T2,
        }
    }

    /// The model, as JSON: what an unknown's path points into.
    pub fn value(&self) -> Value {
        let value = match self {
            Self::T0(p) => serde_json::to_value(p),
            Self::T1(p) => serde_json::to_value(p),
            Self::T2(p) => serde_json::to_value(p),
        };
        value.expect("a payload serializes")
    }
}

/// How a format records an unknown at a path of its model.
pub enum Carrier {
    /// A `Known<T>` field: the value is `Known::Unknown`.
    Typed,
    /// The field has no room: it holds this placeholder, and an entry of
    /// the `unknown` blob says so.
    Untyped(Value),
}

/// The carrier of the unknown at `path`. A width has no room in a kind;
/// every other path the formats can have an unknown at is a `Known<T>`.
pub fn carrier(path: &str) -> Carrier {
    if path.ends_with("/kind/bits") {
        Carrier::Untyped(Value::from(WIDTH_PLACEHOLDER))
    } else {
        Carrier::Typed
    }
}

// ---------- what a build of each format writes ----------

/// The slug T2 records: the name, lower case, with its spaces as hyphens.
pub fn slug(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

fn bits_of(kind: SourceKind) -> Option<u8> {
    match kind {
        SourceKind::Float32 => Some(32),
        SourceKind::Float64 => Some(64),
        SourceKind::Int | SourceKind::Text => None,
    }
}

/// What the core of format 80 writes for `source`: no width for a float.
pub fn fresh0(source: &Source) -> P0 {
    P0 {
        title: source.title.clone(),
        scalars: source
            .scalars
            .iter()
            .map(|s| S0 {
                name: s.name.clone(),
                size: s.size,
                kind: match s.kind {
                    SourceKind::Int => Kind0::Int,
                    SourceKind::Float32 | SourceKind::Float64 => Kind0::Float,
                    SourceKind::Text => Kind0::Text,
                },
                tags: s.tags.clone(),
            })
            .collect(),
    }
}

/// What the core of format 81 writes: a float's width is known.
pub fn fresh1(source: &Source) -> P1 {
    P1 {
        title: source.title.clone(),
        scalars: source
            .scalars
            .iter()
            .map(|s| S1 {
                name: s.name.clone(),
                size: u64::from(s.size),
                kind: Kind1 {
                    base: match s.kind {
                        SourceKind::Int => Base1::Int,
                        SourceKind::Float32 | SourceKind::Float64 => Base1::Float,
                        SourceKind::Text => Base1::Text,
                    },
                    bits: bits_of(s.kind),
                },
                tags: s.tags.clone(),
            })
            .collect(),
        unknown: vec![],
    }
}

/// What the core of format 82 writes: the owner, the total and the slugs.
pub fn fresh2(source: &Source) -> P2 {
    P2 {
        title: source.title.clone(),
        owner: Known::Known(source.owner.clone()),
        total_size: Known::Known(source.scalars.iter().map(|s| u64::from(s.size)).sum()),
        scalars: source
            .scalars
            .iter()
            .map(|s| S2 {
                name: s.name.clone(),
                slug: slug(&s.name),
                bytes: u64::from(s.size),
                kind: Kind2 {
                    base: match s.kind {
                        SourceKind::Int => Base2::Int,
                        SourceKind::Float32 | SourceKind::Float64 => Base2::Float,
                        SourceKind::Text => Base2::Text,
                    },
                    bits: bits_of(s.kind),
                },
                tags: s.tags.clone(),
            })
            .collect(),
        unknown: vec![],
    }
}

/// What the core of format `number` writes for `source`.
pub fn fresh(number: u32, source: &Source) -> AnyPayload {
    match number {
        T0 => AnyPayload::T0(fresh0(source)),
        T1 => AnyPayload::T1(fresh1(source)),
        T2 => AnyPayload::T2(fresh2(source)),
        other => panic!("no test-only format {other}"),
    }
}

// ---------- the codecs ----------
//
// A version's root tree: `meta` (a blob), `scalars` (a tree with one blob per
// scalar, `scalar_0000`, `scalar_0001`, ...) and, only when there is
// something in it, `unknown`. Nothing else, so a tree with an entry these
// codecs don't know fails the canonical-encoding check.

#[derive(Serialize, Deserialize)]
struct Meta01 {
    title: String,
}

#[derive(Serialize, Deserialize)]
struct Meta2 {
    title: String,
    owner: Known<String>,
    total_size: Known<u64>,
}

fn put<T: serde::Serialize + ?Sized>(sink: &mut dyn Sink, value: &T) -> Result<Hash> {
    sink.blob(to_cbor(value)?)
}

fn write<M, S>(
    sink: &mut dyn Sink,
    meta: &M,
    scalars: &[S],
    unknown: &[UnknownRecord],
) -> Result<Hash>
where
    M: serde::Serialize,
    S: serde::Serialize,
{
    let mut root = Tree::new();
    root.add_entry(EntryMode::Blob, "meta".into(), put(sink, meta)?);
    let mut dir = Tree::new();
    for (index, scalar) in scalars.iter().enumerate() {
        dir.add_entry(
            EntryMode::Blob,
            format!("scalar_{index:04}"),
            put(sink, scalar)?,
        );
    }
    let dir = sink.tree(&dir)?;
    root.add_entry(EntryMode::Tree, "scalars".into(), dir);
    if !unknown.is_empty() {
        root.add_entry(EntryMode::Blob, "unknown".into(), put(sink, unknown)?);
    }
    sink.tree(&root)
}

fn read<M, S>(objects: &dyn Objects, root: &Hash) -> Result<(M, Vec<S>, Vec<UnknownRecord>)>
where
    M: serde::de::DeserializeOwned,
    S: serde::de::DeserializeOwned,
{
    let tree = objects.tree(root)?;
    let meta_entry = tree
        .find_entry("meta")
        .ok_or_else(|| eyre!("no `meta` entry"))?;
    let meta: M = from_cbor(&objects.blob(&meta_entry.hash)?)?;
    let dir_entry = tree
        .find_entry("scalars")
        .ok_or_else(|| eyre!("no `scalars` entry"))?;
    let dir = objects.tree(&dir_entry.hash)?;
    let mut scalars: Vec<S> = vec![];
    for entry in &dir.entries {
        scalars.push(from_cbor(&objects.blob(&entry.hash)?)?);
    }
    let unknown: Vec<UnknownRecord> = match tree.find_entry("unknown") {
        Some(entry) => from_cbor(&objects.blob(&entry.hash)?)?,
        None => vec![],
    };
    Ok((meta, scalars, unknown))
}

/// Format 80.
pub struct Format0;

impl Format for Format0 {
    type Payload = P0;

    fn number(&self) -> u32 {
        T0
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<P0> {
        let (meta, scalars, unknown): (Meta01, Vec<S0>, Vec<UnknownRecord>) = read(objects, root)?;
        if !unknown.is_empty() {
            return Err(eyre!("format 80 has no `unknown` entry"));
        }
        Ok(P0 {
            title: meta.title,
            scalars,
        })
    }

    fn encode(&self, payload: &P0, sink: &mut dyn Sink) -> Result<Hash> {
        let meta = Meta01 {
            title: payload.title.clone(),
        };
        write(sink, &meta, &payload.scalars, &[])
    }

    fn unknowns(&self, _: &P0) -> Vec<UnknownRecord> {
        vec![]
    }
}

/// Format 81.
pub struct Format1;

impl Format for Format1 {
    type Payload = P1;

    fn number(&self) -> u32 {
        T1
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<P1> {
        let (meta, scalars, unknown): (Meta01, Vec<S1>, Vec<UnknownRecord>) = read(objects, root)?;
        Ok(P1 {
            title: meta.title,
            scalars,
            unknown,
        })
    }

    fn encode(&self, payload: &P1, sink: &mut dyn Sink) -> Result<Hash> {
        let meta = Meta01 {
            title: payload.title.clone(),
        };
        write(sink, &meta, &payload.scalars, &payload.unknown)
    }

    fn unknowns(&self, payload: &P1) -> Vec<UnknownRecord> {
        payload.unknown.clone()
    }
}

/// Format 82.
pub struct Format2;

impl Format for Format2 {
    type Payload = P2;

    fn number(&self) -> u32 {
        T2
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<P2> {
        let (meta, scalars, unknown): (Meta2, Vec<S2>, Vec<UnknownRecord>) = read(objects, root)?;
        Ok(P2 {
            title: meta.title,
            owner: meta.owner,
            total_size: meta.total_size,
            scalars,
            unknown,
        })
    }

    fn encode(&self, payload: &P2, sink: &mut dyn Sink) -> Result<Hash> {
        let meta = Meta2 {
            title: payload.title.clone(),
            owner: payload.owner.clone(),
            total_size: payload.total_size.clone(),
        };
        write(sink, &meta, &payload.scalars, &payload.unknown)
    }

    /// The untyped carrier's entries, and every `Known::Unknown` field,
    /// each at its path in the model.
    fn unknowns(&self, payload: &P2) -> Vec<UnknownRecord> {
        let mut found = payload.unknown.clone();
        if let Known::Unknown { step, reason } = &payload.owner {
            found.push(typed("/owner", step, reason));
        }
        if let Known::Unknown { step, reason } = &payload.total_size {
            found.push(typed("/total_size", step, reason));
        }
        found
    }
}

fn typed(path: &str, step: &str, reason: &str) -> UnknownRecord {
    UnknownRecord {
        path: path.into(),
        step: step.into(),
        reason: reason.into(),
        original: None,
    }
}

/// Decode a version of format `number`.
pub fn decode_any(number: u32, objects: &dyn Objects, root: &Hash) -> Result<AnyPayload> {
    Ok(match number {
        T0 => AnyPayload::T0(Format0.decode(objects, root)?),
        T1 => AnyPayload::T1(Format1.decode(objects, root)?),
        T2 => AnyPayload::T2(Format2.decode(objects, root)?),
        other => return Err(eyre!("no test-only format {other}")),
    })
}

/// Encode a version of any of the formats.
pub fn encode_any(payload: &AnyPayload, sink: &mut dyn Sink) -> Result<Hash> {
    match payload {
        AnyPayload::T0(p) => Format0.encode(p, sink),
        AnyPayload::T1(p) => Format1.encode(p, sink),
        AnyPayload::T2(p) => Format2.encode(p, sink),
    }
}
