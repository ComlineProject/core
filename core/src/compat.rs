//! What a frozen version provides for checking compatibility at connection
//! time (`comline@2`, runtime#15): the package id, each protocol's line and
//! wire-shape hash, each function's "since" version, and each encoding's
//! name and parameter hash.
//!
//! - **Line:** the release at which a protocol last had a breaking change,
//!   by the diff's rules (`schema::ir::diff`): its own breaking changes and
//!   those of the declarations it reaches (dependencies' included, by
//!   reach). A break elsewhere in the package doesn't move it; a change to
//!   the package's wire defaults (`wire.format`, `wire.framing`,
//!   `handshake`) moves every protocol's. Before 1.0 a breaking change makes
//!   a `0.x` minor release, so the line is that release.
//! - **Since:** the release in which a function first appeared in its
//!   protocol. One removed and later added again starts again.
//!
//! Both name **releases** only. A build between releases (`0.4.0-dev.2`)
//! carries the lines of the release it leads to (`0.4.0`); `comline
//! release` renames them if the release it makes is another version (a
//! raised bump, or 1.0). Both are computed against the last release, not
//! build by build, so what a series of builds did and undid doesn't count.
//!
//! They need history, so each build records them in its root tree, as a
//! `facts` blob ([`FACTS_ENTRY`]) beside `config` and `docs`; a version
//! built before that gets them computed when read. The hashes need only
//! the version itself, so they're computed on demand.

// Standard Uses
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

// Crate Uses
use crate::package::build::cas::build::{compare, last_release};
use crate::package::build::cas::object_store::ObjectStore;
use crate::package::build::cas::objects::{Blob, Commit, EntryMode, Tree};
use crate::package::build::cas::refs::{main_ref, read_ref, ref_exists};
use crate::package::build::cas::storage::{from_cbor, to_cbor, Hash};
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::diff::ConfigChange;
use crate::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use crate::schema::ir::diff::{
    analyze_package_changes_with, concerns_protocol, protocol_reach, BreakingChange, WireFacts,
};
use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::{schema_namespace, ConfigValue, FrozenUnit};

// External Uses
use eyre::{bail, Result};
use serde_derive::{Deserialize, Serialize};

/// The root tree entry holding a version's [`History`].
pub const FACTS_ENTRY: &str = "facts";

type Schemas = Vec<Vec<FrozenUnit>>;

/// What history gives each protocol: stored per version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub protocols: Vec<ProtocolHistory>,
}

/// One protocol's line and its functions' "since" versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolHistory {
    /// Its qualified path, `namespace::Protocol`: protocols of the same name
    /// in two schemas are two protocols.
    pub path: String,
    /// The release at which it last broke.
    pub line: String,
    /// In wire (declaration) order.
    pub functions: Vec<FunctionFacts>,
}

/// A function and the release it first appeared in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFacts {
    pub name: String,
    pub since: String,
}

