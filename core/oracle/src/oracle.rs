//! What core must do with a graph, worked out from the rules and nothing
//! else: whether it builds, and what each protocol then gives up.
//!
//! Deliberately naive, and independent of the compiler. It reads only the
//! [`Model`](crate::model::Model) and walks types by plain depth-first search
//! (it never says "the compiler's bounds pass does X"). Each rule says where
//! it comes from: the proposal (docs#162, *Granular `@unverified`*), the
//! issue (core#223), or the specification's compiler cases
//! (`core/tests/spec/cases/*.json`, by id).
//!
//! The rules, in the order they are checked:
//!
//! 1. **Causes** ([`given_up`]). A protocol gives up `schema_agreement` and
//!    `encoding_agreement` when its handshake is `none`: its own `@handshake`,
//!    else its package's. It gives up `bounded_size` when anything it reaches,
//!    through any chain of types, schemas and packages, has a string or list
//!    without a maximum, or contains itself without `@max_depth`.
//! 2. **Where bounds are checked** ([`bounds_errors`]). A position without a
//!    maximum is allowed only where every protocol reaching it says
//!    `@unverified`. "Reaching" is within the schema that declares the
//!    position: a type no protocol of its own schema reaches must be bounded,
//!    because another package can use it (`bounded/other-schema-*`). A type
//!    another schema declares that holds such a position is one where it is
//!    named (`chain/*`).
//! 3. **Acknowledgement** ([`protocol_errors`]). A protocol is acknowledged for
//!    what its marker covers (plain: all; keyed: the keys listed, which has to
//!    include everything it gives up and no key that isn't a guarantee), plus
//!    what its package covers for it: a cause that comes from the package's
//!    `handshake`, for a protocol that writes no `@handshake`, if the
//!    package's `unverified` says it (`package/*`, `marker/*`, `keyed/*`).
//! 4. **The gate.** `@unverified` and the package's `unverified` need
//!    `annotations.unverified.allowed = true` in that package
//!    (`marker/gate-off`, `package/gate-off`). The package's `unverified` can't
//!    name `bounded_size` or anything that isn't a guarantee
//!    (`package/wrong-scope`).
//! 5. **Acceptance** ([`acceptance_errors`]). A consumer's
//!    `accept_unverified` under each dependency has to equal, as a set, what
//!    every protocol of that dependency's package, and of what it depends on,
//!    gives up, whenever that is anything (`acceptance/*`, `chain/*`).
//!    Entries for protocols that give up nothing are not read.
//!
//! A package is judged by its own settings, so a build is every package in
//! the closure being sound on its own and every one of its dependency edges
//! being accepted (`acceptance/judged-by-its-own-package`).

use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;

/// What a build of the root must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prediction {
    /// It builds, and each protocol of every package in the closure gives up
    /// this (frozen with it: empty is nothing frozen).
    Builds(BTreeMap<String, BTreeSet<G>>),
    /// It doesn't, for these reasons (any one is enough), each starting with
    /// its kind in brackets: `[gate]`, `[package-unverified]`, `[bounds]`,
    /// `[marker]`, `[acceptance]`.
    Fails(Vec<String>),
}

impl Prediction {
    pub fn is_builds(&self) -> bool {
        matches!(self, Prediction::Builds(_))
    }
}

pub fn predict(m: &Model, root: usize) -> Prediction {
    let closure = m.closure(root);
    let mut errors = vec![];
    for &pkg in &closure {
        errors.extend(package_errors(m, pkg));
        errors.extend(acceptance_errors(m, pkg));
    }
    if !errors.is_empty() {
        return Prediction::Fails(errors);
    }
    // An unmarked, uncovered protocol that gives something up is an error
    // above, so what is given up is always what is frozen.
    Prediction::Builds(m.protocols_of(&closure).into_iter().map(|p| (m.protocol_name(p), given_up(m, p))).collect())
}

// ---- 1. Causes ----

/// Every struct reachable from `tys` by following fields, however far, in any
/// schema or package: a plain depth-first search.
fn reachable(m: &Model, tys: &[Ty]) -> BTreeSet<TypeId> {
    let mut seen = BTreeSet::new();
    let mut work: Vec<TypeId> = tys.iter().filter_map(of).collect();
    while let Some(id) = work.pop() {
        if seen.insert(id) {
            work.extend(m.strukt(id).fields.iter().filter_map(of));
        }
    }
    seen
}

