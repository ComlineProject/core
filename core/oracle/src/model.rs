//! A package graph, concretely: what the oracle reads and the renderer writes.
//!
//! Packages are `p0`, `p1`, ... and each depends only on lower ones, so the
//! graph is a DAG by construction. A package's schemas are `src/s0.ids`,
//! `src/s1.ids`, ...; a schema holds structs and protocols. Names are unique
//! across the whole graph (`P1S0T2`, `P1S0Q0`) because an acceptance names a
//! protocol and nothing else.
//!
//! Types are ordered by `(package, schema, struct)`. A plain (non-array)
//! reference only goes to a lower one, and one through an array goes anywhere
//! the schema can see, so every cycle runs through a `[max N]` array, which
//! can be empty: no struct is infinite (`validation::struct-cycle`), and
//! recursion, a cycle, is always possible.

use std::collections::{BTreeMap, BTreeSet};

/// The guarantees core implements a rule for today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum G {
    SchemaAgreement,
    EncodingAgreement,
    EncodingConformance,
    VersionHonesty,
    BoundedSize,
}

impl G {
    pub const ALL: [G; 5] = [
        G::SchemaAgreement,
        G::EncodingAgreement,
        G::EncodingConformance,
        G::VersionHonesty,
        G::BoundedSize,
    ];

    /// The written id.
    pub fn name(self) -> &'static str {
        match self {
            G::SchemaAgreement => "schema_agreement",
            G::EncodingAgreement => "encoding_agreement",
            G::EncodingConformance => "encoding_conformance",
            G::VersionHonesty => "version_honesty",
            G::BoundedSize => "bounded_size",
        }
    }
}

/// A key in a keyed acknowledgement: a guarantee's id, or a misspelling of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Guarantee(G),
    /// `bounded_sise`: no guarantee's id.
    Unknown,
}

/// How a reason is given: one for everything (`"why"`), or one per guarantee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ack {
    Plain,
    Keyed(BTreeSet<Key>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    /// Nothing written: a protocol takes its package's, a package `comline@1`.
    Default,
    None,
    Comline1,
}

/// A struct, by where it is declared; `Ord` is the order references follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId {
    pub pkg: usize,
    pub schema: usize,
    pub idx: usize,
}

/// A protocol, by where it is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolId {
    pub pkg: usize,
    pub schema: usize,
    pub idx: usize,
}

/// A type as a field or an argument holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// `char[max 64]`, or `string`.
    Str { bounded: bool },
    /// `u8[max 16]`, or `u8[]`.
    Bytes { bounded: bool },
    /// `T`, or `T[max 4]`.
    Struct { of: TypeId, array: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Struct {
    /// `@max_depth = 4`.
    pub max_depth: bool,
    pub fields: Vec<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protocol {
    pub handshake: Handshake,
    /// `@unverified`.
    pub marker: Option<Ack>,
    /// The arguments of its one function.
    pub args: Vec<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    pub structs: Vec<Struct>,
    pub protocols: Vec<Protocol>,
}

/// A dependency, and what the package accepts of it: protocol name to the
/// guarantees, in the order written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub on: usize,
    pub accepts: BTreeMap<String, Vec<G>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// `settings = { annotations = { unverified = { allowed = true } } }`.
    pub gate: bool,
    /// `handshake = ...` in config.idp.
    pub handshake: Handshake,
    /// `unverified = ...` in config.idp.
    pub unverified: Option<Ack>,
    pub schemas: Vec<Schema>,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub packages: Vec<Package>,
}

impl Model {
    /// The package the build starts from: the last, which nothing depends on.
    pub fn root(&self) -> usize {
        self.packages.len() - 1
    }

    pub fn strukt(&self, id: TypeId) -> &Struct {
        &self.packages[id.pkg].schemas[id.schema].structs[id.idx]
    }

    pub fn protocol(&self, id: ProtocolId) -> &Protocol {
        &self.packages[id.pkg].schemas[id.schema].protocols[id.idx]
    }

    /// Every protocol of the packages in `pkgs`, in `ProtocolId` order.
    pub fn protocols_of(&self, pkgs: &BTreeSet<usize>) -> Vec<ProtocolId> {
        let mut out = vec![];
        for &pkg in pkgs {
            for (schema, s) in self.packages[pkg].schemas.iter().enumerate() {
                out.extend((0..s.protocols.len()).map(|idx| ProtocolId { pkg, schema, idx }));
            }
        }
        out
    }

    /// The package `root` and everything it depends on, however far.
    pub fn closure(&self, root: usize) -> BTreeSet<usize> {
        let mut seen = BTreeSet::from([root]);
        let mut work = vec![root];
        while let Some(pkg) = work.pop() {
            for dep in &self.packages[pkg].deps {
                if seen.insert(dep.on) {
                    work.push(dep.on);
                }
            }
        }
        seen
    }

    pub fn type_name(&self, id: TypeId) -> String {
        format!("P{}S{}T{}", id.pkg, id.schema, id.idx)
    }

    pub fn protocol_name(&self, id: ProtocolId) -> String {
        format!("P{}S{}Q{}", id.pkg, id.schema, id.idx)
    }
}

// ---- Rendering ----

const REASON: &str = "because";

fn ty_text(m: &Model, ty: &Ty) -> String {
    match ty {
        Ty::Str { bounded: true } => "char[max 64]".into(),
        Ty::Str { bounded: false } => "string".into(),
        Ty::Bytes { bounded: true } => "u8[max 16]".into(),
        Ty::Bytes { bounded: false } => "u8[]".into(),
        Ty::Struct { of, array: false } => m.type_name(*of),
        Ty::Struct { of, array: true } => format!("{}[max 4]", m.type_name(*of)),
    }
}

fn key_text(key: &Key) -> &'static str {
    match key {
        Key::Guarantee(g) => g.name(),
        Key::Unknown => "bounded_sise",
    }
}