/// Everything a version provides for `comline@2`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFacts {
    /// `id` in `config.idp`; `None` for a version built without one.
    pub id: Option<String>,
    /// The version these are the facts of.
    pub version: String,
    /// The package's own protocols, in schema and declaration order.
    pub protocols: Vec<ProtocolFacts>,
    /// Every encoding the protocols use, by kind then name.
    pub encodings: Vec<EncodingFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFacts {
    /// `namespace::Protocol`: what the facts are keyed by.
    pub path: String,
    /// The schema it's declared in.
    pub namespace: String,
    pub name: String,
    pub line: String,
    /// blake3, hex, of the protocol's canonical wire shape ([`shape_hash`]).
    pub shape_hash: String,
    pub functions: Vec<FunctionFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingFacts {
    /// `format`, `framing` or `handshake`.
    pub kind: String,
    /// The canonical name (`packed@1`, `datagram@2`, `comline@1`).
    pub name: String,
    /// Its parameters as written (a framing's `{ name = ..., k = v }`
    /// entries other than `name`), plus `profile`: a declared encoding's
    /// profile hash.
    pub params: BTreeMap<String, String>,
    /// blake3, hex, of the CBOR of `(kind, name, params)`.
    pub param_hash: String,
}

// ---------- reading and storing ----------

/// A version's stored history, if its tree has one.
pub fn read_history(store: &ObjectStore, tree: &Tree) -> Result<Option<History>> {
    let Some(entry) = tree.entries.iter().find(|e| e.name == FACTS_ENTRY && e.mode == EntryMode::Blob)
    else {
        return Ok(None);
    };
    let blob = Blob::from_bytes(&store.read(&entry.hash)?)?;
    Ok(Some(from_cbor(&blob.content)?))
}

/// Store `history` as a blob, for a root tree's [`FACTS_ENTRY`].
pub fn write_history(store: &ObjectStore, history: &History) -> Result<Hash> {
    store.write(&Blob::new(to_cbor(history)?).to_bytes()?)
}

/// `tree` without its [`FACTS_ENTRY`].
pub fn without_facts(tree: &Tree) -> Tree {
    Tree { entries: tree.entries.iter().filter(|e| e.name != FACTS_ENTRY).cloned().collect() }
}

/// `tree` with `history` as its [`FACTS_ENTRY`], replacing any it had.
pub fn with_facts(store: &ObjectStore, tree: &Tree, history: &History) -> Result<Tree> {
    let mut tree = without_facts(tree);
    tree.add_entry(EntryMode::Blob, FACTS_ENTRY.to_string(), write_history(store, history)?);
    Ok(tree)
}

/// The release a commit's version is, or leads to: `0.4.0-dev.2` -> `0.4.0`.
pub fn release_of(version: &str) -> String {
    match semver::Version::parse(version) {
        Ok(v) => semver::Version::new(v.major, v.minor, v.patch).to_string(),
        Err(_) => version.to_string(),
    }
}

/// The history of the commit `hash`: stored, or computed for a version
/// built before it was stored.
pub fn history_of_commit(store: &ObjectStore, hash: &Hash) -> Result<History> {
    let commit = Commit::from_bytes(&store.read(hash)?)?;
    let tree = Tree::from_bytes(&store.read(&commit.tree)?)?;
    if let Some(history) = read_history(store, &tree)? {
        return Ok(history);
    }
    compute_history_of_commit(store, &commit, &tree)
}

/// The history of `commit` computed from what came before, ignoring any it
/// stores. Recomputing a stored one gives it back (the test of
/// reproducibility).
pub fn compute_history_of_commit(store: &ObjectStore, commit: &Commit, tree: &Tree) -> Result<History> {
    let previous = match commit.parents.first() {
        Some(parent) => last_release(store, *parent)?.0,
        None => None,
    };
    let previous = match previous {
        Some((hash, release)) => {
            let release_tree = Tree::from_bytes(&store.read(&release.tree)?)?;
            Some((history_of_commit(store, &hash)?, release_tree))
        }
        None => None,
    };
    history(store, previous.as_ref().map(|(h, t)| (h, t)), tree, &release_of(&commit.version))
}

/// The history of the version `tree`, which is or leads to the release
/// `target`, after the last release (its history and tree), if any.
pub fn history(
    store: &ObjectStore,
    previous: Option<(&History, &Tree)>,
    tree: &Tree,
    target: &str,
) -> Result<History> {
    let new = Loaded::of(store, tree)?;
    let Some((before, old_tree)) = previous else {
        return Ok(History {
            protocols: new
                .protocols()
                .into_iter()
                .map(|(namespace, name, functions)| ProtocolHistory {
                    path: protocol_path(namespace, name),
                    line: target.to_string(),
                    functions: functions
                        .into_iter()
                        .map(|f| FunctionFacts { name: f.to_string(), since: target.to_string() })
                        .collect(),
                })
                .collect(),
        });
    };
    let old = Loaded::of(store, old_tree)?;
    let comparison = compare(store, old_tree, tree)?;
    let dependency_breaking: Vec<_> =
        comparison.dependency_changes.iter().flat_map(|d| &d.changes.breaking_changes).collect();
    let package_wire_moved = comparison.config_changes.0.iter().any(|c| {
        matches!(c, ConfigChange::WireFormatChanged { .. } | ConfigChange::WireFramingChanged { .. })
    }) || package_handshake(&old.config) != package_handshake(&new.config);

    let every: Vec<&[Vec<FrozenUnit>]> = old.all().chain(new.all()).collect();
    // Renames keep a protocol's history: a namespace or a protocol renamed
    // is the same protocol under another path.
    let namespace_renames: Vec<(&str, &str)> = comparison
        .schema_changes
        .breaking_changes
        .iter()
        .filter_map(|c| match c {
            BreakingChange::RenamedNamespace { old_name, new_name } => Some((old_name.as_str(), new_name.as_str())),
            _ => None,
        })
        .collect();
    let protocol_renames: Vec<(&str, &str)> = comparison
        .schema_changes
        .breaking_changes
        .iter()
        .filter_map(|c| match c {
            BreakingChange::RenamedProtocol { old_name, new_name } => Some((old_name.as_str(), new_name.as_str())),
            _ => None,
        })
        .collect();
    let old_paths: BTreeSet<String> =
        old.protocols().into_iter().map(|(ns, name, _)| protocol_path(ns, name)).collect();
    let facts = WireFacts::of((&old.own, &old.config), (&new.own, &new.config));

    let mut protocols = vec![];
    for (namespace, name, functions) in new.protocols() {
        // Where it was at the last release.
        let was_namespace = namespace_renames
            .iter()
            .find(|(_, to)| *to == namespace)
            .map_or(namespace, |(from, _)| *from);
        let was_name = protocol_renames.iter().find(|(_, to)| *to == name).map_or(name, |(from, _)| *from);
        let was: Option<(&str, &str)> = [(was_namespace, was_name), (was_namespace, name)]
            .into_iter()
            .find(|(ns, n)| old_paths.contains(&protocol_path(ns, n)));
        let earlier = was.and_then(|(ns, n)| {
            let path = protocol_path(ns, n);
            before.protocols.iter().find(|p| p.path == path)
        });

        // Its own changes: the diff with every other protocol left out, so
        // a change to one of the same name elsewhere isn't its; then what
        // it reaches, here and in dependencies.
        let (old_ns, old_name) = was.unwrap_or((namespace, name));
        let own = analyze_package_changes_with(
            &only_protocol(&old.own, old_ns, old_name),
            &only_protocol(&new.own, namespace, name),
            &facts,
        );
        let namespaces = [old_ns, namespace];
        let mut reach = protocol_reach(name, Some(&namespaces), &every);
        reach.extend(protocol_reach(old_name, Some(&namespaces), &every));
        let broke = own.breaking_changes.iter().any(|c| concerns_protocol(c, name, &reach) || concerns_protocol(c, old_name, &reach))
            // A dependency's own protocols are never this one.
            || dependency_breaking.iter().any(|c| concerns_protocol(c, "", &reach));
        let line = match earlier {
            Some(p) if !broke && !package_wire_moved => p.line.clone(),
            _ => target.to_string(),
        };
        let functions = functions
            .into_iter()
            .map(|f| {
                let since = earlier
                    .and_then(|p| p.functions.iter().find(|e| e.name == f))
                    .map_or_else(|| target.to_string(), |e| e.since.clone());
                FunctionFacts { name: f.to_string(), since }
            })
            .collect();
        protocols.push(ProtocolHistory { path: protocol_path(namespace, name), line, functions });
    }
    Ok(History { protocols })
}

/// `history` with every version `from` named as `to`: a release made as
/// another version than its builds led to.
pub fn renamed(history: &History, from: &str, to: &str) -> History {
    let rename = |v: &String| if v == from { to.to_string() } else { v.clone() };
    History {
        protocols: history
            .protocols
            .iter()
            .map(|p| ProtocolHistory {
                path: p.path.clone(),
                line: rename(&p.line),
                functions: p
                    .functions
                    .iter()
                    .map(|f| FunctionFacts { name: f.name.clone(), since: rename(&f.since) })
                    .collect(),
            })
            .collect(),
    }
}

/// A protocol's qualified path: `namespace::Protocol`.
pub fn protocol_path(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        format!("{namespace}::{name}")
    }
}

