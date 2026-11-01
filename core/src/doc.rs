//! The documentation model: everything a renderer needs to document a
//! package, as one versioned, serializable value (core#145).
//!
//! [`model`] builds a [`DocModel`] from a compiled [`ProjectContext`]: the
//! package's name, version and `//!` header, each of its modules with its own
//! `//!` header, every unit declared in them with its `///` docs, and each
//! protocol's wire summary from [`crate::inspect`]. [`changelog`] adds what
//! changed between two frozen versions, from the same analysis `comline diff`
//! reports ([`analyze_package_changes`]).
//!
//! The model is the one contract between core and renderers. Its JSON
//! Schema ([`json_schema`]) is committed as `schemas/doc-model.json` and a
//! test fails when it drifts. Theming belongs to each renderer: nothing here
//! describes how the model is presented.
//!
//! **Versioning** (core#158). An additive change keeps
//! [`DOC_MODEL_VERSION`]: a new unit, type or change kind, or a new field
//! that may be absent. So a reader must tolerate what it doesn't know: skip a
//! unit, type or changelog entry of an unknown `kind`, and ignore unknown
//! fields. A consumer validating strictly against `doc-model/1.json` should
//! expect the schema to grow. Only a change an existing reader would
//! misread bumps the version: a field removed, renamed or retyped, or a
//! meaning changed. The generator protocol follows the same rule.
//!
//! Everything a package declares is documented: there is no way to hide a
//! unit. Only the package's own modules are listed; a type from a
//! dependency or `std` appears as a [`UnitRef`] to it.
//!
//! **Where the docs come from.** A unit's `///` docstring is in the frozen IR,
//! so it's there for any built version. The `//!` headers are frozen too, as
//! [`FrozenDocs`]: a build stores them in its commit's root tree as a `docs`
//! blob, next to `config`. [`frozen_docs`] reads them from a compiled context
//! (a module's from the source the context keeps, the package's from
//! `config.idp` on disk, so a package compiled from memory has no package
//! header); [`read_docs`] reads them back from a built version, which
//! [`model_frozen`] documents. A version built before headers were stored has
//! none.

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::inspect::{self, ProtocolSummary};
use crate::package::build::cas::objects::{Blob, EntryMode, Tree};
use crate::package::build::cas::object_store::ObjectStore;
use crate::package::build::cas::storage::{from_cbor, to_cbor, Hash};
use crate::package::config::ir::context::{Origin, ProjectContext};
use crate::schema::idl::module_docs::module_docs;
use crate::schema::ir::diff::{analyze_package_changes, BreakingChange, Modification, NewFeature};
use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::{DeprecatedValue, FrozenUnit};
use crate::settings::{SettingsDict, SettingsValue};

// External Uses
use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};

/// The model's version, so a renderer can refuse a model it can't read.
/// Additive changes (new kinds, new fields that may be absent) keep it;
/// readers tolerate unknown kinds and fields. Only an incompatible change
/// bumps it.
pub const DOC_MODEL_VERSION: u32 = 1;

/// The JSON Schema's `$id`.
pub const SCHEMA_ID: &str = "https://comlineproject.github.io/docs/schemas/doc-model/1.json";

/// A package's documentation.
///
/// Version 1 grows additively: new unit, type and change kinds and new
/// fields that may be absent keep the version, so a reader skips kinds it
/// doesn't know and ignores unknown fields. Only an incompatible change
/// bumps `model_version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocModel {
    /// The model's version when it was produced. Additive changes keep it;
    /// only an incompatible one bumps it.
    pub model_version: u32,
    pub package: Package,
    /// The package's modules (schemas), in the order they compiled.
    pub modules: Vec<Module>,
    /// What changed, one entry per pair of versions compared. Empty unless
    /// one was asked for.
    #[serde(default)]
    pub changelog: Vec<Changelog>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Package {
    pub name: String,
    /// The version it was built as, when the caller knows it.
    pub version: Option<String>,
    /// The `//!` header of its `config.idp`.
    pub docs: Option<String>,
}

/// One schema file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Module {
    /// Its path within the package: `api`, `types::user`.
    pub path: String,
    /// Its `//!` header.
    pub docs: Option<String>,
    /// What it declares, in declaration order.
    pub units: Vec<Unit>,
}

/// A `///` docstring: its description and its `/// @name: ...` lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Doc {
    /// Every line that isn't an `@name:` line, joined with newlines.
    pub text: String,
    /// The `/// @name: description` lines, in order: a field or an argument
    /// each.
    pub params: Vec<DocParam>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocParam {
    pub name: String,
    pub text: String,
}