fn handshake_text(h: Handshake) -> Option<&'static str> {
    match h {
        Handshake::Default => None,
        Handshake::None => Some("\"none\""),
        Handshake::Comline1 => Some("\"comline@1\""),
    }
}

impl Model {
    /// The package's files as `(path relative to the graph's directory, text)`:
    /// `p0/config.idp`, `p0/src/s0.ids`, ...
    pub fn files(&self) -> Vec<(String, String)> {
        let mut files = vec![];
        for (pkg, package) in self.packages.iter().enumerate() {
            files.push((format!("p{pkg}/config.idp"), self.config_text(pkg)));
            for schema in 0..package.schemas.len() {
                files.push((format!("p{pkg}/src/s{schema}.ids"), self.schema_text(pkg, schema)));
            }
        }
        files
    }

    /// All of it as one text, for a failure message.
    pub fn describe(&self) -> String {
        self.files().iter().map(|(path, text)| format!("--- {path}\n{text}")).collect::<Vec<_>>().join("\n")
    }

    fn config_text(&self, pkg: usize) -> String {
        let package = &self.packages[pkg];
        let mut out = format!("congregation p{pkg}\nspecification_version = 1\n");
        if let Some(h) = handshake_text(package.handshake) {
            out += &format!("handshake = {h}\n");
        }
        match &package.unverified {
            None => {}
            Some(Ack::Plain) => out += &format!("unverified = \"{REASON}\"\n"),
            Some(Ack::Keyed(keys)) => {
                out += "unverified = {\n";
                for key in keys {
                    out += &format!("    {} = \"{REASON}\"\n", key_text(key));
                }
                out += "}\n";
            }
        }
        if package.gate {
            out += "settings = { annotations = { unverified = { allowed = true } } }\n";
        }
        if !package.deps.is_empty() {
            out += "dependencies = {\n";
            for dep in &package.deps {
                out += &format!("    p{} = {{\n        path = \"../p{}\"\n", dep.on, dep.on);
                if !dep.accepts.is_empty() {
                    let each: Vec<String> = dep
                        .accepts
                        .iter()
                        .map(|(name, set)| {
                            let set: Vec<String> = set.iter().map(|g| format!("\"{}\"", g.name())).collect();
                            format!("{name} = [{}]", set.join(", "))
                        })
                        .collect();
                    out += &format!("        accept_unverified = {{ {} }}\n", each.join(" "));
                }
                out += "    }\n";
            }
            out += "}\n";
        }
        out
    }

    fn schema_text(&self, pkg: usize, schema: usize) -> String {
        let s = &self.packages[pkg].schemas[schema];

        // A `use` for each type another schema declares.
        let mut uses = BTreeSet::new();
        let tys = s.structs.iter().flat_map(|st| &st.fields).chain(s.protocols.iter().flat_map(|p| &p.args));
        for ty in tys {
            if let Ty::Struct { of, .. } = ty {
                if (of.pkg, of.schema) != (pkg, schema) {
                    let path = if of.pkg == pkg { format!("s{}", of.schema) } else { format!("p{}::s{}", of.pkg, of.schema) };
                    uses.insert(format!("use {path}::{}\n", self.type_name(*of)));
                }
            }
        }
        let mut out: String = uses.into_iter().collect();
        if !out.is_empty() {
            out.push('\n');
        }

        for (idx, st) in s.structs.iter().enumerate() {
            if st.max_depth {
                out += "@max_depth = 4\n";
            }
            out += &format!("struct {} {{\n", self.type_name(TypeId { pkg, schema, idx }));
            for (f, ty) in st.fields.iter().enumerate() {
                out += &format!("    f{f}: {}\n", ty_text(self, ty));
            }
            out += "}\n\n";
        }

        for (idx, p) in s.protocols.iter().enumerate() {
            if let Some(h) = handshake_text(p.handshake) {
                out += &format!("@handshake = {h}\n");
            }
            match &p.marker {
                None => {}
                Some(Ack::Plain) => out += &format!("@unverified = \"{REASON}\"\n"),
                Some(Ack::Keyed(keys)) => {
                    let each: Vec<String> = keys.iter().map(|k| format!("{} = \"{REASON}\"", key_text(k))).collect();
                    out += &format!("@unverified = {{ {} }}\n", each.join(", "));
                }
            }
            let args: Vec<String> =
                p.args.iter().enumerate().map(|(a, ty)| format!("a{a}: {}", ty_text(self, ty))).collect();
            out += &format!(
                "protocol {} {{\n    function f({});\n}}\n\n",
                self.protocol_name(ProtocolId { pkg, schema, idx }),
                args.join(", ")
            );
        }
        out
    }
}