/// `schemas` with every protocol but `namespace::name` left out.
fn only_protocol(schemas: &Schemas, namespace: &str, name: &str) -> Schemas {
    schemas
        .iter()
        .map(|units| {
            let here = schema_namespace(units).unwrap_or("");
            units
                .iter()
                .filter(|u| match u {
                    FrozenUnit::Protocol { name: n, .. } => here == namespace && n == name,
                    _ => true,
                })
                .cloned()
                .collect()
        })
        .collect()
}

fn package_handshake(config: &[ConfigUnit]) -> Option<&str> {
    config.iter().find_map(|u| match u {
        ConfigUnit::Handshake(h) => Some(h.as_str()),
        _ => None,
    })
}

/// A stored version: its own schemas, its dependencies' (vendored), and
/// its config.
struct Loaded {
    own: Schemas,
    dependencies: Vec<Schemas>,
    config: Vec<ConfigUnit>,
}

impl Loaded {
    fn of(store: &ObjectStore, tree: &Tree) -> Result<Self> {
        use crate::package::config::ir::frozen::cas::blob as config_blob;
        use crate::schema::ir::frozen::cas::blob::load_schema_from_tree;

        let dependency_names: BTreeSet<&str> = tree
            .entries
            .iter()
            .filter(|e| e.mode == EntryMode::Tree)
            .filter_map(|e| e.name.strip_prefix("dep_"))
            .collect();
        let mut own = vec![];
        let mut dependencies: BTreeMap<String, Schemas> = BTreeMap::new();
        for entry in &tree.entries {
            if entry.mode == EntryMode::Tree && entry.name.starts_with("schema_") {
                let schema_tree = Tree::from_bytes(&store.read(&entry.hash)?)?;
                let units = load_schema_from_tree(store, &schema_tree)?;
                let first = schema_namespace(&units)
                    .and_then(|ns| ns.split_once("::").map(|(first, _)| first))
                    .filter(|first| dependency_names.contains(first))
                    .map(str::to_string);
                match first {
                    Some(dependency) => dependencies.entry(dependency).or_default().push(units),
                    None => own.push(units),
                }
            }
        }
        let config = tree
            .entries
            .iter()
            .find(|e| e.name == "config" && e.mode == EntryMode::Blob)
            .map(|e| config_blob::read_config(store, &e.hash))
            .transpose()?
            .unwrap_or_default();
        Ok(Loaded { own, dependencies: dependencies.into_values().collect(), config })
    }

