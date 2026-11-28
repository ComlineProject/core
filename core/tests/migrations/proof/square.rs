// The square (core#268, step 1.4; proof.md, "What accurate means"):
//
//                  a package's source, at each point in its history
//              ┌────────────────────┴────────────────────┐
//     build with the core of N                   build with the core of C
//              │                                         │
//       a store of format N                       a store of format C
//              │                                         │
//         migrate N -> C                           forget N -> C
//              │                                         │
//              └──────────────── must be equal ──────────┘
//
// `forget` is not a second implementation of a step. It acts on a *current*
// value, takes the steps' own `unknown` lists, and does one thing: it puts an
// unknown where `up` would have, at every path of the value that a pattern
// of those lists covers, and touches nothing else. How an unknown is
// recorded at a path (a `Known::Unknown`, or a placeholder and an entry of
// the `unknown` blob) is the format's own convention (`formats::carrier`).
//
// What is compared is the model of each version as JSON, after `forget`:
// every field exactly, the unknown entries by `path`, `step` and `reason`
// (the entry's `original` is what N recorded, which a fresh build of C can't
// tell; the witness holds it to account, and the lossy test reads it against
// the old store). Commits are compared by version, release record and
// parents through the old-to-new map, and never by `author`, `timestamp`,
// `message` or hash, which a fresh build can't share (here they are the
// script's, and a commit is found by its message).
//
// Three kinds of mistake each break the equality, and the failure says which:
//
//   Wrong     a value N could know is not the fresh one
//   Invented  a value where `forget` has an unknown: N could not know it
//   Needless  an unknown where `forget` keeps the fresh value: N could know it
//
// The square is also the only thing that sees a step that declares an
// unknown and writes a value instead: the engine holds a step to *the
// unknowns it writes being declared*, not to the declared ones being written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use comline_core::package::build::cas::migrate::{
    path_matches, Consent, Container, Engine, MigrateError, MigrateOptions, Step, UnknownDecl,
    Version, VersionId,
};
use comline_core::package::build::cas::reconcile::Heads;
use comline_core::package::build::cas::storage::Hash;
use comline_ir_formats::v3::types::UnknownRecord;
use eyre::{bail, Result};
use serde_json::Value;

use super::formats::*;
use super::histories::{project, History};
use super::steps::{registry, Defect, Up01, Up12};

static NULL: Value = Value::Null;

/// Why a migration is not what a fresh build would have made.
// The payloads are read through `Debug`, in the messages of failed assertions.
#[allow(dead_code)]
#[derive(Debug)]
pub enum Failure {
    /// The engine refused the migration (its witness, the unknowns it holds
    /// a step to, its verification).
    Engine(eyre::Report),
    /// A step, called directly, failed.
    Step(eyre::Report),
    /// A store could not be read.
    Read(eyre::Report),
    /// A value N could know is not the fresh one.
    Wrong { path: String, detail: String },
    /// A value where `forget` has an unknown.
    Invented { path: String },
    /// An unknown where `forget` keeps the value.
    Needless { path: String },
    /// A commit, a parent, a release record or a ref is not the same through
    /// the old-to-new map.
    Structure(String),
}

impl Failure {
    /// The engine's own error, if it is what refused.
    pub fn engine_error(&self) -> Option<&MigrateError> {
        match self {
            Self::Engine(report) => report.downcast_ref::<MigrateError>(),
            _ => None,
        }
    }
}

/// What a step declares: the steps' `unknown` lists, in the order of the
/// chain, with the step each belongs to.
pub struct Declared {
    pub step: String,
    pub decls: Vec<UnknownDecl>,
}

/// The lists of the steps from `from` to `to`.
pub fn declared(from: u32, to: u32, first: Defect, second: Defect) -> Vec<Declared> {
    let mut chain = vec![];
    if from <= T0 && to >= T1 {
        chain.push(Declared {
            step: format!("{T0}-{T1}"),
            decls: Up01(first).unknown(),
        });
    }
    if from <= T1 && to >= T2 {
        chain.push(Declared {
            step: format!("{T1}-{T2}"),
            decls: Up12(second).unknown(),
        });
    }
    chain
}

