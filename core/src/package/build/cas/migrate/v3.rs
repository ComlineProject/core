// IR format 3 as the engine reads and writes it: the root tree of a version,
// decoded into the frozen types of `comline-ir-formats` and encoded back to
// the same bytes. It is the format this comline writes, so a format-3 store
// has an empty plan, and what `comline store verify` decodes a store with.
//
// The payload keeps the tree's shape (names and order of every entry), so
// that encoding a decoded version gives the bytes it came from; an entry this
// comline has no role for is kept as it is (`Content::Opaque`), because the
// ledger lets a newer build add entries a reader skips, and a migration does
// not drop what it does not understand.

use comline_ir_formats::v3::codec;
use comline_ir_formats::v3::entry;
use comline_ir_formats::v3::types::{
    ConfigUnit, FrozenDocs, History, ProducedBy, SchemaUnit, UnknownRecord,
};
use eyre::{eyre, Result};

use super::super::objects::{EntryMode, Tree};
use super::super::storage::Hash;
use super::format::{Format, Objects, Sink};

/// Format 3.
#[derive(Debug, Clone, Copy, Default)]
pub struct V3;

/// One version of a format-3 store: its root tree's entries, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Payload {
    pub entries: Vec<(String, Node)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Dir(Vec<(String, Node)>),
    Blob(Content),
}

/// A blob's content, decoded by the role its place in the tree gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// One frozen unit of a `schema_<n>` tree.
    Unit(SchemaUnit),
    /// A `config` blob: the package's or a dependency's.
    Config(Vec<ConfigUnit>),
    Docs(FrozenDocs),
    Facts(History),
    Unknown(Vec<UnknownRecord>),
    ProducedBy(ProducedBy),
    /// A dependency's `dependency_configs`.
    DependencyConfigs(Vec<(String, Vec<ConfigUnit>)>),
    /// A dependency's `manifest`: text.
    Manifest(Vec<u8>),
    /// A blob this comline has no role for, kept as it is.
    Opaque(Vec<u8>),
}

#[derive(Clone, Copy)]
enum Where {
    Root,
    Schema,
    Dependency,
    Opaque,
}

fn decode_blob(place: Where, name: &str, bytes: Vec<u8>) -> Result<Content> {
    Ok(match (place, name) {
        (Where::Schema, _) => Content::Unit(codec::decode_unit(&bytes)?),
        (Where::Root, entry::CONFIG) | (Where::Dependency, entry::DEPENDENCY_CONFIG) => {
            Content::Config(codec::decode_config(&bytes)?)
        }
        (Where::Root, entry::DOCS) => Content::Docs(codec::decode_docs(&bytes)?),
        (Where::Root, entry::FACTS) => Content::Facts(codec::decode_history(&bytes)?),
        (Where::Root, entry::UNKNOWN) => Content::Unknown(codec::decode_unknown(&bytes)?),
        (Where::Root, entry::PRODUCED_BY) => {
            Content::ProducedBy(codec::decode_produced_by(&bytes)?)
        }
        (Where::Dependency, entry::DEPENDENCY_CONFIGS) => {
            Content::DependencyConfigs(codec::decode("dependency configs", &bytes)?)
        }
        (Where::Dependency, entry::MANIFEST) => Content::Manifest(bytes),
        _ => Content::Opaque(bytes),
    })
}

fn decode_entries(objects: &dyn Objects, tree: &Tree, place: Where) -> Result<Vec<(String, Node)>> {
    let mut decoded = Vec::with_capacity(tree.entries.len());
    for entry_ in &tree.entries {
        let name = &entry_.name;
        let node = match entry_.mode {
            EntryMode::Tree => {
                let inside = match place {
                    Where::Root if name.starts_with(entry::SCHEMA_PREFIX) => Where::Schema,
                    Where::Root if name.starts_with(entry::DEPENDENCY_PREFIX) => Where::Dependency,
                    Where::Dependency if name.starts_with(entry::SCHEMA_PREFIX) => Where::Schema,
                    _ => Where::Opaque,
                };
                let subtree = objects.tree(&entry_.hash)?;
                Node::Dir(
                    decode_entries(objects, &subtree, inside).map_err(|e| eyre!("`{name}`/{e}"))?,
                )
            }
            EntryMode::Blob => {
                let bytes = objects.blob(&entry_.hash)?;
                Node::Blob(decode_blob(place, name, bytes).map_err(|e| eyre!("`{name}`: {e}"))?)
            }
        };
        decoded.push((name.clone(), node));
    }
    Ok(decoded)
}

fn encode_content(content: &Content) -> Result<Vec<u8>> {
    Ok(match content {
        Content::Unit(value) => codec::encode(value)?,
        Content::Config(value) => codec::encode(value)?,
        Content::Docs(value) => codec::encode(value)?,
        Content::Facts(value) => codec::encode(value)?,
        Content::Unknown(value) => codec::encode(value)?,
        Content::ProducedBy(value) => codec::encode(value)?,
        Content::DependencyConfigs(value) => codec::encode(value)?,
        Content::Manifest(bytes) | Content::Opaque(bytes) => bytes.clone(),
    })
}

fn encode_entries(entries: &[(String, Node)], sink: &mut dyn Sink) -> Result<Hash> {
    let mut tree = Tree::new();
    for (name, node) in entries {
        match node {
            Node::Dir(inside) => {
                let hash = encode_entries(inside, sink)?;
                tree.add_entry(EntryMode::Tree, name.clone(), hash);
            }
            Node::Blob(content) => {
                let hash = sink.blob(encode_content(content)?)?;
                tree.add_entry(EntryMode::Blob, name.clone(), hash);
            }
        }
    }
    sink.tree(&tree)
}

impl Format for V3 {
    type Payload = Payload;

    fn number(&self) -> u32 {
        comline_ir_formats::v3::FORMAT
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<Payload> {
        let tree = objects.tree(root)?;
        Ok(Payload {
            entries: decode_entries(objects, &tree, Where::Root)?,
        })
    }

    fn encode(&self, payload: &Payload, sink: &mut dyn Sink) -> Result<Hash> {
        encode_entries(&payload.entries, sink)
    }

    fn unknowns(&self, payload: &Payload) -> Vec<UnknownRecord> {
        payload
            .entries
            .iter()
            .find_map(|(name, node)| match node {
                Node::Blob(Content::Unknown(records)) if name == entry::UNKNOWN => {
                    Some(records.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    }
}