fn of(ty: &Ty) -> Option<TypeId> {
    match ty {
        Ty::Struct { of, .. } => Some(*of),
        _ => None,
    }
}

fn is_unbounded_value(ty: &Ty) -> bool {
    matches!(ty, Ty::Str { bounded: false } | Ty::Bytes { bounded: false })
}

/// A struct that contains itself: a cycle, through a list, as it is the only
/// way a struct can (core#167). It needs `@max_depth` (`bounded/recursion-*`).
fn recursive_without_depth(m: &Model, id: TypeId) -> bool {
    !m.strukt(id).max_depth && reachable(m, &m.strukt(id).fields).contains(&id)
}

/// Whether `id`'s own fields, not what they name, hold a position without a
/// maximum: a string or list, or itself.
fn holds_own_unbounded(m: &Model, id: TypeId) -> bool {
    m.strukt(id).fields.iter().any(is_unbounded_value) || recursive_without_depth(m, id)
}

/// Whether anything reached from `tys`, or `tys` themselves, has no maximum.
fn reaches_unbounded(m: &Model, tys: &[Ty]) -> bool {
    tys.iter().any(is_unbounded_value) || reachable(m, tys).into_iter().any(|id| holds_own_unbounded(m, id))
}

/// The handshake a protocol has: its own, else its package's, else `comline@1`.
fn effective_handshake(m: &Model, p: ProtocolId) -> Handshake {
    match m.protocol(p).handshake {
        Handshake::Default => m.packages[p.pkg].handshake,
        own => own,
    }
}

/// What the protocol gives up, marked or not (core#223, docs#162).
pub fn given_up(m: &Model, p: ProtocolId) -> BTreeSet<G> {
    let mut set = BTreeSet::new();
    if effective_handshake(m, p) == Handshake::None {
        set.extend([G::SchemaAgreement, G::EncodingAgreement]);
    }
    if reaches_unbounded(m, &m.protocol(p).args) {
        set.insert(G::BoundedSize);
    }
    set
}

// ---- 2. Where bounds are checked ----

fn schema_of(id: TypeId) -> (usize, usize) {
    (id.pkg, id.schema)
}

/// Whether `ty` names a type another schema declares that reaches a position
/// without a maximum.
fn names_foreign_unbounded(m: &Model, here: (usize, usize), ty: &Ty) -> bool {
    match of(ty) {
        Some(id) => schema_of(id) != here && reaches_unbounded(m, std::slice::from_ref(ty)),
        None => false,
    }
}

/// Every position without a maximum that isn't allowed, in package `pkg`.
pub fn bounds_errors(m: &Model, pkg: usize) -> Vec<String> {
    let mut errors = vec![];
    for (schema, s) in m.packages[pkg].schemas.iter().enumerate() {
        let here = (pkg, schema);
        // A struct with a position of its own without a maximum is reached
        // by the protocols of this schema, and each says `@unverified`.
        for idx in 0..s.structs.len() {
            let id = TypeId { pkg, schema, idx };
            let fields = &m.strukt(id).fields;
            if !(holds_own_unbounded(m, id) || fields.iter().any(|t| names_foreign_unbounded(m, here, t))) {
                continue;
            }
            // Reaching goes through this schema's own types only: a path
            // through another schema's type ends there.
            let reaching: Vec<&Protocol> = s.protocols.iter().filter(|q| reaches_in_schema(m, here, &q.args, id)).collect();
            if reaching.is_empty() || reaching.iter().any(|q| q.marker.is_none()) {
                errors.push(format!("[bounds] {}: a position without a maximum, and not every protocol reaching it says `@unverified`", m.type_name(id)));
            }
        }
        // A protocol's own arguments.
        for (idx, q) in s.protocols.iter().enumerate() {
            let own = q.args.iter().any(is_unbounded_value) || q.args.iter().any(|t| names_foreign_unbounded(m, here, t));
            if own && q.marker.is_none() {
                errors.push(format!("[bounds] {}: an argument without a maximum, and no `@unverified`", m.protocol_name(ProtocolId { pkg, schema, idx })));
            }
        }
    }
    errors
}