/// One `@key` or `@key = value` annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Annotation {
    pub key: String,
    /// The value as written; `None` for a bare `@key`.
    pub value: Option<String>,
}

/// A declared unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unit {
    Struct {
        name: String,
        docs: Option<Doc>,
        annotations: Vec<Annotation>,
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        docs: Option<Doc>,
        variants: Vec<String>,
    },
    Protocol {
        name: String,
        docs: Option<Doc>,
        annotations: Vec<Annotation>,
        functions: Vec<Function>,
        /// What it resolves to on the wire ([`crate::inspect`]).
        wire: Option<ProtocolSummary>,
    },
    Error {
        name: String,
        docs: Option<Doc>,
        annotations: Vec<Annotation>,
        /// Its slot in the schema's error space.
        ordinal: u16,
        /// The message template, `{path}` placeholders intact.
        message: String,
        fields: Vec<Field>,
    },
    Validator {
        name: String,
        docs: Option<Doc>,
        annotations: Vec<Annotation>,
        /// The types it applies to (after `for`).
        targets: Vec<Type>,
        /// Its parameters.
        params: Vec<Field>,
        asserts: Vec<Assert>,
    },
    Constant {
        name: String,
        docs: Option<Doc>,
        #[serde(rename = "type")]
        ty: Type,
        /// The value as written.
        value: Option<String>,
    },
    TypeAlias {
        name: String,
        docs: Option<Doc>,
        annotations: Vec<Annotation>,
        target: Type,
    },
    Settings {
        /// `None` for the schema's unnamed block.
        name: Option<String>,
        docs: Option<Doc>,
        /// Each leaf by its dotted key, in key order.
        values: Vec<SettingsEntry>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Field {
    pub name: String,
    pub docs: Option<Doc>,
    pub annotations: Vec<Annotation>,
    #[serde(rename = "type")]
    pub ty: Type,
    pub optional: bool,
    /// `= value`, as written.
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Function {
    pub name: String,
    pub docs: Option<Doc>,
    pub annotations: Vec<Annotation>,
    pub arguments: Vec<Argument>,
    /// `None` when the function has no reply at all; [`Type::Unit`] for `()`.
    pub returns: Option<Type>,
    /// The errors it declares with `!`, each a reference to the error.
    pub throws: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Argument {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Type,
}

/// One `assert(condition, message)` of a validator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Assert {
    pub condition: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsEntry {
    pub key: String,
    pub value: String,
}

/// A type as written, with every name in it resolved to the unit it names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Type {
    /// `u32`, `string`, ...
    Primitive { name: String },
    /// A struct, enum, error or alias, by the name written (`User`,
    /// `types::User`), and the unit it resolves to; `None` when it doesn't
    /// resolve to a compiled unit.
    Named {
        name: String,
        target: Option<UnitRef>,
    },
    /// `T[]` (`length: None`) or `T[N]`.
    Array {
        element: Box<Type>,
        length: Option<u64>,
    },
    /// `union(A B ...)`.
    Union { members: Vec<Type> },
    /// `()`.
    Unit,
}

/// Where a unit is declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnitRef {
    /// The package: the documented one, a dependency (by the name the
    /// documented package gives it) or `std`.
    pub package: String,
    /// The module within that package (`api`, `validators`).
    pub schema: String,
    pub name: String,
}

/// What changed from one version to another, as `comline diff` reports it.
///
/// Names in it are as the declarations write them, not qualified by
/// module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Changelog {
    pub from: String,
    pub to: String,
    /// Changes an older peer can't work with: a major bump.
    pub breaking: Vec<BreakingChange>,
    /// Additions: a minor bump.
    pub features: Vec<NewFeature>,
    /// The rest: a patch bump.
    pub modifications: Vec<Modification>,
}

/// What changed between two versions of a package, each given as its frozen
/// schemas (a commit's, loaded with
/// [`crate::schema::ir::frozen::cas::blob::load_schema_from_tree`], or a
/// compiled context's). `from` and `to` label them.
pub fn changelog(
    from: &str,
    old_schemas: &[Vec<FrozenUnit>],
    to: &str,
    new_schemas: &[Vec<FrozenUnit>],
) -> Changelog {
    let changes = analyze_package_changes(old_schemas, new_schemas);
    Changelog {
        from: from.to_string(),
        to: to.to_string(),
        breaking: changes.breaking_changes,
        features: changes.new_features,
        modifications: changes.modifications,
    }
}