// ---------- forget ----------

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Every JSON pointer into `value`, except the root.
fn pointers(value: &Value, here: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let path = format!("{here}/{}", escape(key));
                out.push(path.clone());
                pointers(inner, &path, out);
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                let path = format!("{here}/{index}");
                out.push(path.clone());
                pointers(inner, &path, out);
            }
        }
        _ => {}
    }
}

/// A current value, as `up` would have left it where the lists say N never
/// recorded something: at each path a pattern covers, an unknown, and
/// nothing else changed.
pub fn forget(fresh: &AnyPayload, chain: &[Declared]) -> Value {
    let mut value = fresh.value();
    let mut paths = vec![];
    pointers(&value, "", &mut paths);
    for step in chain {
        for decl in &step.decls {
            for path in paths.iter().filter(|p| path_matches(&decl.pattern, p)) {
                match carrier(path) {
                    Carrier::Typed => {
                        let unknown = serde_json::to_value(Known::<()>::Unknown {
                            step: step.step.clone(),
                            reason: decl.reason.clone(),
                        })
                        .unwrap();
                        *value.pointer_mut(path).expect("a path of the value") = unknown;
                    }
                    Carrier::Untyped(placeholder) => {
                        *value.pointer_mut(path).expect("a path of the value") = placeholder;
                        let record = serde_json::to_value(UnknownRecord {
                            path: path.clone(),
                            step: step.step.clone(),
                            reason: decl.reason.clone(),
                            original: None,
                        })
                        .unwrap();
                        value["unknown"]
                            .as_array_mut()
                            .expect("a format with an untyped carrier has an `unknown` list")
                            .push(record);
                    }
                }
            }
        }
    }
    value
}

// ---------- compare ----------

/// The model without what is not compared: the entries' `original`, and the
/// entries in a fixed order.
fn masked(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(records) = value.get_mut("unknown").and_then(Value::as_array_mut) {
        for record in records.iter_mut() {
            if let Some(object) = record.as_object_mut() {
                object.remove("original");
            }
        }
        records.sort_by_key(|record| {
            (
                record["path"].as_str().unwrap_or("").to_string(),
                record["step"].as_str().unwrap_or("").to_string(),
            )
        });
    }
    value
}

/// `Known::Unknown` fields of the model, by path.
fn typed_unknowns(value: &Value, here: &str, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("Unknown") {
                out.insert(here.to_string(), value.clone());
                return;
            }
            for (key, inner) in map {
                if here.is_empty() && key == "unknown" {
                    continue;
                }
                typed_unknowns(inner, &format!("{here}/{}", escape(key)), out);
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                typed_unknowns(inner, &format!("{here}/{index}"), out);
            }
        }
        _ => {}
    }
}

/// Every unknown a model holds: the entries of its `unknown` list and its
/// `Known::Unknown` fields, by path.
fn unknown_paths(value: &Value) -> BTreeMap<String, Value> {
    let mut found = BTreeMap::new();
    if let Some(records) = value.get("unknown").and_then(Value::as_array) {
        for record in records {
            if let Some(path) = record["path"].as_str() {
                found.insert(path.to_string(), record.clone());
            }
        }
    }
    typed_unknowns(value, "", &mut found);
    found
}

fn first_difference(a: &Value, b: &Value, here: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for key in keys {
                let path = format!("{here}/{}", escape(key));
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => {
                        if let Some(found) = first_difference(p, q, &path) {
                            return Some(found);
                        }
                    }
                    _ => return Some(path),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(here.to_string());
            }
            for (index, (p, q)) in x.iter().zip(y).enumerate() {
                if let Some(found) = first_difference(p, q, &format!("{here}/{index}")) {
                    return Some(found);
                }
            }
            None
        }
        _ => (a != b).then(|| here.to_string()),
    }
}