    fn all(&self) -> impl Iterator<Item = &[Vec<FrozenUnit>]> {
        std::iter::once(self.own.as_slice()).chain(self.dependencies.iter().map(Vec::as_slice))
    }

    /// The package's own protocols and their functions, in order. Std's
    /// schemas aren't the package's.
    fn protocols(&self) -> Vec<(&str, &str, Vec<&str>)> {
        let mut found = vec![];
        for units in &self.own {
            if schema_namespace(units).is_some_and(|ns| ns == "std" || ns.starts_with("std::")) {
                continue;
            }
            for unit in units {
                if let FrozenUnit::Protocol { name, functions, .. } = unit {
                    let functions = functions
                        .iter()
                        .filter_map(|f| match f {
                            FrozenUnit::Function { name, .. } => Some(name.as_str()),
                            _ => None,
                        })
                        .collect();
                    found.push((schema_namespace(units).unwrap_or(""), name.as_str(), functions));
                }
            }
        }
        found
    }
}

// ---------- the facts ----------

/// The facts of a built version: the latest build, or the version
/// `version` (`0.3.0`, `0.4.0-dev.2`) in its history.
pub fn facts(project_path: &Path, version: Option<&str>) -> Result<PackageFacts> {
    if !ref_exists(project_path, main_ref()) {
        bail!("nothing built yet: run `comline build` first");
    }
    let store = ObjectStore::new(project_path);
    let mut hash = read_ref(project_path, main_ref())?;
    loop {
        let commit = Commit::from_bytes(&store.read(&hash)?)?;
        if version.map_or(true, |v| v == commit.version) {
            return facts_of_commit(&store, &hash);
        }
        match commit.parents.first() {
            Some(parent) => hash = *parent,
            None => bail!("no version {} in the package's history", version.unwrap_or_default()),
        }
    }
}