/// The `//!` headers of a package version: what a build freezes next to its
/// schemas, so documenting a built version has them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenDocs {
    /// The `//!` header of `config.idp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Each own module's `//!` header, by its path (`api`, `types::user`).
    /// A module without one isn't listed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modules: BTreeMap<String, String>,
}

impl FrozenDocs {
    pub fn is_empty(&self) -> bool {
        self.package.is_none() && self.modules.is_empty()
    }
}

/// The root tree entry a build stores [`FrozenDocs`] under.
pub const DOCS_ENTRY: &str = "docs";

/// The `//!` headers of the package `context` compiled.
pub fn frozen_docs(context: &ProjectContext) -> FrozenDocs {
    let index = Index::of(context);
    let package = match &context.origin {
        Origin::Disk(path) => std::fs::read_to_string(path)
            .ok()
            .and_then(|s| module_docs(&s)),
        Origin::Virtual | Origin::Official => None,
    };
    let modules = context
        .schema_contexts
        .iter()
        .filter_map(|schema| {
            let schema = schema.borrow();
            if !index.own(&schema.namespace) {
                return None;
            }
            let docs = schema.codemap.files().first().and_then(|f| module_docs(f.contents()))?;
            Some((schema.namespace.join("::"), docs))
        })
        .collect();
    FrozenDocs { package, modules }
}

/// Store `docs` as a blob, for a root tree's [`DOCS_ENTRY`].
pub fn write_docs(docs: &FrozenDocs, store: &ObjectStore) -> eyre::Result<Hash> {
    store.write(&Blob::new(to_cbor(docs)?).to_bytes()?)
}

/// The headers a built version's root tree froze; empty for a version built
/// before they were stored, or for a package without any.
pub fn read_docs(store: &ObjectStore, root: &Tree) -> eyre::Result<FrozenDocs> {
    let Some(entry) = root
        .entries
        .iter()
        .find(|e| e.name == DOCS_ENTRY && e.mode == EntryMode::Blob)
    else {
        return Ok(FrozenDocs::default());
    };
    let blob = Blob::from_bytes(&store.read(&entry.hash)?)?;
    from_cbor(&blob.content)
}

/// The frozen schemas `context` compiled for its own package, for
/// [`changelog`].
pub fn frozen_schemas(context: &ProjectContext) -> Vec<Vec<FrozenUnit>> {
    let index = Index::of(context);
    context
        .schema_contexts
        .iter()
        .filter(|schema| index.own(&schema.borrow().namespace))
        .filter_map(|schema| schema.borrow().frozen_schema.borrow().clone())
        .collect()
}

/// The documentation of the package `context` compiled, as `version`.
///
/// A version isn't part of what compiles: the CAS gives one to each build
/// (`BuildResult::current_version`, a commit's `version`), so the caller
/// passes it.
pub fn model(context: &ProjectContext, version: Option<String>) -> DocModel {
    let mut headers = frozen_docs(context);
    let package = Package {
        name: context.config.name.value.to_string(),
        version,
        docs: headers.package.take(),
    };

    let index = Index::of(context);
    let mut protocols = inspect::protocols(context)
        .into_iter()
        .filter(|p| !p.dependency)
        .collect::<Vec<_>>();

    let mut modules = vec![];
    for schema in &context.schema_contexts {
        let schema = schema.borrow();
        if !index.own(&schema.namespace) {
            continue;
        }
        let frozen = schema.frozen_schema.borrow();
        let units = frozen.as_deref().unwrap_or(&[]);
        let path = schema.namespace.join("::");
        let scope = Scope {
            index: &index,
            namespace: path.clone(),
            units,
        };
        let units = units
            .iter()
            .filter_map(|unit| {
                let mut unit = scope.unit(unit)?;
                if let Unit::Protocol { name, wire, .. } = &mut unit {
                    let at = protocols
                        .iter()
                        .position(|p| p.schema == path && &p.name == name);
                    *wire = at.map(|at| protocols.remove(at));
                }
                Some(unit)
            })
            .collect();
        modules.push(Module {
            docs: headers.modules.remove(&path),
            path,
            units,
        });
    }

    DocModel {
        model_version: DOC_MODEL_VERSION,
        package,
        modules,
        changelog: vec![],
    }
}