/// Whether what a step migrated is what `forget` made of the fresh build, and
/// if not, which kind of mistake it is.
pub fn compare(migrated: &Value, forgotten: &Value) -> Result<(), Failure> {
    let (m, f) = (masked(migrated), masked(forgotten));
    let (migrated_unknown, forgotten_unknown) = (unknown_paths(&m), unknown_paths(&f));
    if let Some(path) = forgotten_unknown
        .keys()
        .find(|path| !migrated_unknown.contains_key(*path))
    {
        return Err(Failure::Invented { path: path.clone() });
    }
    if let Some(path) = migrated_unknown
        .keys()
        .find(|path| !forgotten_unknown.contains_key(*path))
    {
        return Err(Failure::Needless { path: path.clone() });
    }
    for (path, record) in &migrated_unknown {
        let other = &forgotten_unknown[path];
        if record != other {
            return Err(Failure::Wrong {
                path: path.clone(),
                detail: format!("the unknown is {record}, forget's is {other}"),
            });
        }
    }
    let (mut m, mut f) = (m, f);
    for model in [&mut m, &mut f] {
        if let Some(object) = model.as_object_mut() {
            object.remove("unknown");
        }
    }
    match first_difference(&m, &f, "") {
        None => Ok(()),
        Some(path) => Err(Failure::Wrong {
            detail: format!(
                "migrated {}, forget's {}",
                m.pointer(&path).unwrap_or(&NULL),
                f.pointer(&path).unwrap_or(&NULL)
            ),
            path,
        }),
    }
}

// ---------- the steps, called directly ----------

fn dummy<P>(payload: P) -> Version<P> {
    Version {
        id: VersionId(0),
        parents: vec![],
        version: "0.1.0-dev.1".into(),
        release: None,
        author: "tester".into(),
        timestamp: 0,
        message: "a version".into(),
        payload,
    }
}

/// One step up, as the engine would call it (without its checks).
pub fn up(payload: &AnyPayload, first: Defect, second: Defect) -> Result<AnyPayload> {
    Ok(match payload {
        AnyPayload::T0(p) => AnyPayload::T1(Up01(first).up(dummy(p.clone()))?.payload),
        AnyPayload::T1(p) => AnyPayload::T2(Up12(second).up(dummy(p.clone()))?.payload),
        AnyPayload::T2(_) => bail!("no step out of format {T2}"),
    })
}

/// One step down: the witness.
pub fn down(payload: &AnyPayload, first: Defect, second: Defect) -> Result<AnyPayload> {
    Ok(match payload {
        AnyPayload::T2(p) => AnyPayload::T1(Up12(second).down(dummy(p.clone()))?.payload),
        AnyPayload::T1(p) => AnyPayload::T0(Up01(first).down(dummy(p.clone()))?.payload),
        AnyPayload::T0(_) => bail!("no step into format {T0}"),
    })
}

/// `down(up(v)) == v`.
pub fn witness(payload: &AnyPayload, first: Defect, second: Defect) -> Result<bool> {
    let migrated = up(payload, first, second)?;
    Ok(&down(&migrated, first, second)? == payload)
}

/// The chain from the payload's format to `to`, without the engine.
pub fn migrate_pure(
    payload: &AnyPayload,
    to: u32,
    first: Defect,
    second: Defect,
) -> Result<AnyPayload> {
    let mut current = payload.clone();
    while current.number() < to {
        current = up(&current, first, second)?;
    }
    Ok(current)
}

/// The square on one source, with no store and no engine: the steps are
/// called directly, so a mistake the engine would stop first is seen by the
/// square alone.
pub fn pure_square(
    source: &Source,
    from: u32,
    to: u32,
    first: Defect,
    second: Defect,
) -> Result<(), Failure> {
    let migrated = migrate_pure(&fresh(from, source), to, first, second).map_err(Failure::Step)?;
    let forgotten = forget(&fresh(to, source), &declared(from, to, first, second));
    compare(&migrated.value(), &forgotten)
}

// ---------- the square on stores ----------

fn message_of(container: &Container, hash: &Hash) -> String {
    container
        .index
        .get(hash)
        .map(|id| container.commits[id.0].commit.message.clone())
        .unwrap_or_else(|| format!("(a commit that is not in the history: {hash})"))
}

/// What the square compares exactly of a commit, through the old-to-new map:
/// the version, the release record and the parents, with the commits they
/// name by their messages.
#[derive(Debug, PartialEq)]
struct Shape {
    version: String,
    release: Option<(String, String, Option<String>, bool, bool, Option<String>)>,
    parents: Vec<String>,
}