/// The facts of the commit `hash`.
pub fn facts_of_commit(store: &ObjectStore, hash: &Hash) -> Result<PackageFacts> {
    let commit = Commit::from_bytes(&store.read(hash)?)?;
    let tree = Tree::from_bytes(&store.read(&commit.tree)?)?;
    let history = history_of_commit(store, hash)?;
    let loaded = Loaded::of(store, &tree)?;
    Ok(assemble(&loaded, &history, &commit.version))
}

/// The facts of a compiled project, as its next build would record them,
/// when the history isn't at hand: every line and since is `version`.
pub fn facts_of_context(context: &ProjectContext, version: &str) -> PackageFacts {
    let own: Schemas = context
        .schema_contexts
        .iter()
        .filter_map(|s| s.borrow().frozen_schema.borrow().clone())
        .collect();
    let dependency_names: BTreeSet<&str> =
        context.dependency_configs.iter().map(|(n, _)| n.as_str()).collect();
    let (mut mine, mut theirs) = (vec![], vec![]);
    for units in own {
        let first = schema_namespace(&units).and_then(|ns| ns.split_once("::").map(|(f, _)| f.to_string()));
        if first.as_deref().is_some_and(|f| dependency_names.contains(f)) {
            theirs.push(units);
        } else {
            mine.push(units);
        }
    }
    let loaded = Loaded {
        own: mine,
        dependencies: vec![theirs],
        config: context.config_frozen.clone().unwrap_or_default(),
    };
    let history = History {
        protocols: loaded
            .protocols()
            .into_iter()
            .map(|(namespace, name, functions)| ProtocolHistory {
                path: protocol_path(namespace, name),
                line: version.to_string(),
                functions: functions
                    .into_iter()
                    .map(|f| FunctionFacts { name: f.to_string(), since: version.to_string() })
                    .collect(),
            })
            .collect(),
    };
    assemble(&loaded, &history, version)
}

fn assemble(loaded: &Loaded, history: &History, version: &str) -> PackageFacts {
    let index = Index::of(loaded);
    let mut protocols = vec![];
    let mut encodings: BTreeMap<(String, String, String), EncodingFacts> = BTreeMap::new();
    for units in &loaded.own {
        let namespace = schema_namespace(units).unwrap_or("");
        if namespace == "std" || namespace.starts_with("std::") {
            continue;
        }
        for unit in units {
            let FrozenUnit::Protocol { name, .. } = unit else { continue };
            let in_play = encodings_in_play(unit, &loaded.config);
            for e in &in_play {
                encodings.insert((e.kind.clone(), e.name.clone(), e.param_hash.clone()), e.clone());
            }
            let path = protocol_path(namespace, name);
            let Some(h) = history.protocols.iter().find(|p| p.path == path) else { continue };
            protocols.push(ProtocolFacts {
                path,
                namespace: namespace.to_string(),
                name: name.clone(),
                line: h.line.clone(),
                shape_hash: shape_hash(unit, namespace, &index, &in_play),
                functions: h.functions.clone(),
            });
        }
    }
    PackageFacts {
        id: crate::package::config::ir::frozen::package_id(&loaded.config).map(str::to_string),
        version: version.to_string(),
        protocols,
        encodings: encodings.into_values().collect(),
    }
}

// ---------- encodings ----------