/// The documentation of a built version of package `name`: its own frozen
/// schemas (each starting with its `Namespace` unit, as
/// [`crate::schema::ir::frozen::cas::blob::load_schema_from_tree`] returns
/// them) and the headers [`read_docs`] read from it.
///
/// A built version keeps no source, so two things are missing next to
/// [`model`]: protocols have no wire summary (`wire` is `None`), and a type
/// from a dependency has no target (its schemas aren't the package's).
pub fn model_frozen(
    name: &str,
    version: Option<String>,
    schemas: &[Vec<FrozenUnit>],
    mut headers: FrozenDocs,
) -> DocModel {
    let namespace = |units: &[FrozenUnit]| {
        units.iter().find_map(|u| match u {
            FrozenUnit::Namespace(ns) => Some(ns.clone()),
            _ => None,
        })
    };
    let index = Index {
        own_name: name.to_string(),
        dependencies: vec![],
        declared: schemas
            .iter()
            .filter_map(|units| {
                Some((namespace(units)?, units.iter().filter_map(declared_name).collect()))
            })
            .collect(),
    };
    let modules = schemas
        .iter()
        .filter_map(|units| {
            let path = namespace(units)?;
            let scope = Scope {
                index: &index,
                namespace: path.clone(),
                units,
            };
            Some(Module {
                docs: headers.modules.remove(&path),
                units: units.iter().filter_map(|u| scope.unit(u)).collect(),
                path,
            })
        })
        .collect();
    DocModel {
        model_version: DOC_MODEL_VERSION,
        package: Package {
            name: name.to_string(),
            version,
            docs: headers.package,
        },
        modules,
        changelog: vec![],
    }
}

/// The model's JSON Schema (draft-07), as committed in
/// `schemas/doc-model.json`.
pub fn json_schema() -> serde_json::Value {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<DocModel>();
    let mut value = schema.to_value();
    let object = value.as_object_mut().expect("a root schema is an object");
    object.insert("$id".into(), SCHEMA_ID.into());
    object.insert("title".into(), "Comline documentation model".into());
    value
}

/// Split a docstring into its description and its `@name:` lines.
pub fn parse_docstring(docstring: &str) -> Doc {
    let mut text = vec![];
    let mut params = vec![];
    for line in docstring.lines() {
        let param = line.trim_start().strip_prefix('@').and_then(|rest| {
            let (name, desc) = rest.split_once(':')?;
            let is_name =
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            is_name.then(|| DocParam {
                name: name.to_string(),
                text: desc.trim().to_string(),
            })
        });
        match param {
            Some(param) => params.push(param),
            None => text.push(line),
        }
    }
    let text = text.join("\n").trim().to_string();
    Doc { text, params }
}

fn doc(docstring: &Option<String>) -> Option<Doc> {
    docstring
        .as_deref()
        .filter(|d| !d.trim().is_empty())
        .map(parse_docstring)
}

/// Every compiled unit name, by schema namespace, and which namespaces are
/// the package's own.
struct Index {
    own_name: String,
    dependencies: Vec<String>,
    /// Namespace (`api`, `billing::api`, `std::validators`) to the names it
    /// declares.
    declared: BTreeMap<String, Vec<String>>,
}

impl Index {
    fn of(context: &ProjectContext) -> Self {
        let mut declared = BTreeMap::new();
        for schema in &context.schema_contexts {
            let schema = schema.borrow();
            let frozen = schema.frozen_schema.borrow();
            let names = frozen
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter_map(declared_name)
                .collect();
            declared.insert(schema.namespace.join("::"), names);
        }
        Index {
            own_name: context.config.name.value.to_string(),
            dependencies: context
                .dependency_configs
                .iter()
                .map(|(n, _)| n.clone())
                .collect(),
            declared,
        }
    }

    /// The package a namespace belongs to, and the module within it.
    fn split<'a>(&'a self, namespace: &'a [String]) -> (&'a str, String) {
        match namespace {
            [first, rest @ ..]
                if !rest.is_empty() && (first == "std" || self.dependencies.contains(first)) =>
            {
                (first, rest.join("::"))
            }
            _ => (&self.own_name, namespace.join("::")),
        }
    }

    fn own(&self, namespace: &[String]) -> bool {
        self.split(namespace).0 == self.own_name
    }

    /// The unit `name` in the namespace `namespace`, if it declares one.
    fn find(&self, namespace: &str, name: &str) -> Option<UnitRef> {
        let names = self.declared.get(namespace)?;
        if !names.iter().any(|n| n == name) {
            return None;
        }
        let segments: Vec<String> = namespace.split("::").map(str::to_string).collect();
        let (package, schema) = self.split(&segments);
        Some(UnitRef {
            package: package.to_string(),
            schema,
            name: name.to_string(),
        })
    }

    /// A `::` path (`types::User`, `app::types::User`, `std::x::Y`).
    fn path(&self, path: &str) -> Option<UnitRef> {
        let (namespace, name) = path.rsplit_once("::")?;
        self.find(namespace, name).or_else(|| {
            let local = namespace.strip_prefix(&self.own_name)?.strip_prefix("::")?;
            self.find(local, name)
        })
    }
}

