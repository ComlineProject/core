// Schema diffing and change analysis

use crate::schema::ir::frozen::types::{Literal, TypeRef};
use crate::schema::ir::frozen::unit::{
    schema_namespace, ConfigValue, DeprecatedValue, FrozenArgument, FrozenUnit, Guarantee,
};
use crate::schema::ir::unverified;
use crate::encodings::{official, EncodingKind, Profile};
use crate::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use crate::schema::idl::annotations::{
    FramingValue, DEFAULT_FORMAT, DEFAULT_FRAMING, FORMAT_JSON, FRAMING_JSONRPC,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};

/// Structured representation of schema changes between two versions
#[derive(Debug, Clone, Default)]
pub struct SchemaChanges {
    pub breaking_changes: Vec<BreakingChange>,
    pub new_features: Vec<NewFeature>,
    pub modifications: Vec<Modification>,
}

impl SchemaChanges {
    pub fn is_empty(&self) -> bool {
        self.breaking_changes.is_empty()
            && self.new_features.is_empty()
            && self.modifications.is_empty()
    }

    pub fn is_breaking(&self) -> bool {
        !self.breaking_changes.is_empty()
    }

    pub fn is_feature(&self) -> bool {
        !self.new_features.is_empty()
    }

    /// Append everything `other` found.
    pub fn extend(&mut self, other: SchemaChanges) {
        self.breaking_changes.extend(other.breaking_changes);
        self.new_features.extend(other.new_features);
        self.modifications.extend(other.modifications);
    }
}

/// The changes inside one dependency, between the version this package
/// built against last time and now.
#[derive(Debug, Clone)]
pub struct DependencyChanges {
    pub name: String,
    pub changes: SchemaChanges,
}