/// The format, framing and handshake a protocol uses, resolved as
/// `comline_core::inspect` resolves them.
fn encodings_in_play(protocol: &FrozenUnit, config: &[ConfigUnit]) -> Vec<EncodingFacts> {
    use crate::schema::idl::annotations::{canonical_value, DEFAULT_FORMAT, DEFAULT_FRAMING, FORMAT_JSON, FRAMING_JSONRPC};
    let FrozenUnit::Protocol { parameters, .. } = protocol else { return vec![] };
    let (wire_format, wire_framing) = crate::package::config::ir::frozen::wire(config);
    let property = |key: &str| {
        parameters.iter().find_map(|u| match u {
            FrozenUnit::Property { name, value } if name == key => Some(value),
            _ => None,
        })
    };
    let (framing, framing_params) = match property("framing") {
        Some(value) => {
            let (name, params) = crate::inspect::split(value);
            (canonical_value("framing", &name).map(str::to_string).unwrap_or(name), params)
        }
        None => (wire_framing.unwrap_or(DEFAULT_FRAMING).to_string(), BTreeMap::new()),
    };
    let format = match wire_format {
        Some(f) => f.to_string(),
        None if framing == FRAMING_JSONRPC => FORMAT_JSON.to_string(),
        None => DEFAULT_FORMAT.to_string(),
    };
    let handshake = crate::schema::ir::unverified::handshake(parameters).to_string();
    let declared = |name: &str| {
        config.iter().find_map(|u| match u {
            ConfigUnit::Encoding { name: n, profile_hash, .. } if n == name => Some(profile_hash.clone()),
            _ => None,
        })
    };
    [("format", format, BTreeMap::new()), ("framing", framing, framing_params), ("handshake", handshake, BTreeMap::new())]
        .into_iter()
        .map(|(kind, name, mut params)| {
            if let Some(profile) = declared(&name) {
                params.insert("profile".to_string(), profile);
            }
            encoding(kind, &name, params)
        })
        .collect()
}

/// An encoding's facts: its parameter hash covers its kind, name and
/// parameters.
pub fn encoding(kind: &str, name: &str, params: BTreeMap<String, String>) -> EncodingFacts {
    let bytes = to_cbor(&(kind, name, &params)).expect("CBOR of strings");
    EncodingFacts {
        kind: kind.to_string(),
        name: name.to_string(),
        param_hash: blake3::hash(&bytes).to_hex().to_string(),
        params,
    }
}

// ---------- the wire shape ----------

/// Declarations by qualified and bare name, to resolve what a protocol
/// reaches.
struct Index<'a> {
    qualified: HashMap<String, &'a FrozenUnit>,
    bare: HashMap<String, Vec<(&'a str, &'a FrozenUnit)>>,
    /// Errors by ordinal, per namespace.
    errors: HashMap<(String, u16), &'a FrozenUnit>,
}

impl<'a> Index<'a> {
    fn of(loaded: &'a Loaded) -> Self {
        let mut index = Index { qualified: HashMap::new(), bare: HashMap::new(), errors: HashMap::new() };
        for schemas in loaded.all() {
            for units in schemas {
                let ns = schema_namespace(units).unwrap_or("");
                for unit in units {
                    let name = match unit {
                        FrozenUnit::Struct { name, .. }
                        | FrozenUnit::Enum { name, .. }
                        | FrozenUnit::Error { name, .. }
                        | FrozenUnit::TypeAlias { name, .. } => name,
                        _ => continue,
                    };
                    if let FrozenUnit::Error { ordinal, .. } = unit {
                        index.errors.insert((ns.to_string(), *ordinal), unit);
                    }
                    if matches!(unit, FrozenUnit::Error { imported_from: Some(_), .. }) {
                        continue;
                    }
                    index.qualified.insert(format!("{ns}::{name}"), unit);
                    index.bare.entry(name.clone()).or_default().push((ns, unit));
                }
            }
        }
        index
    }

    /// The declaration `name` names, written in `namespace`.
    fn find(&self, name: &str, namespace: &str) -> Option<&'a FrozenUnit> {
        if let Some(unit) = self.qualified.get(name) {
            return Some(unit);
        }
        let last = name.rsplit_once("::").map_or(name, |(_, l)| l);
        let candidates = self.bare.get(last)?;
        candidates
            .iter()
            .find(|(ns, _)| *ns == namespace)
            .or_else(|| {
                let prefix = name.rsplit_once("::").map(|(p, _)| p);
                candidates.iter().find(|(ns, _)| prefix.is_some_and(|p| ns.ends_with(p)))
            })
            .or(candidates.first())
            .map(|(_, u)| *u)
    }
}