fn declared_name(unit: &FrozenUnit) -> Option<String> {
    match unit {
        FrozenUnit::Struct { name, .. }
        | FrozenUnit::Enum { name, .. }
        | FrozenUnit::Protocol { name, .. }
        | FrozenUnit::Validator { name, .. }
        | FrozenUnit::Constant { name, .. }
        | FrozenUnit::TypeAlias { name, .. } => Some(name.clone()),
        FrozenUnit::Error {
            name,
            imported_from: None,
            ..
        } => Some(name.clone()),
        _ => None,
    }
}

/// One schema, while its units are documented.
struct Scope<'a> {
    index: &'a Index,
    /// The schema's namespace, `::`-joined.
    namespace: String,
    units: &'a [FrozenUnit],
}

impl Scope<'_> {
    /// What `name`, written in this schema, names.
    fn resolve(&self, name: &str) -> Option<UnitRef> {
        if name.contains("::") {
            return self.index.path(name);
        }
        if let Some(found) = self.index.find(&self.namespace, name) {
            return Some(found);
        }
        self.units.iter().find_map(|unit| {
            let FrozenUnit::Import(path, alias, _) = unit else {
                return None;
            };
            if let Some(glob) = path.strip_suffix("::*") {
                return self
                    .index
                    .find(glob, name)
                    .or_else(|| self.index.path(&format!("{glob}::{name}")));
            }
            let (_, last) = path.rsplit_once("::").unwrap_or(("", path));
            if alias.as_deref().unwrap_or(last) != name {
                return None;
            }
            self.index.path(path)
        })
    }

    fn ty(&self, ty: &TypeRef) -> Type {
        match ty {
            TypeRef::Primitive(p) => Type::Primitive {
                name: p.to_string(),
            },
            TypeRef::Named(name) => Type::Named {
                name: name.clone(),
                target: self.resolve(name),
            },
            TypeRef::Array { element, length } => Type::Array {
                element: Box::new(self.ty(element)),
                length: *length,
            },
            TypeRef::Union(members) => Type::Union {
                members: members.iter().map(|m| self.ty(m)).collect(),
            },
            TypeRef::Unit => Type::Unit,
        }
    }

    fn field(&self, unit: &FrozenUnit) -> Option<Field> {
        let FrozenUnit::Field {
            docstring,
            parameters,
            optional,
            name,
            ty,
            default,
            ..
        } = unit
        else {
            return None;
        };
        Some(Field {
            name: name.clone(),
            docs: doc(docstring),
            annotations: annotations(parameters),
            ty: self.ty(ty),
            optional: *optional,
            default: default.as_ref().map(ToString::to_string),
        })
    }

    fn fields(&self, units: &[FrozenUnit]) -> Vec<Field> {
        units.iter().filter_map(|u| self.field(u)).collect()
    }

    /// The error at `ordinal` in this schema's error space.
    fn thrown(&self, ordinal: u16) -> Type {
        let error = self.units.iter().find_map(|u| match u {
            FrozenUnit::Error {
                ordinal: o,
                name,
                imported_from,
                ..
            } if *o == ordinal => Some((name, imported_from)),
            _ => None,
        });
        match error {
            Some((name, None)) => Type::Named {
                name: name.clone(),
                target: self.resolve(name),
            },
            Some((name, Some(from))) => Type::Named {
                name: name.clone(),
                target: self.index.find(from, name).or_else(|| self.resolve(name)),
            },
            None => Type::Named {
                name: format!("#{ordinal}"),
                target: None,
            },
        }
    }

    fn unit(&self, unit: &FrozenUnit) -> Option<Unit> {
        Some(match unit {
            FrozenUnit::Struct {
                docstring,
                parameters,
                name,
                fields,
                ..
            } => Unit::Struct {
                name: name.clone(),
                docs: doc(docstring),
                annotations: annotations(parameters),
                fields: self.fields(fields),
            },
            FrozenUnit::Enum {
                docstring,
                name,
                variants,
                ..
            } => Unit::Enum {
                name: name.clone(),
                docs: doc(docstring),
                variants: variants
                    .iter()
                    .filter_map(|v| match v {
                        FrozenUnit::EnumVariant(name, _) => Some(name.clone()),
                        _ => None,
                    })
                    .collect(),
            },
            FrozenUnit::Protocol {
                docstring,
                parameters,
                name,
                functions,
                ..
            } => Unit::Protocol {
                name: name.clone(),
                docs: doc(&Some(docstring.clone())),
                annotations: annotations(parameters),
                functions: functions
                    .iter()
                    .filter_map(|f| {
                        let FrozenUnit::Function {
                            docstring,
                            parameters,
                            name,
                            arguments,
                            _return,
                            throws,
                            ..
                        } = f
                        else {
                            return None;
                        };
                        Some(Function {
                            name: name.clone(),
                            docs: doc(&Some(docstring.clone())),
                            annotations: annotations(parameters),
                            arguments: arguments
                                .iter()
                                .map(|a| Argument {
                                    name: a.name.clone(),
                                    ty: self.ty(&a.ty),
                                })
                                .collect(),
                            returns: _return.as_ref().map(|t| self.ty(t)),
                            throws: throws.iter().map(|o| self.thrown(*o)).collect(),
                        })
                    })
                    .collect(),
                wire: None,
            },
            FrozenUnit::Error {
                docstring,
                parameters,
                ordinal,
                imported_from: None,
                name,
                message,
                fields,
                ..
            } => Unit::Error {
                name: name.clone(),
                docs: doc(docstring),
                annotations: annotations(parameters),
                ordinal: *ordinal,
                message: message.clone(),
                fields: self.fields(fields),
            },
            FrozenUnit::Validator {
                docstring,
                properties,
                name,
                targets,
                expression_block,
                ..
            } => Unit::Validator {
                name: name.clone(),
                docs: doc(docstring),
                annotations: annotations(properties),
                targets: targets.iter().map(|t| self.ty(t)).collect(),
                params: self.fields(properties),
                asserts: match expression_block.as_ref() {
                    FrozenUnit::ExpressionBlock { asserts } => asserts
                        .iter()
                        .filter_map(|a| match a {
                            FrozenUnit::Assert {
                                condition, message, ..
                            } => Some(Assert {
                                condition: condition.clone(),
                                message: message.clone(),
                            }),
                            _ => None,
                        })
                        .collect(),
                    _ => vec![],
                },
            },
            FrozenUnit::Constant {
                docstring,
                name,
                ty,
                value,
                ..
            } => Unit::Constant {
                name: name.clone(),
                docs: doc(docstring),
                ty: self.ty(ty),
                value: value.as_ref().map(ToString::to_string),
            },
            FrozenUnit::TypeAlias {
                docstring,
                parameters,
                name,
                target,
                ..
            } => Unit::TypeAlias {
                name: name.clone(),
                docs: doc(docstring),
                annotations: annotations(parameters),
                target: self.ty(target),
            },
            FrozenUnit::Settings {
                docstring,
                name,
                values,
                ..
            } => {
                let mut entries = vec![];
                flatten("", values, &mut entries);
                Unit::Settings {
                    name: name.clone(),
                    docs: doc(docstring),
                    values: entries,
                }
            }
            _ => return None,
        })
    }
}

