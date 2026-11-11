//! The raw description a generator draws, and how it becomes a [`Model`].
//!
//! A [`Spec`] is selectors and flags, never a reference that could dangle: a
//! pick is a number reduced modulo what there is to pick from, so any value
//! of any shape is a valid graph. That is what lets proptest shrink it freely
//! and the fuzz target read it from any bytes. [`Spec::model`] resolves the
//! picks, and the "exactly what is needed" choices (a keyed marker listing
//! what the protocol gives up, an acceptance of exactly the set) are filled in
//! from the oracle on the graph as drawn, so they hold the graph's own
//! numbers: a change to the graph afterwards ([`Change`]) leaves them as they
//! were, which is what an upstream change to a package a consumer already
//! accepted is.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;
use crate::oracle;

/// What a protocol or the package says about why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckSpec {
    Nothing,
    Plain,
    /// Keyed, with exactly the keys that are needed (the guarantees given up,
    /// minus what the package covers; for a package, what its protocols give
    /// up through its settings). A plain one where nothing is needed: the
    /// marker that gives nothing up.
    Needed,
    /// Nothing where nothing is needed, else as `Needed`: what an author
    /// who got it right writes.
    Auto,
    /// Keyed, with the keys whose bit is set: one per guarantee, then
    /// `bounded_sise`.
    Keys(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TyKind {
    Str { bounded: bool },
    Bytes { bounded: bool },
    /// A struct the position can see, picked by `pick`, plain or in a list.
    Struct { array: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TySpec {
    pub kind: TyKind,
    pub pick: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructSpec {
    pub max_depth: bool,
    pub fields: Vec<TySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolSpec {
    pub handshake: Handshake,
    pub marker: AckSpec,
    pub args: Vec<TySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSpec {
    pub structs: Vec<StructSpec>,
    pub protocols: Vec<ProtocolSpec>,
}

/// What a consumer writes for one protocol of a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptSpec {
    /// Exactly what it gives up.
    Exact,
    /// Nothing.
    Omit,
    /// The guarantees whose bit is set, whatever it gives up.
    Set(u8),
    /// What it gives up, and the bits besides.
    Plus(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepSpec {
    /// Which lower package, modulo.
    pub on: u8,
    /// For each protocol of the dependency's closure, in order; `Exact` past
    /// the end.
    pub accepts: Vec<AcceptSpec>,
    /// Write each acceptance's guarantees in reverse order.
    pub reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    pub gate: bool,
    pub handshake: Handshake,
    pub unverified: AckSpec,
    pub schemas: Vec<SchemaSpec>,
    pub deps: Vec<DepSpec>,
}

/// An upstream change between two builds: the consumers' acceptances stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A field, which may hold no maximum, added to a struct.
    AddField { pkg: u8, schema: u8, strukt: u8, ty: TySpec },
    /// A field's type swapped for another.
    SwapField { pkg: u8, schema: u8, strukt: u8, field: u8, ty: TySpec },
    /// A package's default handshake changed.
    SetHandshake { pkg: u8, handshake: Handshake },
    /// A protocol's own handshake changed.
    SetProtocolHandshake { pkg: u8, schema: u8, protocol: u8, handshake: Handshake },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub packages: Vec<PackageSpec>,
    pub change: Option<Change>,
}

// ---- Resolving ----

/// The structs a field or argument at `at` can name: everything in its own
/// schema, the schemas before it in its package, and every schema of the
/// packages it depends on. A struct's field names a plain one only if it
/// is declared before the struct (`plain`), so only lists close a cycle.
fn visible(m: &Model, at: (usize, usize), owner: Option<TypeId>, array: bool) -> Vec<TypeId> {
    let mut out = vec![];
    let (pkg, schema) = at;
    let mut schemas: Vec<(usize, usize)> = (0..=schema).map(|s| (pkg, s)).collect();
    for dep in &m.packages[pkg].deps {
        schemas.extend((0..m.packages[dep.on].schemas.len()).map(|s| (dep.on, s)));
    }
    for (p, s) in schemas {
        out.extend((0..m.packages[p].schemas[s].structs.len()).map(|idx| TypeId { pkg: p, schema: s, idx }));
    }
    if !array {
        if let Some(owner) = owner {
            out.retain(|id| *id < owner);
        }
    }
    out
}

fn resolve_ty(m: &Model, at: (usize, usize), owner: Option<TypeId>, spec: TySpec) -> Ty {
    match spec.kind {
        TyKind::Str { bounded } => Ty::Str { bounded },
        TyKind::Bytes { bounded } => Ty::Bytes { bounded },
        TyKind::Struct { array } => {
            let candidates = visible(m, at, owner, array);
            match candidates.len() {
                0 => Ty::Str { bounded: true },
                n => Ty::Struct { of: candidates[spec.pick as usize % n], array },
            }
        }
    }
}

fn keys(mask: u8) -> BTreeSet<Key> {
    let mut set: BTreeSet<Key> = G::ALL.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, g)| Key::Guarantee(*g)).collect();
    if mask >> G::ALL.len() & 1 == 1 {
        set.insert(Key::Unknown);
    }
    set
}

fn guarantees(mask: u8) -> BTreeSet<G> {
    G::ALL.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, g)| *g).collect()
}

/// A keyed acknowledgement that lists nothing isn't written: it is the plain one.
fn keyed(set: BTreeSet<Key>) -> Ack {
    if set.is_empty() {
        Ack::Plain
    } else {
        Ack::Keyed(set)
    }
}

impl Spec {
    /// The graph as drawn.
    pub fn model(&self) -> Model {
        assert!(!self.packages.is_empty(), "a graph has a package");
        let n = self.packages.len();
        let mut m = Model { packages: vec![] };
        // What was written for each dependency a package resolves to.
        let mut written: Vec<Vec<Option<&DepSpec>>> = vec![];

        // Structure first: types, then the references between them, which
        // need every struct to exist to be picked from.
        for (pkg, spec) in self.packages.iter().enumerate() {
            let mut deps: Vec<(usize, Option<&DepSpec>)> = vec![];
            if pkg > 0 {
                for dep in &spec.deps {
                    let on = dep.on as usize % pkg;
                    if !deps.iter().any(|(d, _)| *d == on) {
                        deps.push((on, Some(dep)));
                    }
                }
                // Every package past the first depends on something.
                if deps.is_empty() {
                    deps.push((pkg - 1, None));
                }
            }
            written.push(deps.iter().map(|(_, s)| *s).collect());
            let schemas = spec
                .schemas
                .iter()
                .map(|s| Schema {
                    structs: s.structs.iter().map(|st| Struct { max_depth: st.max_depth, fields: vec![] }).collect(),
                    protocols: s.protocols.iter().map(|q| Protocol { handshake: q.handshake, marker: None, args: vec![] }).collect(),
                })
                .collect();
            m.packages.push(Package {
                gate: spec.gate,
                handshake: spec.handshake,
                unverified: None,
                schemas,
                deps: deps.into_iter().map(|(on, _)| Dep { on, accepts: BTreeMap::new() }).collect(),
            });
        }
        for (pkg, spec) in self.packages.iter().enumerate() {
            for (schema, s) in spec.schemas.iter().enumerate() {
                for (idx, st) in s.structs.iter().enumerate() {
                    let owner = TypeId { pkg, schema, idx };
                    let fields = st.fields.iter().map(|f| resolve_ty(&m, (pkg, schema), Some(owner), *f)).collect();
                    m.packages[pkg].schemas[schema].structs[idx].fields = fields;
                }
                for (idx, q) in s.protocols.iter().enumerate() {
                    let args = q.args.iter().map(|a| resolve_ty(&m, (pkg, schema), None, *a)).collect();
                    m.packages[pkg].schemas[schema].protocols[idx].args = args;
                }
            }
        }

        // What each package says, then each protocol (which may depend on
        // what the package covers), then each acceptance (which depends on
        // what the protocols give up).
        for (pkg, spec) in self.packages.iter().enumerate() {
            m.packages[pkg].unverified = match spec.unverified {
                AckSpec::Nothing => None,
                AckSpec::Plain => Some(Ack::Plain),
                AckSpec::Keys(mask) => Some(keyed(keys(mask))),
                AckSpec::Needed | AckSpec::Auto => {
                    // What the handshake gives up, for the protocols that take it.
                    let takes = m.packages[pkg].schemas.iter().flat_map(|s| &s.protocols).any(|q| q.handshake == Handshake::Default);
                    let needed = if takes && m.packages[pkg].handshake == Handshake::None {
                        BTreeSet::from([Key::Guarantee(G::SchemaAgreement), Key::Guarantee(G::EncodingAgreement)])
                    } else {
                        BTreeSet::new()
                    };
                    (!needed.is_empty()).then(|| Ack::Keyed(needed))
                }
            };
        }
        for (pkg, spec) in self.packages.iter().enumerate() {
            for (schema, s) in spec.schemas.iter().enumerate() {
                for (idx, q) in s.protocols.iter().enumerate() {
                    let id = ProtocolId { pkg, schema, idx };
                    m.packages[pkg].schemas[schema].protocols[idx].marker = match q.marker {
                        AckSpec::Nothing => None,
                        AckSpec::Plain => Some(Ack::Plain),
                        AckSpec::Keys(mask) => Some(keyed(keys(mask))),
                        AckSpec::Needed | AckSpec::Auto => {
                            let covered = oracle::covered_by_package(&m, id);
                            let needed: BTreeSet<Key> =
                                oracle::given_up(&m, id).difference(&covered).map(|g| Key::Guarantee(*g)).collect();
                            (!needed.is_empty() || q.marker == AckSpec::Needed).then(|| keyed(needed))
                        }
                    };
                }
            }
        }
        for pkg in 0..n {
            for (d, dep) in m.packages[pkg].deps.clone().iter().enumerate() {
                let spec_dep = written[pkg][d];
                let accepts = m.protocols_of(&m.closure(dep.on)).into_iter().enumerate().filter_map(|(k, p)| {
                    let given = oracle::given_up(&m, p);
                    let how = spec_dep.and_then(|s| s.accepts.get(k).copied()).unwrap_or(AcceptSpec::Exact);
                    let set: BTreeSet<G> = match how {
                        AcceptSpec::Exact => given,
                        AcceptSpec::Omit => return None,
                        AcceptSpec::Set(mask) => guarantees(mask),
                        AcceptSpec::Plus(mask) => given.union(&guarantees(mask)).copied().collect(),
                    };
                    let mut set: Vec<G> = set.into_iter().collect();
                    if set.is_empty() && how == AcceptSpec::Exact {
                        return None;
                    }
                    if spec_dep.is_some_and(|s| s.reversed) {
                        set.reverse();
                    }
                    Some((m.protocol_name(p), set))
                });
                let accepts: BTreeMap<String, Vec<G>> = accepts.collect();
                m.packages[pkg].deps[d].accepts = accepts;
            }
        }
        m
    }
}

impl Model {
    /// The graph after `change`. A pick that names nothing (a package with no
    /// struct to add a field to) changes nothing.
    pub fn changed(&self, change: &Change) -> Model {
        let mut m = self.clone();
        let pkg = |p: &u8| *p as usize % m.packages.len();
        match change {
            Change::AddField { pkg: p, schema, strukt, ty } | Change::SwapField { pkg: p, schema, strukt, ty, .. } => {
                let p = pkg(p);
                let schema = *schema as usize % m.packages[p].schemas.len();
                let count = m.packages[p].schemas[schema].structs.len();
                if count == 0 {
                    return m;
                }
                let idx = *strukt as usize % count;
                let owner = TypeId { pkg: p, schema, idx };
                let ty = resolve_ty(&m, (p, schema), Some(owner), *ty);
                let fields = &mut m.packages[p].schemas[schema].structs[idx].fields;
                match change {
                    Change::SwapField { field, .. } if !fields.is_empty() => {
                        let at = *field as usize % fields.len();
                        fields[at] = ty;
                    }
                    _ => fields.push(ty),
                }
            }
            Change::SetHandshake { pkg: p, handshake } => {
                let p = pkg(p);
                m.packages[p].handshake = *handshake;
            }
            Change::SetProtocolHandshake { pkg: p, schema, protocol, handshake } => {
                let p = pkg(p);
                let schema = *schema as usize % m.packages[p].schemas.len();
                let count = m.packages[p].schemas[schema].protocols.len();
                if count > 0 {
                    m.packages[p].schemas[schema].protocols[*protocol as usize % count].handshake = *handshake;
                }
            }
        }
        m
    }
}