/// A type as it travels: aliases resolved, named declarations by their
/// position in the shape's table (names of types don't travel).
#[derive(Serialize)]
enum Ty {
    Primitive(String),
    Declared(u32),
    Array(Box<Ty>, Option<u64>),
    Union(Vec<Ty>),
    Unit,
    Unresolved(String),
}

#[derive(Serialize)]
enum Declared {
    Struct(Vec<Field>),
    Enum(Vec<String>),
    Error { ordinal: u16, name: String, fields: Vec<Field> },
}

#[derive(Serialize)]
struct Field {
    name: String,
    ty: Ty,
    optional: bool,
}

#[derive(Serialize)]
struct Function {
    name: String,
    arguments: Vec<(String, Ty)>,
    returns: Option<Ty>,
    throws: Vec<Ty>,
    /// Direction annotations (`@provider`, `@consumer`): who calls it.
    direction: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Shape<'a> {
    encodings: Vec<(&'a str, &'a str, &'a str)>,
    functions: Vec<Function>,
    declared: Vec<Declared>,
}

struct Builder<'a, 'i> {
    index: &'i Index<'a>,
    /// Where the format keys fields by position (`struct_fields`): their
    /// order travels. Otherwise they're hashed sorted by name.
    fields_ordered: bool,
    /// Where the format encodes variants by index (`enum_variants`).
    variants_ordered: bool,
    seen: HashMap<*const FrozenUnit, u32>,
    declared: Vec<Option<Declared>>,
}

impl<'a, 'i> Builder<'a, 'i> {
    fn ty(&mut self, ty: &TypeRef, namespace: &str, depth: usize) -> Ty {
        if depth > 64 {
            return Ty::Unresolved(ty.to_string());
        }
        match ty {
            TypeRef::Primitive(p) => Ty::Primitive(TypeRef::Primitive(*p).to_string()),
            TypeRef::Unit => Ty::Unit,
            TypeRef::Array { element, length } => {
                Ty::Array(Box::new(self.ty(element, namespace, depth + 1)), *length)
            }
            TypeRef::Union(types) => {
                Ty::Union(types.iter().map(|t| self.ty(t, namespace, depth + 1)).collect())
            }
            TypeRef::Named(name) => match self.index.find(name, namespace) {
                Some(FrozenUnit::TypeAlias { target, .. }) => self.ty(target, namespace, depth + 1),
                Some(unit) => Ty::Declared(self.declare(unit, namespace, depth)),
                None => Ty::Unresolved(name.rsplit_once("::").map_or(name.as_str(), |(_, l)| l).to_string()),
            },
        }
    }

    fn declare(&mut self, unit: &'a FrozenUnit, namespace: &str, depth: usize) -> u32 {
        let key = unit as *const FrozenUnit;
        if let Some(i) = self.seen.get(&key) {
            return *i;
        }
        let i = self.declared.len() as u32;
        self.seen.insert(key, i);
        self.declared.push(None);
        let declared = match unit {
            FrozenUnit::Struct { fields, .. } => Declared::Struct(self.fields(fields, namespace, depth)),
            FrozenUnit::Error { fields, ordinal, name, .. } => Declared::Error {
                ordinal: *ordinal,
                name: name.clone(),
                fields: self.fields(fields, namespace, depth),
            },
            FrozenUnit::Enum { variants, .. } => {
                let mut names: Vec<String> = variants
                    .iter()
                    .filter_map(|v| match v {
                        FrozenUnit::EnumVariant(name, _) => Some(name.clone()),
                        _ => None,
                    })
                    .collect();
                if !self.variants_ordered {
                    names.sort();
                }
                Declared::Enum(names)
            }
            _ => Declared::Enum(vec![]),
        };
        self.declared[i as usize] = Some(declared);
        i
    }