fn flatten(prefix: &str, dict: &SettingsDict, out: &mut Vec<SettingsEntry>) {
    for (key, value) in &dict.0 {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            SettingsValue::Dict(inner) => flatten(&key, inner, out),
            value => out.push(SettingsEntry {
                key,
                value: value.to_string(),
            }),
        }
    }
}

/// A unit's annotations, as written. `@validators = [...]` is one entry per
/// validator; what `@unverified` gives up is in the protocol's `wire`.
fn annotations(parameters: &[FrozenUnit]) -> Vec<Annotation> {
    parameters
        .iter()
        .filter_map(|p| match p {
            FrozenUnit::Property { name, value } => Some(Annotation {
                key: name.clone(),
                value: (!value.is_marker()).then(|| value.source_text()),
            }),
            FrozenUnit::Deprecated(value) => Some(Annotation {
                key: "deprecated".to_string(),
                value: match value {
                    DeprecatedValue::False => Some("false".to_string()),
                    DeprecatedValue::True => None,
                    DeprecatedValue::Message(m) => Some(format!("{m:?}")),
                    DeprecatedValue::Malformed(m) => Some(m.clone()),
                },
            }),
            FrozenUnit::ValidatorRef { name, args, .. } => {
                let args: Vec<String> = args
                    .iter()
                    .filter_map(|a| match a {
                        FrozenUnit::Property { name, value } => {
                            Some(format!("{name} = {}", value.source_text()))
                        }
                        _ => None,
                    })
                    .collect();
                Some(Annotation {
                    key: "validators".to_string(),
                    value: Some(format!("{name}({})", args.join(", "))),
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    const CONFIG: &str = "//! A small package for testing the documentation model.\n\
        //!\n//! Second paragraph.\ncongregation shop\nspecification_version = 1\n";

    const TYPES: &str = "//! Shared types.\n\n\
        /// Who buys.\n\
        /// @id: unique per shop\n\
        struct Customer {\n    /// The customer's id.\n    id: u64\n    @deprecated = \"use email\"\n    name: string\n    optional email: string\n}\n\n\
        /// How an order is paid.\nenum Payment {\n    Card\n    Cash\n}\n\n\
        /// The longest name.\nconst MAX_NAME: u32 = 64\n\n\
        type CustomerId = u64\n";

    const API: &str = "//! The shop's API.\n\n\
        use types::Customer\n\
        use types::Payment\n\n\
        /// Not in stock.\nerror OutOfStock {\n    message = f\"{item} is out of stock\"\n    item: string\n}\n\n\
        /// One order.\nstruct Order {\n    customer: Customer\n    payments: Payment[]\n    lines: union(u32 string)\n}\n\n\
        /// Orders things.\nprotocol Orders {\n    \
        /// Place an order.\n    /// @order: what to place\n    \
        function place(order: Order) -> u64 ! OutOfStock;\n    \
        function ping();\n}\n";

    fn package(dir: &Path, types: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("config.idp"), CONFIG).unwrap();
        std::fs::write(dir.join("src/types.ids"), types).unwrap();
        std::fs::write(dir.join("src/api.ids"), API).unwrap();
        dir.to_path_buf()
    }

    fn unit<'a>(model: &'a DocModel, module: &str, name: &str) -> &'a Unit {
        let module = model.modules.iter().find(|m| m.path == module).unwrap();
        module
            .units
            .iter()
            .find(|u| match u {
                Unit::Struct { name: n, .. }
                | Unit::Enum { name: n, .. }
                | Unit::Protocol { name: n, .. }
                | Unit::Error { name: n, .. }
                | Unit::Validator { name: n, .. }
                | Unit::Constant { name: n, .. }
                | Unit::TypeAlias { name: n, .. } => n == name,
                Unit::Settings { name: n, .. } => n.as_deref() == Some(name),
            })
            .unwrap_or_else(|| panic!("no `{name}` in {module:?}"))
    }

    fn named(name: &str, schema: &str) -> Type {
        Type::Named {
            name: name.to_string(),
            target: Some(UnitRef {
                package: "shop".into(),
                schema: schema.into(),
                name: name.into(),
            }),
        }
    }

    #[test]
    fn documents_the_package_its_modules_and_units() {
        let tmp = tempfile::tempdir().unwrap();
        let context =
            crate::package::build::compile_package(&package(&tmp.path().join("shop"), TYPES))
                .unwrap();
        let model = model(&context, Some("0.1.0".into()));

        assert_eq!(model.model_version, DOC_MODEL_VERSION);
        assert_eq!(model.package.name, "shop");
        assert_eq!(model.package.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            model.package.docs.as_deref(),
            Some("A small package for testing the documentation model.\n\nSecond paragraph.")
        );
        let mut paths: Vec<&str> = model.modules.iter().map(|m| m.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["api", "types"], "std isn't the package's");
        let types = model.modules.iter().find(|m| m.path == "types").unwrap();
        assert_eq!(types.docs.as_deref(), Some("Shared types."));

        let Unit::Struct { docs, fields, .. } = unit(&model, "types", "Customer") else {
            panic!()
        };
        let docs = docs.as_ref().unwrap();
        assert_eq!(docs.text, "Who buys.");
        assert_eq!(
            docs.params,
            [DocParam {
                name: "id".into(),
                text: "unique per shop".into()
            }]
        );
        assert_eq!(fields[0].docs.as_ref().unwrap().text, "The customer's id.");
        assert_eq!(fields[0].ty, Type::Primitive { name: "u64".into() });
        assert_eq!(
            fields[1].annotations,
            [Annotation {
                key: "deprecated".into(),
                value: Some("\"use email\"".into())
            }]
        );
        assert!(fields[2].optional);

        let Unit::Enum { variants, .. } = unit(&model, "types", "Payment") else {
            panic!()
        };
        assert_eq!(variants, &["Card", "Cash"]);
        let Unit::Constant { ty, value, .. } = unit(&model, "types", "MAX_NAME") else {
            panic!()
        };
        assert_eq!(
            (ty, value.as_deref()),
            (&Type::Primitive { name: "u32".into() }, Some("64"))
        );
        assert!(matches!(
            unit(&model, "types", "CustomerId"),
            Unit::TypeAlias { .. }
        ));

        // A name from another module carries where it's declared.
        let Unit::Struct { fields, .. } = unit(&model, "api", "Order") else {
            panic!()
        };
        assert_eq!(fields[0].ty, named("Customer", "types"));
        assert_eq!(
            fields[1].ty,
            Type::Array {
                element: Box::new(named("Payment", "types")),
                length: None
            }
        );
        assert!(matches!(&fields[2].ty, Type::Union { members } if members.len() == 2));

        let Unit::Error {
            message,
            ordinal,
            fields,
            ..
        } = unit(&model, "api", "OutOfStock")
        else {
            panic!()
        };
        assert_eq!(
            (message.as_str(), *ordinal, fields.len()),
            ("{item} is out of stock", 0, 1)
        );

        let Unit::Protocol {
            docs,
            functions,
            wire,
            ..
        } = unit(&model, "api", "Orders")
        else {
            panic!()
        };
        assert_eq!(docs.as_ref().unwrap().text, "Orders things.");
        let [place, ping] = functions.as_slice() else {
            panic!()
        };
        let place_docs = place.docs.as_ref().unwrap();
        assert_eq!(place_docs.text, "Place an order.");
        assert_eq!(
            place_docs.params,
            [DocParam {
                name: "order".into(),
                text: "what to place".into()
            }]
        );
        assert_eq!(place.arguments[0].ty, named("Order", "api"));
        assert_eq!(place.returns, Some(Type::Primitive { name: "u64".into() }));
        assert_eq!(place.throws, [named("OutOfStock", "api")]);
        assert_eq!((ping.docs.as_ref(), ping.returns.as_ref()), (None, None));
        let wire = wire.as_ref().unwrap();
        assert_eq!(
            (wire.schema.as_str(), wire.name.as_str()),
            ("api", "Orders")
        );

        // It round-trips through JSON.
        let json = serde_json::to_value(&model).unwrap();
        assert!(json["modules"][0]["units"][0]["kind"].is_string());
        let back: DocModel = serde_json::from_value(json).unwrap();
        assert_eq!(back, model);
    }

    #[test]
    fn a_changelog_between_two_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let old = crate::package::build::compile_package(&package(&tmp.path().join("shop"), TYPES))
            .unwrap();
        let changed = TYPES
            .replace(
                "    optional email: string\n",
                "    optional email: string\n    optional phone: string\n",
            )
            .replace("    Cash\n", "");
        let new =
            crate::package::build::compile_package(&package(&tmp.path().join("shop2"), &changed))
                .unwrap();

        let log = changelog(
            "0.1.0",
            &frozen_schemas(&old),
            "0.2.0",
            &frozen_schemas(&new),
        );
        assert_eq!((log.from.as_str(), log.to.as_str()), ("0.1.0", "0.2.0"));
        // Under core's default format (`packed@1`: fields by position, an
        // unknown field rejected), an optional field is breaking too.
        assert_eq!(
            log.breaking,
            [
                BreakingChange::AddedOptionalField {
                    type_name: "Customer".into(),
                    field_name: "phone".into(),
                    field_type: "string".into(),
                },
                BreakingChange::RemovedEnumVariant {
                    enum_name: "Payment".into(),
                    variant: "Cash".into()
                },
            ]
        );
        assert!(!log.features.iter().any(|f| matches!(f, NewFeature::AddedField { .. })));

        let mut model = model(&new, Some("0.2.0".into()));
        model.changelog.push(log);
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json["changelog"][0]["breaking"][1]["kind"],
            "removed_enum_variant"
        );
        let back: DocModel = serde_json::from_value(json).unwrap();
        assert_eq!(back, model);
    }

    #[test]
    fn docstring_params() {
        let doc = parse_docstring(
            "Send it.\n@message: the message\nReturns whether it went.\n@ not a param",
        );
        assert_eq!(
            doc.text,
            "Send it.\nReturns whether it went.\n@ not a param"
        );
        assert_eq!(
            doc.params,
            [DocParam {
                name: "message".into(),
                text: "the message".into()
            }]
        );
    }

    fn schema_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas/doc-model.json")
    }

    /// `schemas/doc-model.json` is generated from these types. Regenerate it
    /// with `COMLINE_BLESS=1 cargo test -p comline-core --lib doc::tests`.
    #[test]
    fn the_committed_schema_matches_the_types() {
        let generated = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
        let path = schema_path();
        if std::env::var_os("COMLINE_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &generated).unwrap();
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == generated,
            "{} is out of date with comline_core::doc - regenerate it with \
             `COMLINE_BLESS=1 cargo test -p comline-core --lib doc::tests`",
            path.display()
        );
    }
}