/// Breaking changes that require a major version bump
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BreakingChange {
    RemovedStruct {
        name: String,
    },
    RemovedEnum {
        name: String,
    },
    /// A field disappeared, optional or not: its name is gone for every
    /// consumer that reads it.
    RemovedField {
        type_name: String,
        field_name: String,
    },
    AddedRequiredField {
        type_name: String,
        field_name: String,
        field_type: String,
    },
    /// An optional field added where a format in play can't tolerate it: it
    /// keys fields by position, rejects an unknown field, or rejects a
    /// missing optional one (its profile's `struct_fields`,
    /// `unknown_field`, `missing_optional`).
    AddedOptionalField {
        type_name: String,
        field_name: String,
        field_type: String,
    },
    /// Fields kept their names but moved, where a format in play keys them
    /// by position (`struct_fields = by_position`).
    ReorderedFields {
        type_name: String,
    },
    ChangedFieldType {
        type_name: String,
        field_name: String,
        old_type: String,
        new_type: String,
    },
    /// `optional` flipped, either way. Made optional: old readers may now
    /// receive nothing where they expect a value. Made required: old writers
    /// may still leave it out.
    ChangedFieldOptionality {
        type_name: String,
        field_name: String,
        now_optional: bool,
    },
    RemovedEnumVariant {
        enum_name: String,
        variant: String,
    },
    /// One variant removed and one added at the same position: reported as
    /// the rename it most likely is, rather than a removal plus an addition.
    /// Still breaking, since variants travel by name.
    RenamedEnumVariant {
        enum_name: String,
        old_name: String,
        new_name: String,
    },
    /// Variants kept their names but moved (reordered, or one inserted
    /// before others), where a format in play encodes them by index
    /// (`enum_variants = by_index`).
    ReorderedEnumVariants {
        enum_name: String,
    },
    RemovedFunction {
        protocol_name: String,
        function_name: String,
    },
    /// Functions kept their names but moved (reordered, or one inserted
    /// before others), where the protocol's framing calls them by index
    /// (`calls = by_index`).
    ReorderedFunctions {
        protocol_name: String,
    },
    /// A protocol renamed: one removed and one added with the same
    /// functions. Its name doesn't reach the wire, but consumers refer to it.
    RenamedProtocol {
        old_name: String,
        new_name: String,
    },
    /// A schema file's namespace renamed: one removed and one added with
    /// the same declarations. Consumers refer to it in paths.
    RenamedNamespace {
        old_name: String,
        new_name: String,
    },
    ChangedFunctionSignature {
        protocol_name: String,
        function_name: String,
        details: String,
    },
    /// A function no longer declares an error it used to (`! Name`): the
    /// generated error variant disappears for its callers.
    RemovedThrownError {
        protocol_name: String,
        function_name: String,
        error: String,
    },
    RemovedProtocol {
        name: String,
    },
    RemovedError {
        name: String,
    },
    /// The error's ordinal is what travels on the wire, and it shifts when a
    /// local error is inserted, removed or reordered before it.
    ChangedErrorOrdinal {
        name: String,
        old_ordinal: u16,
        new_ordinal: u16,
    },
    RemovedConstant {
        name: String,
    },
    ChangedConstantType {
        name: String,
        old_type: String,
        new_type: String,
    },
    RemovedTypeAlias {
        name: String,
    },
    ChangedTypeAliasTarget {
        name: String,
        old_target: String,
        new_target: String,
    },
    RemovedValidator {
        name: String,
    },
    /// A validator declaration's targets, properties or body changed. What
    /// it accepts can't be compared in general, so any change counts as
    /// narrowing.
    ChangedValidator {
        name: String,
    },
    /// The validators applied to a declaration changed other than by pure
    /// removal: what's accepted may have narrowed. The set compared is the
    /// effective one: a field's own validators plus those of the aliases
    /// its type goes through. `target` may also be a constant whose value
    /// changed while a validator argument uses it.
    ValidationTightened {
        target: String,
    },
    /// An annotation that changes what travels on the wire (`@framing`).
    ChangedWireAnnotation {
        target: String,
        key: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
    /// A protocol gave up guarantees it used to keep (`@unverified`, or its
    /// set growing): peers relied on them.
    GaveUpGuarantees {
        target: String,
        #[serde(with = "guarantee_names")]
        #[schemars(with = "Vec<String>")]
        guarantees: Vec<Guarantee>,
    },
}

/// New features that require a minor version bump
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewFeature {
    AddedStruct {
        name: String,
        field_count: usize,
    },
    AddedEnum {
        name: String,
        variant_count: usize,
    },
    AddedField {
        type_name: String,
        field_name: String,
        field_type: String,
        optional: bool,
    },
    AddedEnumVariant {
        enum_name: String,
        variant: String,
    },
    AddedFunction {
        protocol_name: String,
        function_name: String,
        signature: String,
    },
    /// A function declares a new error (`! Name`). Older callers see it as
    /// a remote error they don't know.
    AddedThrownError {
        protocol_name: String,
        function_name: String,
        error: String,
    },
    AddedProtocol {
        name: String,
        function_count: usize,
    },
    AddedError {
        name: String,
        field_count: usize,
    },
    AddedConstant {
        name: String,
        ty: String,
    },
    /// A constant's value changed, and nothing on the wire uses it: no
    /// validator argument and no field default. (One a validator uses is
    /// [`BreakingChange::ValidationTightened`]; one only a default uses is
    /// [`Modification::ChangedConstantValue`].)
    ChangedConstantValue {
        name: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
    AddedTypeAlias {
        name: String,
        target: String,
    },
    AddedValidator {
        name: String,
    },
    /// Validators were only removed from a declaration: what's accepted
    /// widens.
    ValidationLoosened {
        target: String,
    },
    /// `@deprecated` was added to a declaration.
    Deprecated {
        target: String,
    },
    /// A protocol keeps guarantees it used to give up.
    RestoredGuarantees {
        target: String,
        #[serde(with = "guarantee_names")]
        #[schemars(with = "Vec<String>")]
        guarantees: Vec<Guarantee>,
    },
}

/// [`Guarantee`]s by their written names (`schema_agreement`), as
/// `comline inspect --json` writes them.
mod guarantee_names {
    use crate::schema::ir::frozen::unit::Guarantee;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(guarantees: &[Guarantee], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(guarantees.iter().map(|g| g.name()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Guarantee>, D::Error> {
        Vec::<String>::deserialize(d)?
            .into_iter()
            .map(|name| {
                Guarantee::from_name(&name)
                    .ok_or_else(|| serde::de::Error::custom(format!("`{name}` isn't a guarantee")))
            })
            .collect()
    }
}

/// Non-breaking modifications that may warrant a patch bump
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Modification {
    /// A docstring's text changed: a declaration's (`S`) or a member's
    /// (`S.field`, `P.function`). Documentation only: a patch.
    DocstringChanged {
        name: String,
    },
    /// A type respelled without changing what it is: swapped for an
    /// equivalent alias, either way, or one alias for another with the same
    /// target (compared after resolving aliases, core#110). `target` is the
    /// field (`S.f`), function (`P.f`), constant or alias.
    TypeRespelled {
        target: String,
        old_type: String,
        new_type: String,
    },
    /// A constant's value changed where only field defaults use it:
    /// defaults don't travel on the wire.
    ChangedConstantValue {
        name: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
    ChangedFieldDefault {
        type_name: String,
        field_name: String,
    },
    ChangedErrorMessage {
        name: String,
    },
    /// An annotation that doesn't change the wire changed: `@timeout_ms`,
    /// `@idempotent`, `@deprecated` removed or reworded, or one core doesn't
    /// know.
    ChangedAnnotation {
        target: String,
        key: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
}

/// What the encodings in play say about how things travel, from their
/// profiles' facts (`core_stdlib/packages/comline/profiles/`). The rules
/// that depend on the format or framing read these, never a list of
/// format names.
///
/// Each flag is the *lenient* reading and holds only when every encoding
/// in play states it, so [`WireFacts::default`] is the strictest: what an
/// unknown encoding (one with no profile the comparison can read) gets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireFacts {
    /// Every format keys struct fields by name (`struct_fields = by_name`).
    pub fields_by_name: bool,
    /// Every format ignores an unknown field and accepts a missing optional
    /// one (`unknown_field = ignored`, `missing_optional = absent`).
    pub optional_fields_tolerated: bool,
    /// Every format encodes enum variants by name (`enum_variants = by_name`).
    pub variants_by_name: bool,
    /// Every framing sends errors by name (`errors = by_name`), so an
    /// error's ordinal doesn't travel.
    pub errors_by_name: bool,
    /// Per protocol (`namespace::Protocol`): whether its framing calls
    /// functions by something stable (`calls = by_name` or `by_id`) rather
    /// than by their index. A protocol not listed is called by index.
    pub calls_stable: BTreeMap<String, bool>,
}

impl WireFacts {
    /// The facts of both versions of a package: each version's schemas and
    /// frozen config (its `wire` defaults; empty for none). Each protocol's
    /// framing is its frozen `@framing`, else the package's, else core's;
    /// its format the package's, else JSON under JSON-RPC, else core's (as
    /// `comline_core::inspect` resolves them). A package with no protocol
    /// has its default format and framing in play. Facts come from the
    /// official profiles; any other encoding reads as the strictest.
    pub fn of(
        old: (&[Vec<FrozenUnit>], &[ConfigUnit]),
        new: (&[Vec<FrozenUnit>], &[ConfigUnit]),
    ) -> Self {
        let mut formats: Vec<Option<&Profile>> = vec![];
        let mut framings: Vec<Option<&Profile>> = vec![];
        let mut calls: BTreeMap<String, Vec<Option<&Profile>>> = BTreeMap::new();
        for (schemas, config) in [old, new] {
            let (wire_format, wire_framing) = crate::package::config::ir::frozen::wire(config);
            let mut any = false;
            for units in schemas {
                let namespace = schema_namespace(units).unwrap_or("");
                for unit in units {
                    let FrozenUnit::Protocol { name, parameters, .. } = unit else { continue };
                    any = true;
                    let framing = written_framing(parameters)
                        .or(wire_framing)
                        .unwrap_or(DEFAULT_FRAMING);
                    let framing_profile = official::profile(EncodingKind::Framing, framing);
                    let jsonrpc = FramingValue::named(framing).is_some_and(|f| f.name == FRAMING_JSONRPC);
                    let format = wire_format
                        .unwrap_or(if jsonrpc { FORMAT_JSON } else { DEFAULT_FORMAT });
                    formats.push(official::profile(EncodingKind::Format, format));
                    framings.push(framing_profile);
                    calls.entry(protocol_key(namespace, name)).or_default().push(framing_profile);
                }
            }
            if !any {
                formats.push(official::profile(EncodingKind::Format, wire_format.unwrap_or(DEFAULT_FORMAT)));
                framings.push(official::profile(EncodingKind::Framing, wire_framing.unwrap_or(DEFAULT_FRAMING)));
            }
        }
        let all = |profiles: &[Option<&Profile>], fact: &str, values: &[&str]| {
            !profiles.is_empty()
                && profiles.iter().all(|p| {
                    p.and_then(|p| p.facts.get(fact)).is_some_and(|f| values.contains(&f.value.as_str()))
                })
        };
        WireFacts {
            fields_by_name: all(&formats, "struct_fields", &["by_name"]),
            optional_fields_tolerated: all(&formats, "unknown_field", &["ignored"])
                && all(&formats, "missing_optional", &["absent"]),
            variants_by_name: all(&formats, "enum_variants", &["by_name"]),
            errors_by_name: all(&framings, "errors", &["by_name"]),
            calls_stable: calls
                .into_iter()
                .map(|(key, profiles)| {
                    let stable = all(&profiles, "calls", &["by_name", "by_id"]);
                    (key, stable)
                })
                .collect(),
        }
    }

    fn calls_stable(&self, namespace: &str, protocol: &str) -> bool {
        self.calls_stable.get(&protocol_key(namespace, protocol)).copied().unwrap_or(false)
    }
}

fn protocol_key(namespace: &str, protocol: &str) -> String {
    if namespace.is_empty() {
        protocol.to_string()
    } else {
        format!("{namespace}::{protocol}")
    }
}

/// A protocol's `@framing` name as frozen, either form, unresolved.
fn written_framing(parameters: &[FrozenUnit]) -> Option<&str> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == "framing" => match value {
            ConfigValue::Str(v) => Some(v.as_str()),
            ConfigValue::Dict(_) => match value.entry("name") {
                Some(ConfigValue::Str(v)) => Some(v.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    })
}

/// What one version of the package is, for comparing types and constants
/// by what they mean rather than how they're spelled.
#[derive(Default)]
struct Side<'a> {
    /// Every `type` alias, by its bare and its qualified names.
    aliases: HashMap<String, &'a FrozenUnit>,
    /// Constants (by their last path segment) a validator argument or a
    /// validator's parameter default uses.
    in_validators: HashSet<String>,
    /// Constants a field default uses.
    in_defaults: HashSet<String>,
}

impl<'a> Side<'a> {
    fn of(schemas: &[&'a [FrozenUnit]]) -> Self {
        let mut side = Side::default();
        for units in schemas {
            let namespace = schema_namespace(units);
            for unit in units.iter() {
                if let FrozenUnit::TypeAlias { name, .. } = unit {
                    side.aliases.insert(name.clone(), unit);
                    if let Some(ns) = namespace {
                        side.aliases.insert(format!("{ns}::{name}"), unit);
                        if let Some((_, last)) = ns.rsplit_once("::") {
                            side.aliases.insert(format!("{last}::{name}"), unit);
                        }
                    }
                }
                side.collect_references(unit, false);
            }
        }
        side
    }

    fn collect_references(&mut self, unit: &FrozenUnit, in_validator: bool) {
        let all = |side: &mut Self, units: &[FrozenUnit], v: bool| {
            units.iter().for_each(|u| side.collect_references(u, v))
        };
        match unit {
            FrozenUnit::Struct { parameters, fields, .. } | FrozenUnit::Error { parameters, fields, .. } => {
                all(self, parameters, false);
                all(self, fields, false);
            }
            FrozenUnit::TypeAlias { parameters, .. } | FrozenUnit::Protocol { parameters, .. } => {
                all(self, parameters, false)
            }
            FrozenUnit::Field { parameters, default, .. } => {
                if let Some(Literal::Reference(r)) = default {
                    let last = last_segment(r).to_string();
                    if in_validator {
                        self.in_validators.insert(last);
                    } else {
                        self.in_defaults.insert(last);
                    }
                }
                all(self, parameters, in_validator);
            }
            FrozenUnit::Validator { properties, .. } => all(self, properties, true),
            FrozenUnit::ValidatorRef { args, .. } => all(self, args, true),
            FrozenUnit::Property { value, .. } if in_validator => self.collect_value(value),
            _ => {}
        }
    }

    fn collect_value(&mut self, value: &ConfigValue) {
        match value {
            ConfigValue::Name(n) | ConfigValue::Path(n) => {
                self.in_validators.insert(last_segment(n).to_string());
            }
            ConfigValue::Dict(entries) => entries.iter().for_each(|(_, v)| self.collect_value(v)),
            _ => {}
        }
    }

    fn alias(&self, name: &str) -> Option<&'a FrozenUnit> {
        self.aliases.get(name).copied()
    }

    /// `ty` with every alias replaced by what it stands for.
    fn resolve(&self, ty: &TypeRef) -> TypeRef {
        self.resolve_depth(ty, 0)
    }

    fn resolve_depth(&self, ty: &TypeRef, depth: usize) -> TypeRef {
        if depth > 32 {
            return ty.clone();
        }
        match ty {
            TypeRef::Named(name) => match self.alias(name) {
                Some(FrozenUnit::TypeAlias { target, .. }) => self.resolve_depth(target, depth + 1),
                _ => ty.clone(),
            },
            TypeRef::Array { element, length } => TypeRef::Array {
                element: Box::new(self.resolve_depth(element, depth + 1)),
                length: *length,
            },
            TypeRef::Union(types) => {
                TypeRef::Union(types.iter().map(|t| self.resolve_depth(t, depth + 1)).collect())
            }
            other => other.clone(),
        }
    }

    /// The validators the aliases `ty` goes through apply.
    fn alias_validators(&self, ty: &TypeRef) -> Vec<FrozenUnit> {
        let mut found = vec![];
        let mut ty = ty;
        for _ in 0..32 {
            let TypeRef::Named(name) = ty else { break };
            let Some(FrozenUnit::TypeAlias { target, parameters, .. }) = self.alias(name) else { break };
            found.extend(validator_refs(parameters));
            ty = target;
        }
        found
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit_once("::").map_or(path, |(_, last)| last)
}

/// Everything a comparison of one schema needs beyond its two versions.
struct Context<'a> {
    old: Side<'a>,
    new: Side<'a>,
    facts: &'a WireFacts,
    namespace: String,
}

/// Analyze changes between two versions of a package: its schema files,
/// paired by namespace, under the encodings their own protocols name (and
/// core's defaults). [`analyze_package_changes_with`] takes the facts of a
/// package's declared defaults too.
pub fn analyze_package_changes(
    old_schemas: &[Vec<FrozenUnit>],
    new_schemas: &[Vec<FrozenUnit>],
) -> SchemaChanges {
    let facts = WireFacts::of((old_schemas, &[]), (new_schemas, &[]));
    analyze_package_changes_with(old_schemas, new_schemas, &facts)
}

/// Analyze changes between two versions of a package, with the encodings'
/// facts given ([`WireFacts::of`]).
///
/// A file whose namespace exists only in `old` is compared against nothing,
/// so each of its declarations is a removal; one only in `new`, an addition.
/// Pairing by namespace rather than by position means adding, renaming or
/// reordering files doesn't pair the wrong schemas. One namespace gone and
/// one new holding the same declarations is a rename: reported as one, and
/// their contents compared.
pub fn analyze_package_changes_with(
    old_schemas: &[Vec<FrozenUnit>],
    new_schemas: &[Vec<FrozenUnit>],
    facts: &WireFacts,
) -> SchemaChanges {
    let old = by_namespace(old_schemas);
    let new = by_namespace(new_schemas);
    let old_all: Vec<&[FrozenUnit]> = old.values().copied().collect();
    let new_all: Vec<&[FrozenUnit]> = new.values().copied().collect();
    let context = |namespace: &str| Context {
        old: Side::of(&old_all),
        new: Side::of(&new_all),
        facts,
        namespace: namespace.to_string(),
    };

    let gone: Vec<&String> = old.keys().filter(|ns| !new.contains_key(*ns)).collect();
    let added: Vec<&String> = new.keys().filter(|ns| !old.contains_key(*ns)).collect();
    let renamed = match (gone.as_slice(), added.as_slice()) {
        ([from], [to]) => {
            let names = |units: &[FrozenUnit]| build_index(units).into_keys().collect::<BTreeSet<_>>();
            let (before, after) = (names(old[*from]), names(new[*to]));
            (!before.is_empty() && before == after).then(|| ((*from).clone(), (*to).clone()))
        }
        _ => None,
    };

    let mut changes = SchemaChanges::default();
    for (namespace, old_units) in &old {
        if let Some((from, to)) = renamed.as_ref().filter(|(from, _)| from == namespace) {
            changes.breaking_changes.push(BreakingChange::RenamedNamespace {
                old_name: from.clone(),
                new_name: to.clone(),
            });
            changes.extend(analyze(old_units, new[to], &context(to)));
            continue;
        }
        let new_units = new.get(namespace).copied().unwrap_or(&[]);
        changes.extend(analyze(old_units, new_units, &context(namespace)));
    }
    for (namespace, new_units) in &new {
        let is_rename_target = renamed.as_ref().is_some_and(|(_, to)| to == namespace);
        if !old.contains_key(namespace) && !is_rename_target {
            changes.extend(analyze(&[], new_units, &context(namespace)));
        }
    }
    changes
}

/// Each schema keyed by its namespace. One without a namespace keeps its
/// position, the best there is.
fn by_namespace(schemas: &[Vec<FrozenUnit>]) -> BTreeMap<String, &[FrozenUnit]> {
    schemas
        .iter()
        .enumerate()
        .map(|(i, units)| {
            let key = schema_namespace(units)
                .map(str::to_string)
                .unwrap_or_else(|| format!("#{i}"));
            (key, units.as_slice())
        })
        .collect()
}

/// Analyze changes between two versions of one schema, under the
/// encodings its own protocols name (and core's defaults).
pub fn analyze_schema_changes(
    old_schema: &[FrozenUnit],
    new_schema: &[FrozenUnit],
) -> SchemaChanges {
    let facts = WireFacts::of(
        (std::slice::from_ref(&old_schema.to_vec()), &[]),
        (std::slice::from_ref(&new_schema.to_vec()), &[]),
    );
    let context = Context {
        old: Side::of(&[old_schema]),
        new: Side::of(&[new_schema]),
        facts: &facts,
        namespace: schema_namespace(new_schema).or(schema_namespace(old_schema)).unwrap_or("").to_string(),
    };
    analyze(old_schema, new_schema, &context)
}

fn analyze(old_schema: &[FrozenUnit], new_schema: &[FrozenUnit], cx: &Context) -> SchemaChanges {
    let mut changes = SchemaChanges::default();

    // Build indices for efficient lookup
    let old_index = build_index(old_schema);
    let new_index = build_index(new_schema);

    // One protocol gone and one new with the same functions: a rename.
    let functions_of = |unit: &FrozenUnit| match unit {
        FrozenUnit::Protocol { functions, .. } => Some(
            functions
                .iter()
                .filter_map(|f| match f {
                    FrozenUnit::Function { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let gone: Vec<&String> = old_index
        .iter()
        .filter(|(n, u)| matches!(u, FrozenUnit::Protocol { .. }) && !new_index.contains_key(*n))
        .map(|(n, _)| n)
        .collect();
    let added: Vec<&String> = new_index
        .iter()
        .filter(|(n, u)| matches!(u, FrozenUnit::Protocol { .. }) && !old_index.contains_key(*n))
        .map(|(n, _)| n)
        .collect();
    let renamed_protocol: Option<(String, String)> = match (gone.as_slice(), added.as_slice()) {
        ([from], [to]) if functions_of(old_index[*from]) == functions_of(new_index[*to]) => {
            Some(((*from).clone(), (*to).clone()))
        }
        _ => None,
    };
    if let Some((from, to)) = &renamed_protocol {
        changes.breaking_changes.push(BreakingChange::RenamedProtocol {
            old_name: from.clone(),
            new_name: to.clone(),
        });
    }
    let renamed_from = |name: &str| renamed_protocol.as_ref().filter(|(_, to)| to == name).map(|(from, _)| from.as_str());
    let renamed_to = |name: &str| renamed_protocol.as_ref().is_some_and(|(from, _)| from == name);

    // Check for removed declarations (breaking)
    for (name, old_unit) in &old_index {
        if !new_index.contains_key(name) && !renamed_to(name) {
            let name = name.clone();
            let removed = match old_unit {
                FrozenUnit::Struct { .. } => Some(BreakingChange::RemovedStruct { name }),
                FrozenUnit::Enum { .. } => Some(BreakingChange::RemovedEnum { name }),
                FrozenUnit::Protocol { .. } => Some(BreakingChange::RemovedProtocol { name }),
                FrozenUnit::Error { .. } => Some(BreakingChange::RemovedError { name }),
                FrozenUnit::Constant { .. } => Some(BreakingChange::RemovedConstant { name }),
                FrozenUnit::TypeAlias { .. } => Some(BreakingChange::RemovedTypeAlias { name }),
                FrozenUnit::Validator { .. } => Some(BreakingChange::RemovedValidator { name }),
                _ => None,
            };
            changes.breaking_changes.extend(removed);
        }
    }

    // Check for added declarations (features)
    for (name, new_unit) in &new_index {
        if !old_index.contains_key(name) && renamed_from(name).is_none() {
            let name = name.clone();
            let added = match new_unit {
                FrozenUnit::Struct { fields, .. } => Some(NewFeature::AddedStruct {
                    name,
                    field_count: fields.len(),
                }),
                FrozenUnit::Enum { variants, .. } => Some(NewFeature::AddedEnum {
                    name,
                    variant_count: variants.len(),
                }),
                FrozenUnit::Protocol { functions, .. } => Some(NewFeature::AddedProtocol {
                    name,
                    function_count: functions.len(),
                }),
                FrozenUnit::Error { fields, .. } => Some(NewFeature::AddedError {
                    name,
                    field_count: fields.len(),
                }),
                FrozenUnit::Constant { ty, .. } => Some(NewFeature::AddedConstant {
                    name,
                    ty: ty.to_string(),
                }),
                FrozenUnit::TypeAlias { target, .. } => Some(NewFeature::AddedTypeAlias {
                    name,
                    target: target.to_string(),
                }),
                FrozenUnit::Validator { .. } => Some(NewFeature::AddedValidator { name }),
                _ => None,
            };
            changes.new_features.extend(added);
        }
    }

    // Error ordinal -> name, per side, so a function's `! Name` list (which
    // holds ordinals) is compared by name, not by numbers that shift.
    let old_errors = error_names(old_schema);
    let new_errors = error_names(new_schema);

    // Check for modifications to existing declarations
    for (name, new_unit) in &new_index {
        let Some(old_unit) = old_index.get(renamed_from(name).unwrap_or(name)) else {
            continue;
        };
        match (old_unit, new_unit) {
            (
                FrozenUnit::Struct {
                    docstring: old_docstring,
                    parameters: old_params,
                    fields: old_fields,
                    ..
                },
                FrozenUnit::Struct {
                    docstring: new_docstring,
                    parameters: new_params,
                    fields: new_fields,
                    ..
                },
            ) => {
                compare_struct_fields(name, old_fields, new_fields, cx, &mut changes);
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Enum {
                    docstring: old_docstring,
                    variants: old_variants,
                    ..
                },
                FrozenUnit::Enum {
                    docstring: new_docstring,
                    variants: new_variants,
                    ..
                },
            ) => {
                compare_enum_variants(name, old_variants, new_variants, cx, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Protocol {
                    docstring: old_docstring,
                    parameters: old_params,
                    functions: old_funcs,
                    ..
                },
                FrozenUnit::Protocol {
                    docstring: new_docstring,
                    parameters: new_params,
                    functions: new_funcs,
                    ..
                },
            ) => {
                compare_protocol_functions(
                    name,
                    (old_funcs, &old_errors),
                    (new_funcs, &new_errors),
                    cx,
                    &mut changes,
                );
                compare_parameters(name, old_params, new_params, &mut changes);
                // Protocol.docstring is a plain String (not Option<String>
                // like the others) - wrap for a uniform comparison helper.
                push_docstring_change(
                    name,
                    &Some(old_docstring.clone()),
                    &Some(new_docstring.clone()),
                    &mut changes,
                );
            }
            (
                FrozenUnit::Constant {
                    docstring: old_docstring,
                    ty: old_ty,
                    value: old_value,
                    ..
                },
                FrozenUnit::Constant {
                    docstring: new_docstring,
                    ty: new_ty,
                    value: new_value,
                    ..
                },
            ) => {
                if !compare_types(name, old_ty, new_ty, cx, &mut changes) {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedConstantType {
                            name: name.clone(),
                            old_type: old_ty.to_string(),
                            new_type: new_ty.to_string(),
                        });
                } else if old_value != new_value {
                    // Counted by what it reaches: a validator argument is the
                    // validator rules (a changed argument narrows), a field
                    // default a patch, nothing on the wire a minor.
                    let (old_value, new_value) = (
                        old_value.as_ref().map(Literal::to_string),
                        new_value.as_ref().map(Literal::to_string),
                    );
                    if cx.old.in_validators.contains(name) || cx.new.in_validators.contains(name) {
                        changes
                            .breaking_changes
                            .push(BreakingChange::ValidationTightened { target: name.clone() });
                    } else if cx.old.in_defaults.contains(name) || cx.new.in_defaults.contains(name) {
                        changes.modifications.push(Modification::ChangedConstantValue {
                            name: name.clone(),
                            old_value,
                            new_value,
                        });
                    } else {
                        changes.new_features.push(NewFeature::ChangedConstantValue {
                            name: name.clone(),
                            old_value,
                            new_value,
                        });
                    }
                }
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Error {
                    docstring: old_docstring,
                    parameters: old_params,
                    ordinal: old_ordinal,
                    message: old_message,
                    fields: old_fields,
                    ..
                },
                FrozenUnit::Error {
                    docstring: new_docstring,
                    parameters: new_params,
                    ordinal: new_ordinal,
                    message: new_message,
                    fields: new_fields,
                    ..
                },
            ) => {
                // The ordinal travels where a framing sends errors by it.
                if old_ordinal != new_ordinal && !cx.facts.errors_by_name {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedErrorOrdinal {
                            name: name.clone(),
                            old_ordinal: *old_ordinal,
                            new_ordinal: *new_ordinal,
                        });
                }
                // An error's fields travel like a struct's, so the same
                // rules apply.
                compare_struct_fields(name, old_fields, new_fields, cx, &mut changes);
                if old_message != new_message {
                    changes
                        .modifications
                        .push(Modification::ChangedErrorMessage { name: name.clone() });
                }
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::TypeAlias {
                    docstring: old_docstring,
                    parameters: old_params,
                    target: old_target,
                    ..
                },
                FrozenUnit::TypeAlias {
                    docstring: new_docstring,
                    parameters: new_params,
                    target: new_target,
                    ..
                },
            ) => {
                // What it resolves to changing is major (and shows again at
                // every use); a respelled target is a patch.
                if !compare_types(name, old_target, new_target, cx, &mut changes) {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedTypeAliasTarget {
                            name: name.clone(),
                            old_target: old_target.to_string(),
                            new_target: new_target.to_string(),
                        });
                }
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Validator {
                    docstring: old_docstring,
                    ..
                },
                FrozenUnit::Validator {
                    docstring: new_docstring,
                    ..
                },
            ) => {
                if validator_shape(old_unit) != validator_shape(new_unit) {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedValidator { name: name.clone() });
                }
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            _ => {}
        }
    }

    changes
}

/// Compare two spellings of a type after resolving aliases on each side
/// (core#110). `true` when they're the same type: a respelling is recorded
/// as [`Modification::TypeRespelled`] for `target`. `false` when they
/// differ, for the caller to report as it sees fit.
fn compare_types(
    target: &str,
    old: &TypeRef,
    new: &TypeRef,
    cx: &Context,
    changes: &mut SchemaChanges,
) -> bool {
    if cx.old.resolve(old) != cx.new.resolve(new) {
        return false;
    }
    if old != new {
        changes.modifications.push(Modification::TypeRespelled {
            target: target.to_string(),
            old_type: old.to_string(),
            new_type: new.to_string(),
        });
    }
    true
}

/// Record a `Modification::DocstringChanged` when a docstring differs
/// between versions - a documentation-only change warrants a patch bump,
/// not silence.
fn push_docstring_change(
    name: &str,
    old_docstring: &Option<String>,
    new_docstring: &Option<String>,
    changes: &mut SchemaChanges,
) {
    if old_docstring != new_docstring {
        changes.modifications.push(Modification::DocstringChanged {
            name: name.to_string(),
        });
    }
}

/// Declarations by name, sorted, so changes come out in a stable order.
fn build_index(schema: &[FrozenUnit]) -> BTreeMap<String, &FrozenUnit> {
    let mut index = BTreeMap::new();
    for unit in schema {
        let name = match unit {
            FrozenUnit::Struct { name, .. }
            | FrozenUnit::Enum { name, .. }
            | FrozenUnit::Protocol { name, .. }
            | FrozenUnit::Constant { name, .. }
            | FrozenUnit::Error { name, .. }
            | FrozenUnit::TypeAlias { name, .. }
            | FrozenUnit::Validator { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(n) = name {
            index.insert(n, unit);
        }
    }
    index
}

fn error_names(schema: &[FrozenUnit]) -> HashMap<u16, String> {
    schema
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Error { name, ordinal, .. } => Some((*ordinal, name.clone())),
            _ => None,
        })
        .collect()
}

struct FieldInfo<'a> {
    position: usize,
    ty: &'a TypeRef,
    optional: bool,
    default: &'a Option<Literal>,
    docstring: &'a Option<String>,
    parameters: &'a [FrozenUnit],
}

/// Whether the names both versions share kept their relative positions:
/// each one's index in `old` and `new`, among the shared names only.
fn shared_order_kept<'a>(old: &[&'a str], new: &[&'a str]) -> bool {
    let shared_old: Vec<&str> = old.iter().copied().filter(|n| new.contains(n)).collect();
    let shared_new: Vec<&str> = new.iter().copied().filter(|n| old.contains(n)).collect();
    shared_old == shared_new
}

/// Whether every name `old` has is at the same index in `new`: nothing
/// moved, and nothing was inserted before an old one.
fn positions_kept(old: &[&str], new: &[&str]) -> bool {
    old.iter()
        .enumerate()
        .all(|(i, name)| !new.contains(name) || new.get(i) == Some(name))
}

fn compare_struct_fields(
    struct_name: &str,
    old_fields: &[FrozenUnit],
    new_fields: &[FrozenUnit],
    cx: &Context,
    changes: &mut SchemaChanges,
) {
    let old_field_map = build_field_map(old_fields);
    let new_field_map = build_field_map(new_fields);
    let order = |map: &HashMap<String, FieldInfo>| {
        let mut names: Vec<(usize, String)> = map.iter().map(|(n, f)| (f.position, n.clone())).collect();
        names.sort();
        names.into_iter().map(|(_, n)| n).collect::<Vec<_>>()
    };
    let (old_order, new_order) = (order(&old_field_map), order(&new_field_map));
    let old_order: Vec<&str> = old_order.iter().map(String::as_str).collect();
    let new_order: Vec<&str> = new_order.iter().map(String::as_str).collect();

    // Removed fields (breaking), optional ones included: the name is gone.
    for field_name in &old_order {
        if !new_field_map.contains_key(*field_name) {
            changes.breaking_changes.push(BreakingChange::RemovedField {
                type_name: struct_name.to_string(),
                field_name: field_name.to_string(),
            });
        }
    }

    // Where fields travel by position, moving one breaks every peer.
    if !cx.facts.fields_by_name && !shared_order_kept(&old_order, &new_order) {
        changes.breaking_changes.push(BreakingChange::ReorderedFields {
            type_name: struct_name.to_string(),
        });
    }

    for field_name in &new_order {
        let new = &new_field_map[*field_name];
        let field_name = field_name.to_string();
        let Some(old) = old_field_map.get(&field_name) else {
            // A required field is always breaking. An optional one is a
            // feature where every format tolerates an unknown field and a
            // missing optional one, and either keys fields by name or gets
            // it after every old field.
            let appended = new.position >= old_order.len();
            if new.optional
                && cx.facts.optional_fields_tolerated
                && (cx.facts.fields_by_name || appended)
            {
                changes.new_features.push(NewFeature::AddedField {
                    type_name: struct_name.to_string(),
                    field_name,
                    field_type: new.ty.to_string(),
                    optional: true,
                });
            } else if new.optional {
                changes.breaking_changes.push(BreakingChange::AddedOptionalField {
                    type_name: struct_name.to_string(),
                    field_name,
                    field_type: new.ty.to_string(),
                });
            } else {
                changes
                    .breaking_changes
                    .push(BreakingChange::AddedRequiredField {
                        type_name: struct_name.to_string(),
                        field_name,
                        field_type: new.ty.to_string(),
                    });
            }
            continue;
        };
        let target = format!("{struct_name}.{field_name}");

        // Exact match after resolving aliases, so `f32` -> `f64` and
        // `u8[4]` -> `u8[8]` count, and an alias swap doesn't.
        if !compare_types(&target, old.ty, new.ty, cx, changes) {
            let (old_resolved, new_resolved) = (cx.old.resolve(old.ty), cx.new.resolve(new.ty));
            let render = |written: &TypeRef, resolved: &TypeRef| {
                if written == resolved {
                    written.to_string()
                } else {
                    format!("{written} ({resolved})")
                }
            };
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFieldType {
                    type_name: struct_name.to_string(),
                    field_name: field_name.clone(),
                    old_type: render(old.ty, &old_resolved),
                    new_type: render(new.ty, &new_resolved),
                });
        }
        if old.optional != new.optional {
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFieldOptionality {
                    type_name: struct_name.to_string(),
                    field_name: field_name.clone(),
                    now_optional: new.optional,
                });
        }
        if old.default != new.default {
            changes.modifications.push(Modification::ChangedFieldDefault {
                type_name: struct_name.to_string(),
                field_name: field_name.clone(),
            });
        }
        compare_parameters_with(
            &target,
            (old.parameters, cx.old.alias_validators(old.ty)),
            (new.parameters, cx.new.alias_validators(new.ty)),
            changes,
        );
        push_docstring_change(&target, old.docstring, new.docstring, changes);
    }
}

fn build_field_map(fields: &[FrozenUnit]) -> HashMap<String, FieldInfo<'_>> {
    fields
        .iter()
        .filter(|f| matches!(f, FrozenUnit::Field { .. }))
        .enumerate()
        .filter_map(|(position, field)| match field {
            FrozenUnit::Field {
                name,
                ty,
                optional,
                default,
                docstring,
                parameters,
                ..
            } => Some((
                name.clone(),
                FieldInfo {
                    position,
                    ty,
                    optional: *optional,
                    default,
                    docstring,
                    parameters,
                },
            )),
            _ => None,
        })
        .collect()
}

fn compare_enum_variants(
    enum_name: &str,
    old_variants: &[FrozenUnit],
    new_variants: &[FrozenUnit],
    cx: &Context,
    changes: &mut SchemaChanges,
) {
    let old_names = variant_names(old_variants);
    let new_names = variant_names(new_variants);
    let removed: Vec<&str> = old_names
        .iter()
        .copied()
        .filter(|v| !new_names.contains(v))
        .collect();
    let added: Vec<&str> = new_names
        .iter()
        .copied()
        .filter(|v| !old_names.contains(v))
        .collect();

    // Exactly one variant gone and one new in its place: a rename.
    if let ([old], [new]) = (removed.as_slice(), added.as_slice()) {
        let same_place = old_names.iter().position(|v| v == old)
            == new_names.iter().position(|v| v == new);
        if same_place {
            changes
                .breaking_changes
                .push(BreakingChange::RenamedEnumVariant {
                    enum_name: enum_name.to_string(),
                    old_name: old.to_string(),
                    new_name: new.to_string(),
                });
            return;
        }
    }

    // Where variants travel by index, one that moved, or an insertion
    // before an old one, breaks every peer.
    if !cx.facts.variants_by_name && removed.is_empty() && !positions_kept(&old_names, &new_names) {
        changes.breaking_changes.push(BreakingChange::ReorderedEnumVariants {
            enum_name: enum_name.to_string(),
        });
    }

    // Removed variants (breaking)
    for variant in removed {
        changes
            .breaking_changes
            .push(BreakingChange::RemovedEnumVariant {
                enum_name: enum_name.to_string(),
                variant: variant.to_string(),
            });
    }

    // Added variants (feature)
    for variant in added {
        changes.new_features.push(NewFeature::AddedEnumVariant {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
        });
    }
}

/// Variant names in declaration order.
fn variant_names(variants: &[FrozenUnit]) -> Vec<&str> {
    variants
        .iter()
        .filter_map(|v| match v {
            FrozenUnit::EnumVariant(name, _) => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

struct FunctionInfo<'a> {
    signature: String,
    arguments: &'a [FrozenArgument],
    ret: &'a Option<TypeRef>,
    throws: Vec<String>,
    docstring: &'a str,
    parameters: &'a [FrozenUnit],
}

fn compare_protocol_functions(
    protocol_name: &str,
    (old_funcs, old_errors): (&[FrozenUnit], &HashMap<u16, String>),
    (new_funcs, new_errors): (&[FrozenUnit], &HashMap<u16, String>),
    cx: &Context,
    changes: &mut SchemaChanges,
) {
    let old_func_map = build_function_map(old_funcs, old_errors);
    let new_func_map = build_function_map(new_funcs, new_errors);
    let order = |funcs: &[FrozenUnit]| -> Vec<String> {
        funcs
            .iter()
            .filter_map(|f| match f {
                FrozenUnit::Function { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    let (old_order, new_order) = (order(old_funcs), order(new_funcs));
    let old_order: Vec<&str> = old_order.iter().map(String::as_str).collect();
    let new_order: Vec<&str> = new_order.iter().map(String::as_str).collect();

    // Removed functions (breaking)
    for func_name in &old_order {
        if !new_func_map.contains_key(*func_name) {
            changes
                .breaking_changes
                .push(BreakingChange::RemovedFunction {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.to_string(),
                });
        }
    }

    // Where the framing calls by index, a function that moved, or one
    // inserted before an old one, breaks every peer.
    // (A removal shifts the rest too, but is already reported.)
    let removed_any = old_order.iter().any(|f| !new_func_map.contains_key(*f));
    if !cx.facts.calls_stable(&cx.namespace, protocol_name)
        && !removed_any
        && !positions_kept(&old_order, &new_order)
    {
        changes.breaking_changes.push(BreakingChange::ReorderedFunctions {
            protocol_name: protocol_name.to_string(),
        });
    }

    for func_name in &new_order {
        let new = &new_func_map[*func_name];
        let func_name = func_name.to_string();
        let Some(old) = old_func_map.get(&func_name) else {
            // Added functions (feature)
            changes.new_features.push(NewFeature::AddedFunction {
                protocol_name: protocol_name.to_string(),
                function_name: func_name.clone(),
                signature: new.signature.clone(),
            });
            continue;
        };
        let target = format!("{protocol_name}.{func_name}");

        // Changed function signatures (breaking), compared after resolving
        // aliases: a respelled argument or return type is a patch.
        let resolved = |side: &Side, f: &FunctionInfo| {
            let arguments: Vec<FrozenArgument> = f
                .arguments
                .iter()
                .map(|a| FrozenArgument { name: a.name.clone(), ty: side.resolve(&a.ty), span: (0, 0) })
                .collect();
            format_function_signature(&arguments, &f.ret.as_ref().map(|t| side.resolve(t)))
        };
        let (old_resolved, new_resolved) = (resolved(&cx.old, old), resolved(&cx.new, new));
        if old_resolved != new_resolved {
            let details = if old.signature != new.signature {
                format!("{} → {}", old.signature, new.signature)
            } else {
                format!("{old_resolved} → {new_resolved}")
            };
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFunctionSignature {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.clone(),
                    details,
                });
        } else if old.signature != new.signature {
            changes.modifications.push(Modification::TypeRespelled {
                target: target.clone(),
                old_type: old.signature.clone(),
                new_type: new.signature.clone(),
            });
        }

        for error in old.throws.iter().filter(|e| !new.throws.contains(e)) {
            changes
                .breaking_changes
                .push(BreakingChange::RemovedThrownError {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.clone(),
                    error: error.clone(),
                });
        }
        for error in new.throws.iter().filter(|e| !old.throws.contains(e)) {
            changes.new_features.push(NewFeature::AddedThrownError {
                protocol_name: protocol_name.to_string(),
                function_name: func_name.clone(),
                error: error.clone(),
            });
        }

        compare_parameters(&target, old.parameters, new.parameters, changes);
        if old.docstring != new.docstring {
            changes.modifications.push(Modification::DocstringChanged { name: target });
        }
    }
}

fn build_function_map<'a>(
    functions: &'a [FrozenUnit],
    errors: &HashMap<u16, String>,
) -> HashMap<String, FunctionInfo<'a>> {
    functions
        .iter()
        .filter_map(|func| match func {
            FrozenUnit::Function {
                name,
                arguments,
                _return,
                throws,
                docstring,
                parameters,
                ..
            } => Some((
                name.clone(),
                FunctionInfo {
                    signature: format_function_signature(arguments, _return),
                    arguments,
                    ret: _return,
                    throws: throws
                        .iter()
                        .map(|o| errors.get(o).cloned().unwrap_or_else(|| format!("#{o}")))
                        .collect(),
                    docstring,
                    parameters,
                },
            )),
            _ => None,
        })
        .collect()
}

/// Argument names are part of the signature: JSON keys parameters by name,
/// so renaming one breaks decoding there.
fn format_function_signature(args: &[FrozenArgument], ret: &Option<TypeRef>) -> String {
    let args: Vec<String> = args
        .iter()
        .map(|arg| format!("{}: {}", arg.name, arg.ty))
        .collect();

    let ret_type = ret
        .as_ref()
        .map(TypeRef::to_string)
        .unwrap_or_else(|| "void".to_string());

    format!("({}) -> {}", args.join(", "), ret_type)
}

/// A rendered `@handshake` value as the handshake it selects.
fn effective_handshake(rendered: &Option<String>) -> &str {
    rendered.as_deref().unwrap_or(crate::schema::idl::annotations::DEFAULT_HANDSHAKE)
}

/// Compare a declaration's annotations: `@key = value` properties,
/// `@deprecated`, and the validators applied to it.
fn compare_parameters(
    target: &str,
    old: &[FrozenUnit],
    new: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    compare_parameters_with(target, (old, vec![]), (new, vec![]), changes)
}

/// [`compare_parameters`], with the validators each side inherits from
/// the aliases its type goes through: the effective set is both.
fn compare_parameters_with(
    target: &str,
    (old, old_inherited): (&[FrozenUnit], Vec<FrozenUnit>),
    (new, new_inherited): (&[FrozenUnit], Vec<FrozenUnit>),
    changes: &mut SchemaChanges,
) {
    let old_props = property_map(old);
    let new_props = property_map(new);
    let keys: std::collections::BTreeSet<&str> =
        old_props.keys().chain(new_props.keys()).copied().collect();
    for key in keys {
        let old_value = old_props.get(key).map(|v| render_value(v));
        let new_value = new_props.get(key).map(|v| render_value(v));
        if old_value == new_value {
            continue;
        }
        // An absent `@handshake` means `comline@1`; spelling it out changes
        // nothing a peer sees.
        if key == "handshake" && effective_handshake(&old_value) == effective_handshake(&new_value) {
            continue;
        }
        // `"jsonrpc"` is shorthand for `{ name = "jsonrpc" }`, and an alias
        // spells the same framing: compared by what they mean.
        if key == "framing" {
            let meaning = |props: &BTreeMap<&str, &ConfigValue>| {
                props.get(key).and_then(|v| crate::schema::idl::annotations::FramingValue::of(v))
            };
            if let (Some(old), Some(new)) = (meaning(&old_props), meaning(&new_props)) {
                if old == new {
                    continue;
                }
            }
        }
        match key {
            // Build-time only: it never reaches a peer or generated code.
            "settings" => {}
            // Adding or removing the marker is judged by the set it freezes,
            // below; only a reworded reason is reported here.
            "unverified" if old_value.is_none() || new_value.is_none() => {}
            "framing" | "handshake" => changes
                .breaking_changes
                .push(BreakingChange::ChangedWireAnnotation {
                    target: target.to_string(),
                    key: key.to_string(),
                    old_value,
                    new_value,
                }),
            _ => changes.modifications.push(Modification::ChangedAnnotation {
                target: target.to_string(),
                key: key.to_string(),
                old_value,
                new_value,
            }),
        }
    }

    let old_given_up = unverified::frozen_set(old);
    let new_given_up = unverified::frozen_set(new);
    let gave_up: Vec<Guarantee> =
        new_given_up.iter().filter(|g| !old_given_up.contains(g)).copied().collect();
    let restored: Vec<Guarantee> =
        old_given_up.iter().filter(|g| !new_given_up.contains(g)).copied().collect();
    if !gave_up.is_empty() {
        changes.breaking_changes.push(BreakingChange::GaveUpGuarantees {
            target: target.to_string(),
            guarantees: gave_up,
        });
    }
    if !restored.is_empty() {
        changes.new_features.push(NewFeature::RestoredGuarantees {
            target: target.to_string(),
            guarantees: restored,
        });
    }

    let old_deprecated = deprecation(old);
    let new_deprecated = deprecation(new);
    if old_deprecated != new_deprecated {
        if old_deprecated.is_none() {
            changes.new_features.push(NewFeature::Deprecated {
                target: target.to_string(),
            });
        } else {
            changes.modifications.push(Modification::ChangedAnnotation {
                target: target.to_string(),
                key: "deprecated".to_string(),
                old_value: old_deprecated,
                new_value: new_deprecated,
            });
        }
    }

    // The effective set: inherited from aliases, then the declaration's
    // own. Compared as a set, so moving one around isn't a change.
    let effective = |inherited: Vec<FrozenUnit>, own: &[FrozenUnit]| {
        let mut set: Vec<FrozenUnit> = inherited.iter().map(without_spans).collect();
        set.extend(validator_refs(own));
        set.sort_by_key(|v| format!("{v:?}"));
        set.dedup();
        set
    };
    let old_validators = effective(old_inherited, old);
    let new_validators = effective(new_inherited, new);
    if old_validators != new_validators {
        let only_removed = new_validators.len() < old_validators.len()
            && new_validators.iter().all(|v| old_validators.contains(v));
        if only_removed {
            changes.new_features.push(NewFeature::ValidationLoosened {
                target: target.to_string(),
            });
        } else {
            changes
                .breaking_changes
                .push(BreakingChange::ValidationTightened {
                    target: target.to_string(),
                });
        }
    }
}

fn property_map(parameters: &[FrozenUnit]) -> BTreeMap<&str, &ConfigValue> {
    parameters
        .iter()
        .filter_map(|p| match p {
            FrozenUnit::Property { name, value } => Some((name.as_str(), value)),
            _ => None,
        })
        .collect()
}

fn render_value(value: &ConfigValue) -> String {
    value
        .as_text()
        .map(|t| t.into_owned())
        .unwrap_or_else(|| "(marker)".to_string())
}

/// `@deprecated`'s message, or `""` for a bare one. `None` when it's absent
/// or `false`.
fn deprecation(parameters: &[FrozenUnit]) -> Option<String> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Deprecated(DeprecatedValue::False) => None,
        FrozenUnit::Deprecated(DeprecatedValue::Message(m)) => Some(m.clone()),
        FrozenUnit::Deprecated(other) => Some(format!("{other:?}")),
        _ => None,
    })
}

/// The validators applied to a declaration, without source positions, so
/// moving them around in the file isn't a change.
fn validator_refs(parameters: &[FrozenUnit]) -> Vec<FrozenUnit> {
    parameters
        .iter()
        .filter(|p| matches!(p, FrozenUnit::ValidatorRef { .. }))
        .map(without_spans)
        .collect()
}

/// A validator declaration without its docstring or source positions: what
/// decides whether it accepts something differently.
fn validator_shape(unit: &FrozenUnit) -> Option<FrozenUnit> {
    match without_spans(unit) {
        FrozenUnit::Validator {
            properties,
            name,
            targets,
            expression_block,
            ..
        } => Some(FrozenUnit::Validator {
            docstring: None,
            properties,
            name,
            targets,
            expression_block,
            span: (0, 0),
            validate_span: None,
        }),
        _ => None,
    }
}

/// A copy of `unit` with every source position cleared, for comparing
/// content regardless of formatting. Covers the units validators and their
/// references are built from.
fn without_spans(unit: &FrozenUnit) -> FrozenUnit {
    let all = |units: &[FrozenUnit]| units.iter().map(without_spans).collect::<Vec<_>>();
    match unit {
        FrozenUnit::Validator {
            docstring,
            properties,
            name,
            targets,
            expression_block,
            ..
        } => FrozenUnit::Validator {
            docstring: docstring.clone(),
            properties: all(properties),
            name: name.clone(),
            targets: targets.clone(),
            expression_block: Box::new(without_spans(expression_block)),
            span: (0, 0),
            validate_span: None,
        },
        FrozenUnit::ValidatorRef { name, args, .. } => FrozenUnit::ValidatorRef {
            name: name.clone(),
            args: all(args),
            span: (0, 0),
            arg_spans: vec![],
        },
        FrozenUnit::ExpressionBlock { asserts } => FrozenUnit::ExpressionBlock {
            asserts: all(asserts),
        },
        FrozenUnit::Assert {
            condition,
            message,
            references,
            expr,
            uninterpolated,
            ..
        } => FrozenUnit::Assert {
            condition: condition.clone(),
            message: message.clone(),
            references: references.clone(),
            expr: expr.clone(),
            message_span: None,
            uninterpolated: uninterpolated.clone(),
        },
        FrozenUnit::Field {
            docstring,
            parameters,
            optional,
            name,
            ty,
            default,
            ..
        } => FrozenUnit::Field {
            docstring: docstring.clone(),
            parameters: all(parameters),
            optional: *optional,
            name: name.clone(),
            ty: ty.clone(),
            default: default.clone(),
            span: (0, 0),
        },
        other => other.clone(),
    }
}

// ===== Backward Compatibility =====
// The Differ trait is used by the (incomplete) autodoc system
// We keep it here for compatibility, though it's not used by the new diff system

/// Legacy trait for diffing operations (used by autodoc)
#[allow(unused)]
pub trait Differ {
    fn on_namespace_changed(&mut self, old: &str, new: &str);
    fn on_const_name_changed(&mut self, old: &str, new: &str);
    fn on_const_kind_changed(&mut self, old: u8, new: u8);
    fn on_const_default_value_changed(
        &mut self,
        name: &str,
        left_value: Option<&Literal>,
        right_value: Option<&Literal>,
    );
}