fn shape(container: &Container, index: usize) -> Shape {
    let commit = &container.commits[index].commit;
    Shape {
        version: commit.version.clone(),
        release: commit.release.as_ref().map(|release| {
            (
                format!("{:?}", release.computed),
                format!("{:?}", release.applied),
                release.reason.clone(),
                release.covered,
                release.stable,
                release
                    .since
                    .as_ref()
                    .map(|hash| message_of(container, hash)),
            )
        }),
        parents: commit
            .parents
            .iter()
            .map(|hash| message_of(container, hash))
            .collect(),
    }
}

fn heads(container: &Container) -> BTreeMap<String, Vec<String>> {
    container
        .refs
        .iter()
        .map(|(name, heads)| {
            let hashes = match heads {
                Heads::One(head) => vec![*head],
                Heads::Two(ours, theirs) => vec![*ours, *theirs],
            };
            (
                name.clone(),
                hashes
                    .iter()
                    .map(|hash| message_of(container, hash))
                    .collect(),
            )
        })
        .collect()
}

/// The history of two stores is the same through the old-to-new map: the
/// same commits (found by their messages), and for each the same version,
/// release record and parents, and the same refs. Returns the index of each
/// commit in the first store with its index in the second.
pub fn same_history(a: &Container, b: &Container) -> Result<Vec<(usize, usize)>, Failure> {
    let by_message = |container: &Container| -> BTreeMap<String, usize> {
        container
            .commits
            .iter()
            .enumerate()
            .map(|(index, node)| (node.commit.message.clone(), index))
            .collect()
    };
    let (in_a, in_b) = (by_message(a), by_message(b));
    if in_a.keys().ne(in_b.keys()) {
        return Err(Failure::Structure(format!(
            "one store has the commits {:?}, the other {:?}",
            in_a.keys().collect::<Vec<_>>(),
            in_b.keys().collect::<Vec<_>>()
        )));
    }
    let mut pairs = vec![];
    for (message, &i) in &in_a {
        let j = in_b[message];
        let (ours, theirs) = (shape(a, i), shape(b, j));
        if ours != theirs {
            return Err(Failure::Structure(format!(
                "{message}: {ours:?} against {theirs:?}"
            )));
        }
        pairs.push((i, j));
    }
    if heads(a) != heads(b) {
        return Err(Failure::Structure(format!(
            "the refs name {:?} in one store, {:?} in the other",
            heads(a),
            heads(b)
        )));
    }
    Ok(pairs)
}

/// The square on two store directories: `migrated` is the old store after
/// the engine, `fresh_store` what the core of `to` built from the same
/// script.
pub fn compare_stores(
    migrated: &Path,
    fresh_store: &Path,
    to: u32,
    chain: &[Declared],
) -> Result<(), Failure> {
    let m = Container::open(migrated).map_err(Failure::Read)?;
    let f = Container::open(fresh_store).map_err(Failure::Read)?;
    for (i, j) in same_history(&m, &f)? {
        let message = &m.commits[i].commit.message;
        let migrated_payload =
            decode_any(to, &m.store, &m.commits[i].commit.tree).map_err(Failure::Read)?;
        let fresh_payload =
            decode_any(to, &f.store, &f.commits[j].commit.tree).map_err(Failure::Read)?;
        compare(&migrated_payload.value(), &forget(&fresh_payload, chain)).map_err(|failure| {
            match failure {
                Failure::Wrong { path, detail } => Failure::Wrong {
                    path,
                    detail: format!("{message}: {detail}"),
                },
                other => other,
            }
        })?;
    }
    Ok(())
}

/// Run the engine over a store the core of format `from` built from
/// `history`, and compare what it made with what the core of `to` builds.
pub fn disk_square(
    history: &History,
    from: u32,
    to: u32,
    first: Defect,
    second: Defect,
) -> Result<(), Failure> {
    let old = project(history, from);
    let fresh_project = project(history, to);
    let mut engine = Engine::new(registry(to, first, second));
    engine
        .migrate(old.path(), &MigrateOptions::default(), Consent::Command)
        .map_err(Failure::Engine)?;
    compare_stores(
        &old.path().join(".comline"),
        &fresh_project.path().join(".comline"),
        to,
        &declared(from, to, first, second),
    )
}