/// Whether `target` is reached from `tys` by fields declared in schema `here`.
fn reaches_in_schema(m: &Model, here: (usize, usize), tys: &[Ty], target: TypeId) -> bool {
    let mut seen = BTreeSet::new();
    let mut work: Vec<TypeId> = tys.iter().filter_map(of).filter(|id| schema_of(*id) == here).collect();
    while let Some(id) = work.pop() {
        if seen.insert(id) {
            work.extend(m.strukt(id).fields.iter().filter_map(of).filter(|t| schema_of(*t) == here));
        }
    }
    seen.contains(&target)
}

// ---- 3 and 4. Acknowledgement and the gate ----

/// What the package's `unverified` covers for `p`: a guarantee given up whose
/// only cause is a package setting `p` takes, which the package says why of.
/// `bounded_size` comes from a type, never a setting (docs#162).
pub fn covered_by_package(m: &Model, p: ProtocolId) -> BTreeSet<G> {
    let package = &m.packages[p.pkg];
    let takes_handshake = m.protocol(p).handshake == Handshake::Default;
    let says = |g: G| match &package.unverified {
        None => false,
        Some(Ack::Plain) => true,
        Some(Ack::Keyed(keys)) => keys.contains(&Key::Guarantee(g)),
    };
    given_up(m, p)
        .into_iter()
        .filter(|g| matches!(g, G::SchemaAgreement | G::EncodingAgreement) && takes_handshake && says(*g))
        .collect()
}

/// Everything wrong with one protocol's acknowledgement.
pub fn protocol_errors(m: &Model, p: ProtocolId) -> Vec<String> {
    let name = m.protocol_name(p);
    let given = given_up(m, p);
    let covered = covered_by_package(m, p);
    let listed: BTreeSet<G> = match &m.protocol(p).marker {
        None => BTreeSet::new(),
        // One reason for everything (`marker/plain-marker`).
        Some(Ack::Plain) => given.clone(),
        Some(Ack::Keyed(keys)) => keys.iter().filter_map(|k| if let Key::Guarantee(g) = k { Some(*g) } else { None }).collect(),
    };
    let mut errors = vec![];
    if let Some(Ack::Keyed(keys)) = &m.protocol(p).marker {
        if keys.contains(&Key::Unknown) {
            errors.push(format!("[marker] {name}: a key that is no guarantee's id (`keyed/unknown-guarantee`)"));
        }
    }
    // A marker covers its own listing, the package what it covers.
    let missing: Vec<&str> = given.iter().filter(|g| !listed.contains(g) && !covered.contains(g)).map(|g| g.name()).collect();
    if !missing.is_empty() {
        errors.push(format!("[marker] {name}: gives up {missing:?} without saying why"));
    }
    errors
}

/// Everything wrong inside package `pkg`, apart from its dependencies.
pub fn package_errors(m: &Model, pkg: usize) -> Vec<String> {
    let package = &m.packages[pkg];
    let mut errors = vec![];

    let marked = package.schemas.iter().flat_map(|s| &s.protocols).any(|q| q.marker.is_some());
    if (marked || package.unverified.is_some()) && !package.gate {
        errors.push(format!("[gate] p{pkg}: `unverified` used, and the package's settings don't allow it"));
    }
    if let Some(Ack::Keyed(keys)) = &package.unverified {
        if keys.contains(&Key::Guarantee(G::BoundedSize)) || keys.contains(&Key::Unknown) {
            errors.push(format!("[package-unverified] p{pkg}: the package's `unverified` names `bounded_size` or no guarantee"));
        }
    }
    errors.extend(bounds_errors(m, pkg));
    for (schema, s) in package.schemas.iter().enumerate() {
        for idx in 0..s.protocols.len() {
            errors.extend(protocol_errors(m, ProtocolId { pkg, schema, idx }));
        }
    }
    errors
}

// ---- 5. Acceptance ----

/// What `pkg` fails to accept of each dependency.
pub fn acceptance_errors(m: &Model, pkg: usize) -> Vec<String> {
    let mut errors = vec![];
    for dep in &m.packages[pkg].deps {
        for p in m.protocols_of(&m.closure(dep.on)) {
            let name = m.protocol_name(p);
            let given = given_up(m, p);
            if given.is_empty() {
                continue;
            }
            let accepted: Option<BTreeSet<G>> = dep.accepts.get(&name).map(|set| set.iter().copied().collect());
            if accepted.as_ref() != Some(&given) {
                errors.push(format!("[acceptance] p{pkg}: {name} gives up {:?}, accepted for {accepted:?}", given.iter().map(|g| g.name()).collect::<Vec<_>>()));
            }
        }
    }
    errors
}
