//! The frozen types of IR format 3, as they are today.
//!
//! Copies of the types core persists under `.comline/`, not re-exports: a
//! change to a live type can't change format 3 by changing this file's view
//! of it. Serde derives and nothing else, with every `#[serde(...)]`
//! attribute that decides the bytes. No behaviour and no dependency on
//! `comline-core` (the `live == frozen(3)` test is in core, which depends on
//! this crate).
//!
//! Names differ from core's where core has two types of one name: core's
//! `schema::ir::frozen::unit::FrozenUnit` is [`SchemaUnit`] here, and
//! `package::config::ir::frozen::FrozenUnit` is [`ConfigUnit`]. A CBOR
//! encoding names fields and variants, never types, so that changes no
//! bytes. [`EncodingKind`] is core's `ProfileKind`.
//!
//! Where each type is stored, and what an absent field or entry means, is in
//! `SCHEMA.md` beside this file.

use std::collections::BTreeMap;

use serde_derive::{Deserialize, Serialize};

// ---------- objects: the container ----------

/// A BLAKE3 digest, serialized as the 32 bytes' sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub mode: EntryMode,
    pub name: String,
    pub hash: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryMode {
    Blob,
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub tree: Hash,
    pub parents: Vec<Hash>,
    pub author: String,
    pub timestamp: i64,
    pub message: String,
    pub version: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub computed: VersionBump,
    pub applied: VersionBump,
    pub reason: Option<String>,
    pub covered: bool,
    pub stable: bool,
    pub since: Option<Hash>,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionBump {
    None,
    Patch,
    Minor,
    Major,
}


// ---------- schema units: one blob per unit, in a `schema_<n>` tree ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaUnit {
    Namespace(String),
    Name(String),
    Import(String, Option<String>, (usize, usize)),
    Constant {
        docstring: Option<String>,
        name: String,
        ty: TypeRef,
        value: Option<Literal>,
        span: (usize, usize),
    },
    Property {
        name: String,
        value: ConfigValue
    },
    ExpressionBlock {
        asserts: Vec<SchemaUnit>
    },
    Assert {
        condition: String,
        message: String,
        references: Vec<String>,
        expr: Condition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_span: Option<(usize, usize)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        uninterpolated: Vec<String>,
    },
    Enum {
        docstring: Option<String>,
        name: String,
        variants: Vec<SchemaUnit>,
        span: (usize, usize),
    },
    EnumVariant(String, (usize, usize)),
    Settings {
        docstring: Option<String>,
        name: Option<String>,
        values: SettingsDict,
        spans: SettingsSpans,
    },
    Struct {
        docstring: Option<String>,
        parameters: Vec<SchemaUnit>,
        name: String,
        fields: Vec<SchemaUnit>,
        span: (usize, usize),
    },
    Protocol {
        docstring: String,
        parameters: Vec<SchemaUnit>,
        name: String,
        functions: Vec<SchemaUnit>,
        span: (usize, usize),
    },
    Function {
        docstring: String,
        parameters: Vec<SchemaUnit>,
        name: String,
        arguments: Vec<FrozenArgument>,
        _return: Option<TypeRef>,
        throws: Vec<u16>,
        span: (usize, usize),
    },
    Error {
        docstring: Option<String>,
        parameters: Vec<SchemaUnit>,
        ordinal: u16,
        imported_from: Option<String>,
        name: String,
        message: String,
        fields: Vec<SchemaUnit>,
        #[serde(default)]
        span: Option<(usize, usize)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_span: Option<(usize, usize)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        uninterpolated: Vec<String>,
    },
    Validator {
        docstring: Option<String>,
        properties: Vec<SchemaUnit>,
        name: String,
        targets: Vec<TypeRef>,
        expression_block: Box<SchemaUnit>,
        #[serde(default)]
        span: (usize, usize),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        validate_span: Option<(usize, usize)>,
    },
    ValidatorRef {
        name: String,
        args: Vec<SchemaUnit>,
        #[serde(default)]
        span: (usize, usize),
        #[serde(default)]
        arg_spans: Vec<(usize, usize)>,
    },
    Field {
        docstring: Option<String>,
        parameters: Vec<SchemaUnit>,
        optional: bool,
        name: String,
        ty: TypeRef,
        default: Option<Literal>,
        span: (usize, usize),
    },
    TypeAlias {
        docstring: Option<String>,
        #[serde(default)]
        parameters: Vec<SchemaUnit>,
        name: String,
        target: TypeRef,
        span: (usize, usize),
    },
    Deprecated(DeprecatedValue),
    Unverified(Vec<Guarantee>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigValue {
    Marker,
    Bool(bool),
    Integer(i64),
    Str(String),
    Name(String),
    Path(String),
    FString(String),
    Dict(Vec<(String, ConfigValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeprecatedValue {
    False,
    True,
    Message(String),
    Malformed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct  FrozenArgument {
    pub name: String,
    pub ty: TypeRef,
    pub span: (usize, usize),
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<SchemaUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Guarantee {
    SchemaAgreement,
    EncodingAgreement,
    EncodingConformance,
    VersionHonesty,
    BoundedSize,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveType {
    S8,
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    String,
    U128,
    S128,
    Char,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeRef {
    Primitive(PrimitiveType),
    Named(String),
    Array {
        element: Box<TypeRef>,
        length: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u64>,
    },
    Union(Vec<TypeRef>),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Literal {
    Integer(i64),
    Bool(bool),
    String(String),
    Reference(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub first: Comparison,
    pub rest: Vec<(LogicalOp, Comparison)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub left: Operand,
    pub op: CompareOp,
    pub right: Operand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operand {
    Path(Vec<String>),
    Integer(i64),
    String(String),
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsValue {
    Bool(bool),
    Integer(i64),
    Str(String),
    Identifier(String),
    Dict(SettingsDict),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDict(pub BTreeMap<String, SettingsValue>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSpans(pub BTreeMap<String, (usize, usize)>);


// ---------- config units: the `config` blob and a dependency's ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigUnit {
    Namespace(String),
    PackageId(String),
    SpecificationVersion(u8),
    PackageVersion(String),
    SchemaPath(String),
    Dependency(Dependency),
    CodeGeneration(LanguageDetails),
    PublishRegistry((String, PublishRegistry)),
    Settings(SettingsDict),
    Handshake(String),
    HandshakeParams(BTreeMap<String, String>),
    Wire {
        format: Option<String>,
        framing: Option<String>,
    },
    Encoding {
        name: String,
        kind: EncodingKind,
        profile_hash: String,
        map: EncodingMap,
        #[serde(default)]
        establishes: Vec<Guarantee>,
        #[serde(default)]
        has_vectors: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        grammar: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        calls: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directions: Option<String>,
    },
    Unverified(ConfigValue),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub author: String,
    pub project: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageDetails {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRegistry {
    pub kind: RegistryKind,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryKind {
    LocalStorage, RegistryServer
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingKind {
    Format,
    Framing,
    Handshake,
}


pub type EncodingMap = BTreeMap<String, EncodingMapping>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingMapping {
    #[serde(rename = "type")]
    pub ty: String,
    pub package: Option<String>,
    pub version: Option<String>,
}


// ---------- the `docs` blob ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenDocs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modules: BTreeMap<String, String>,
}


// ---------- the `facts` blob ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub protocols: Vec<ProtocolHistory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
    #[serde(default)]
    pub numbered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolHistory {
    pub path: String,
    pub line: String,
    #[serde(default)]
    pub number: u16,
    pub functions: Vec<FunctionFacts>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encodings: Vec<EncodingFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFacts {
    pub name: String,
    pub since: String,
    #[serde(default)]
    pub number: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    pub number: u16,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingFacts {
    pub kind: String,
    pub name: String,
    pub params: BTreeMap<String, String>,
    pub param_hash: String,
}

// ---------- reserved: nothing writes these yet ----------

/// One element of the root tree's `unknown` blob (a CBOR array of these):
/// something a step could not know, and what it recorded. Reserved: no
/// release writes it yet, and a reader that meets one is refused (see
/// `codec::read_unknown`). `deny_unknown_fields`, so that a record with a
/// field this build doesn't know is refused rather than read short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownRecord {
    /// A JSON pointer into the version's model.
    pub path: String,
    /// The step that wrote it, as `"<from>-<to>"` (`"3-4"`).
    pub step: String,
    /// Why the format it came from could not know it.
    pub reason: String,
    /// What that format recorded there, where it recorded something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<Vec<u8>>,
}
