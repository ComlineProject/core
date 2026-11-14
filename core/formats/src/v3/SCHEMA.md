# IR format 3

The format of what comline stores under `.comline/` when `FORMAT_VERSION`
holds `3`. This is the source of truth for the format: the frozen types in
[`types.rs`](types.rs) are its copy in code, [`codec.rs`](codec.rs) is the only
code that reads and writes its bytes, and [`ledger.toml`](ledger.toml) lists
what was added to it without a bump. The design is
[IR migrations](https://github.com/ComlineProject/docs/tree/main/docs/docs/proposals/ir-migrations)
(core#268). The guide on the CAS describes only the current format and links
here for the others.

Format 3 is **the current format** and is not sealed: core's test
`live == frozen(current)` holds the live types to the copy in this directory.
The directory is sealed in `formats/MANIFEST` when format 4 is introduced.

## Stamps

| Format | Stamp | Introduced | What it is |
|---|---|---|---|
| 0 | none | 2026-01-10 | bincode, which tags fields and variants by position. Never marked, never migrated |
| 1 | `1` | 2026-10-11 | Name-tagged CBOR. The stamp is introduced |
| 2 | `2` | 2026-10-12 | `Property.expression: Option<String>` became `value: ConfigValue` |
| 3 | `3` | 2026-10-18 | `KindValue` text became `TypeRef`, `Literal` and `Condition` |

Formats 0 to 2 have no document and no frozen copy: nothing reads them, and a
store in one is never touched by a command on its own.

## The store

| Path | Holds |
|---|---|
| `FORMAT_VERSION` | The stamp: the format number as decimal text |
| `objects/<xx>/<rest>` | One object each, named by the lowercase hex of its hash: the first two digits are the directory. The file is the object's CBOR, compressed with LZ4 (block, the size prepended). The hash is the BLAKE3 of the **uncompressed** CBOR |
| `refs/heads/main` | The hash of the newest commit, in hex. After a Git merge, conflict markers around two |
| `deps-cache/` | Git checkouts of dependencies. A cache, not part of the store |

The container (the object layout, CBOR, LZ4, BLAKE3, `Blob`, `Tree`, `Commit`,
the ref file) is the same in every CBOR format so far. A change to it is a
format change like any other.

## Objects

CBOR here is what `ciborium` writes for the serde derive of each type: structs
as maps keyed by field name, enums externally tagged (a unit variant is its
name; any other, a one-entry map from the name), tuples and `Vec` as arrays,
`Option` as the value or null. A field marked *skipped when empty* is left out
of the map when it holds `None`, an empty `Vec` or an empty map, and a field
marked *defaults* reads as its default when absent. Both exist so that content
that doesn't use a field keeps the bytes it had before the field.

**Hash** is a BLAKE3 digest, serialized as the array of its 32 bytes.

**Blob** `{ content: bytes }`: raw content, which the blobs below describe.

**Tree** `{ entries: [TreeEntry] }`, in the order written. A
`TreeEntry` is `{ mode, name, hash }`, where `mode` is `Blob` or `Tree`.

**Commit** `{ tree, parents, author, timestamp, message, version, release? }`

| Field | Meaning | Absent reads as |
|---|---|---|
| `tree` | The root tree's hash | |
| `parents` | Parent commits, first parent first. Walks follow the first; a second is what a reconcile after a Git merge records | |
| `author`, `message` | Text. The author is the user name | |
| `timestamp` | Unix seconds | |
| `version` | Semver: a release (`0.3.0`) or a build between releases (`0.3.0-dev.2`) | |
| `release` | Set on a commit `release` made. *Defaults, skipped when absent* | a release only if `version` has no pre-release part (3.05) |

**Release** `{ computed, applied, reason, covered, stable, since }`:
`computed` and `applied` are a `VersionBump` (`None`, `Patch`, `Minor`,
`Major`), `reason` is optional text, `covered` and `stable` are booleans, and
`since` is the previous release's commit hash, if there is one.

## A version's root tree

The root tree of a commit holds:

| Entry | Mode | Holds | Absent reads as |
|---|---|---|---|
| `schema_<n>` | Tree | One schema file's frozen units, `<n>` its index among the package's schemas | |
| `dep_<name>` | Tree | A vendored dependency, `<name>` its declared name: [below](#a-dependency-tree) | the dependency was not vendored (built without `deps`) |
| `config` | Blob | The package's frozen config: CBOR of `[ConfigUnit]` | the version was written before configs were kept: **unseen** |
| `docs` | Blob | The `//!` headers: CBOR of `FrozenDocs`. Written only if there are any | no headers (3.10) |
| `facts` | Blob | The protocols' history: CBOR of `History` | computed when read (3.11) |
| `unknown` | Blob | CBOR of `[UnknownRecord]`. **Reserved**: nothing writes it | no unknowns (3.21) |
| `produced_by` | Blob | CBOR of `ProducedBy`. **Reserved**: nothing writes it | unknown (3.22) |

Entries are in the order a build adds them: the schemas, the dependencies,
`config`, `docs`, then `facts`. A reader finds entries by name and mode, and
skips a name it doesn't know; the ledger is why a name may be new.

A `schema_<n>` tree has one Blob per frozen unit of the file, in the order the
compiler froze them, each `{ content }` holding the CBOR of one `SchemaUnit`.
The names are `namespace_<ns>`, `struct_<name>`, `enum_<name>`,
`protocol_<name>`, `const_<name>`, `import_<index>`, and `unit_<index>` for
the rest; they only make a tree readable, and a reader doesn't depend on them.

### A dependency tree

`dep_<name>` holds `schema_<n>` trees like a root tree's, and Blobs:

| Entry | Holds | Absent reads as |
|---|---|---|
| `manifest` | The content hash verified when the dependency was vendored, as text: its hex digits | |
| `config` | The dependency's own frozen config: CBOR of `[ConfigUnit]`, written even when empty | **unseen**: not empty, not defaulted (3.19) |
| `dependency_configs` | The frozen configs of the dependency's own dependencies: CBOR of `[(name, [ConfigUnit])]`, in the order the compile saw them. Written only where it has any | **unseen** (3.19) |

## Schema units

`SchemaUnit` (`FrozenUnit` in `comline_core::schema::ir::frozen::unit`) is
one of the variants below. `span` is `(start, end)`, byte offsets into the
source file. A span is part of a unit's identity on purpose: a change that only
moves text is a different blob.

| Variant | Fields |
|---|---|
| `Namespace(String)`, `Name(String)` | |
| `Import(path, alias?, span)` | the resolved path; the `X` of `use p as X` |
| `Constant` | `docstring?`, `name`, `ty: TypeRef`, `value?: Literal`, `span` |
| `Property` | `name`, `value: ConfigValue`: an `@key = value` annotation |
| `ExpressionBlock` | `asserts: [SchemaUnit]` |
| `Assert` | `condition`, `message`, `references`, `expr: Condition`, `message_span?`, `uninterpolated` (the last two *defaults, skipped when empty*) |
| `Enum` | `docstring?`, `name`, `variants: [SchemaUnit]`, `span` |
| `EnumVariant(name, span)` | |
| `Settings` | `docstring?`, `name?`, `values: SettingsDict`, `spans: SettingsSpans` |
| `Struct` | `docstring?`, `parameters`, `name`, `fields`, `span` |
| `Protocol` | `docstring`, `parameters`, `name`, `functions`, `span` |
| `Function` | `docstring`, `parameters`, `name`, `arguments: [FrozenArgument]`, `_return?: TypeRef`, `throws: [u16]` (the `ordinal` of each error), `span` |
| `Error` | `docstring?`, `parameters`, `ordinal: u16`, `imported_from?`, `name`, `message`, `fields`, `span?` (*defaults*), `message_span?`, `uninterpolated` (*defaults, skipped when empty*) |
| `Validator` | `docstring?`, `properties`, `name`, `targets: [TypeRef]`, `expression_block: SchemaUnit`, `span` (*defaults*), `validate_span?` (*defaults, skipped when absent*) |
| `ValidatorRef` | `name`, `args`, `span` (*defaults*), `arg_spans` (*defaults*) |
| `Field` | `docstring?`, `parameters`, `optional`, `name`, `ty`, `default?: Literal`, `span` |
| `TypeAlias` | `docstring?`, `parameters` (*defaults*), `name`, `target: TypeRef`, `span` |
| `Deprecated(DeprecatedValue)` | `False`, `True`, `Message(String)`, `Malformed(String)` |
| `Unverified([Guarantee])` | a protocol's frozen set of given-up guarantees |

`parameters` is a list of `Property`, `Unverified` and `Deprecated` units.
`FrozenArgument` is `{ name, ty, span, parameters }`, `parameters` *defaults,
skipped when empty*. `ConfigValue` is `Marker`, `Bool`, `Integer(i64)`,
`Str`, `Name`, `Path`, `FString` (text) or `Dict([(String, ConfigValue)])`.
`Guarantee` is `SchemaAgreement`, `EncodingAgreement`, `EncodingConformance`,
`VersionHonesty` or `BoundedSize`.

### Types and values

`TypeRef` is `Primitive(PrimitiveType)`, `Named(String)`,
`Array { element, length?: u64, max?: u64 }` (`max` *defaults, skipped when
absent*), `Union([TypeRef])` or `Unit`. `PrimitiveType` is `S8`, `S16`, `S32`,
`S64`, `U8`, `U16`, `U32`, `U64`, `F32`, `F64`, `Bool`, `String`, `U128`,
`S128` or `Char`.

`Literal` is `Integer(i64)`, `Bool`, `String` or `Reference(String)`.
`Condition` is `{ first: Comparison, rest: [(LogicalOp, Comparison)] }`, a
`Comparison` is `{ left, op, right }` with `Operand` sides (`Path([String])`,
`Integer(i64)` or `String`), `LogicalOp` is `And` or `Or`, and `CompareOp` is
`Eq`, `Ne`, `Ge`, `Le`, `Gt` or `Lt`.

`SettingsDict` is a map from a key to a `SettingsValue` (`Bool`,
`Integer(i64)`, `Str`, `Identifier` or `Dict(SettingsDict)`), ordered by key.
`SettingsSpans` is a map from a dotted path to a span, ordered by path.

## Config units

A `config` blob is a list of `ConfigUnit`, in the order the config was frozen:

| Variant | Fields |
|---|---|
| `Namespace(String)`, `PackageId(String)`, `PackageVersion(String)`, `SchemaPath(String)` | |
| `SpecificationVersion(u8)` | the language edition the package chose, not this format |
| `Dependency` | `author`, `project`, `version` |
| `CodeGeneration` | `name`: a language and its version |
| `PublishRegistry((name, PublishRegistry))` | `kind` (`LocalStorage` or `RegistryServer`), `uri` |
| `Settings(SettingsDict)` | |
| `Handshake(String)`, `HandshakeParams(map)` | |
| `Wire` | `format?`, `framing?` |
| `Encoding` | `name`, `kind` (`format`, `framing` or `handshake`), `profile_hash`, `map: { name: { type, package?, version? } }`, `establishes: [Guarantee]` and `has_vectors` (*defaults*), `grammar?`, `calls?`, `directions?` (*defaults, skipped when absent*) |
| `Unverified(ConfigValue)` | |

## The `docs` blob

`FrozenDocs { package?, modules }`: the `//!` header of `config.idp`, and each
own module's by its path. Both *default, skipped when empty*; a module without
a header isn't listed.

## The `facts` blob

`History { protocols, retired, numbered }`, `retired` *defaults, skipped when
empty* and `numbered` *defaults* to `false`:

- `ProtocolHistory { path, line, number, functions, retired, encodings }`:
  `number` *defaults* to 0; `retired` and `encodings` *default, skipped when
  empty*.
- `FunctionFacts { name, since, number }`: `number` *defaults* to 0.
- `Retired { number: u16, name }`.
- `EncodingFacts { kind, name, params: map, param_hash }`.

It is a record of what the build computed under the rules in force, not an
input: a core that reads it may recompute it, and does when `numbered` is
false.

## Reserved entries

Two root-tree entries exist in the format, and nothing writes them yet. They
are here so that the readers fail closed before any step or build writes one
(`codec::decode_unknown`, `codec::decode_produced_by`, and core's
`require_known`).

`unknown`: CBOR of `[UnknownRecord]`, each `{ path, step, reason, original? }`.
`path` is a JSON pointer into the version's model, `step` the step that
wrote it as `<from>-<to>`, `reason` why the format it came from could not know
it, and `original` the bytes that format recorded there, if it recorded any.
A version with a non-empty list has parts nobody recorded: a reader that can't
say what they are refuses the version.

`produced_by`: CBOR of `ProducedBy { core, spec }`: the `comline-core`
version, and the specification version of the catalog it was built against.
A version without it reads as written by an unknown core and spec, never as
the reading ones.

Both records refuse a field this copy of the format doesn't have.

## Absence readings and the ledger

Format 3 gained twenty-two additions without a bump. The rule that allowed it
was that a new entry, a new field with a default, or a new variant needs none,
and it is the ledger that makes it checked: a store holds commits written by
cores at different rows, so "format 3" doesn't say which of the rows below a
version has, and each row says what the version before it reads as. **Without a
bump, the only changes allowed are**

- a new entry in a root or dependency tree whose absence reads as unknown;
- a new field with `#[serde(default)]` and `skip_serializing_if` for the
  default, so that content which doesn't use it keeps its bytes;
- a new enum variant, if nothing existing needs it.

Each is a row of [`ledger.toml`](ledger.toml), the lines in `types.rs`, and the
text here, in one change, and a change to what a build writes for a package
that doesn't use it moves the golden hashes
(`tests/migrations/expected/`), which the gate then requires a row for.
Any other change is a bump.

| Row | Commit | Date | Added | A version written before it reads as |
|---|---|---|---|---|
| 3.01 | 0983015 | 2026-10-18 | validator call `span`, `arg_spans` | unknown span, `(0, 0)` |
| 3.02 | 8243d98 | 2026-10-18 | type alias `parameters` | none |
| 3.03 | ad2b3f5 | 2026-10-19 | `Assert.message_span`, `uninterpolated`; validator `validate_span` | none |
| 3.04 | bd9d9f4 | 2026-10-20 | `Unverified(Vec<Guarantee>)` in a protocol's parameters; the `Guarantee` enum | no recorded set |
| 3.05 | af0371f | 2026-10-20 | `Commit.release` | a release only if the version has no pre-release part |
| 3.06 | 6571780 | 2026-10-20 | config units `Handshake`, `HandshakeParams` | the default handshake of the reading core |
| 3.07 | 031d74c | 2026-10-25 | config unit `Wire` | the default format and framing of the reading core |
| 3.08 | a5d6f85 | 2026-10-25 | `ConfigValue::Dict` | a variant older readers can't decode |
| 3.09 | f643d51 | 2026-10-31 | config unit `Encoding` | no declaration |
| 3.10 | 038591e | 2026-11-01 | root `docs` blob | no `//!` headers |
| 3.11 | f873d98 | 2026-11-01 | root `facts` blob; config unit `PackageId` | computed when read; no id |
| 3.12 | 4ba7c57 | 2026-11-01 | `Encoding.establishes`, `has_vectors` | the strict reading: nothing established, no vectors |
| 3.13 | 4eb6228 | 2026-11-02 | `PrimitiveType::Char`, `TypeRef::Array.max` | a variant older readers can't decode; no maximum |
| 3.14 | 2b6d636 | 2026-11-02 | `Guarantee::BoundedSize` | a variant older readers can't decode |
| 3.15 | 8ec27df | 2026-11-03 | facts' `number`, `retired`, `numbered` | `numbered = false`: computed when read |
| 3.16 | 4da05ab | 2026-11-07 | `Encoding.grammar`, `calls`, `directions` | none |
| 3.17 | bbbd957, 9a6482e | 2026-11-08 | facts' per-protocol `encodings`; the defaults moved to `datagram@3` and `comline@2` | resolved when read, with what an absent value was (`IMPLICIT_FRAMING`, `IMPLICIT_HANDSHAKE`) |
| 3.18 | 9ea142e | 2026-11-10 | config unit `Unverified` | none |
| 3.19 | 965e781 | 2026-11-12 | `dep_<name>/config`, `dependency_configs` | unseen: not empty, not defaulted |
| 3.20 | 66feecb | 2026-11-12 | `FrozenArgument.parameters` (markers on arguments) | none |
| 3.21 | core#268, step 1.2 | 2026-11-15 | root `unknown` blob: what a step could not know, with the original (reserved: nothing writes it) | none: the version has no unknowns. A reader refuses a version that has one |
| 3.22 | core#268, step 1.2 | 2026-11-15 | root `produced_by` blob: the core and specification versions that wrote the version (reserved: nothing writes it) | unknown: not the reading core's, not defaulted |