    fn fields(&mut self, fields: &'a [FrozenUnit], namespace: &str, depth: usize) -> Vec<Field> {
        // Sorted before they're walked, so what they reach is numbered in
        // an order their declaration order doesn't decide.
        let mut fields: Vec<&'a FrozenUnit> = fields.iter().collect();
        if !self.fields_ordered {
            fields.sort_by_key(|f| match f {
                FrozenUnit::Field { name, .. } => name.as_str(),
                _ => "",
            });
        }
        fields
            .into_iter()
            .filter_map(|f| match f {
                FrozenUnit::Field { name, ty, optional, .. } => Some(Field {
                    name: name.clone(),
                    ty: self.ty(ty, namespace, depth + 1),
                    optional: *optional,
                }),
                _ => None,
            })
            .collect()
    }
}

/// blake3, hex, of a protocol's canonical wire shape: the CBOR of its
/// encodings in play (each by its parameter hash), its functions (names,
/// arguments, return, thrown errors, direction) and every declaration they
/// reach, aliases resolved, each numbered in the order it's first reached.
/// Order counts only where it travels: functions in declaration order
/// where the framing calls them by index, fields and variants where the
/// format goes by position; otherwise each is sorted by name. Docstrings,
/// spans, validators, constants, defaults, advisory annotations and every
/// namespace and type name are left out: none of them travels.
fn shape_hash(protocol: &FrozenUnit, namespace: &str, index: &Index, encodings: &[EncodingFacts]) -> String {
    use crate::encodings::{official, EncodingKind, Profile};
    let FrozenUnit::Protocol { functions, .. } = protocol else { return String::new() };

    // Order is hashed only where it travels, by the encodings' profiles (as
    // the diff reads them, `WireFacts`); an encoding without an official
    // profile counts as the strictest, order-sensitive.
    let profile = |kind: EncodingKind, key: &str| {
        encodings.iter().find(|e| e.kind == key).and_then(|e| official::profile(kind, &e.name))
    };
    let states = |p: Option<&Profile>, fact: &str, values: &[&str]| {
        p.and_then(|p| p.facts.get(fact)).is_some_and(|f| values.contains(&f.value.as_str()))
    };
    let (format, framing) = (profile(EncodingKind::Format, "format"), profile(EncodingKind::Framing, "framing"));
    let calls_ordered = !states(framing, "calls", &["by_name", "by_id"]);
    let mut builder = Builder {
        index,
        fields_ordered: !states(format, "struct_fields", &["by_name"]),
        variants_ordered: !states(format, "enum_variants", &["by_name"]),
        seen: HashMap::new(),
        declared: vec![],
    };
    let mut functions: Vec<&FrozenUnit> = functions.iter().collect();
    if !calls_ordered {
        functions.sort_by_key(|f| match f {
            FrozenUnit::Function { name, .. } => name.as_str(),
            _ => "",
        });
    }
    let mut shaped = vec![];
    for function in functions {
        let FrozenUnit::Function { name, arguments, _return, throws, parameters, .. } = function else {
            continue;
        };
        let arguments =
            arguments.iter().map(|a| (a.name.clone(), builder.ty(&a.ty, namespace, 0))).collect();
        let returns = _return.as_ref().map(|t| builder.ty(t, namespace, 0));
        let throws = throws
            .iter()
            .map(|o| match index.errors.get(&(namespace.to_string(), *o)) {
                Some(unit) => Ty::Declared(builder.declare(unit, namespace, 0)),
                None => Ty::Unresolved(format!("error #{o}")),
            })
            .collect();
        let direction = parameters
            .iter()
            .filter_map(|p| match p {
                FrozenUnit::Property { name, value } if name == "provider" || name == "consumer" => {
                    Some((name.clone(), render(value)))
                }
                _ => None,
            })
            .collect();
        shaped.push(Function { name: name.clone(), arguments, returns, throws, direction });
    }
    let shape = Shape {
        encodings: encodings.iter().map(|e| (e.kind.as_str(), e.name.as_str(), e.param_hash.as_str())).collect(),
        functions: shaped,
        declared: builder.declared.into_iter().map(|d| d.expect("declared")).collect(),
    };
    blake3::hash(&to_cbor(&shape).expect("CBOR of a shape")).to_hex().to_string()
}

fn render(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Str(s) | ConfigValue::Name(s) | ConfigValue::Path(s) => s.clone(),
        other => format!("{other:?}"),
    }
}
