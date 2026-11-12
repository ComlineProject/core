// Frozen congregation <-> CAS blob.
//
// The whole frozen config (a handful of `FrozenUnit`s) is small, so it is
// stored as one blob per version rather than a unit-per-blob subtree like
// schemas. The blob is added to the commit's root tree under the name `config`.

use crate::package::build::cas::object_store::ObjectStore;
use crate::package::build::cas::objects::{Blob, EntryMode, Tree};
use crate::package::build::cas::storage::{from_cbor, to_cbor, Hash};
use crate::package::config::ir::frozen::FrozenUnit;
use crate::schema::ir::unverified::Encodings;

use eyre::Result;

/// Serialize the frozen congregation into a blob and store it, returning its hash.
pub fn write_config(units: &[FrozenUnit], store: &ObjectStore) -> Result<Hash> {
    let blob = Blob::new(to_cbor(units)?);
    store.write(&blob.to_bytes()?)
}

/// Load a frozen congregation blob back from the store.
pub fn read_config(store: &ObjectStore, hash: &Hash) -> Result<Vec<FrozenUnit>> {
    let blob = Blob::from_bytes(&store.read(hash)?)?;
    from_cbor(&blob.content)
}

// ---------- a version's configs, own and its dependencies' ----------

/// The root tree entry holding the package's own frozen config.
pub const CONFIG_ENTRY: &str = "config";

/// A root tree's entries for vendored dependencies are `dep_<name>`.
pub const DEPENDENCY_PREFIX: &str = "dep_";

/// The `dep_<name>` entry holding the dependency's own frozen config
/// (core#251). Written on every build that vendors the dependency, even
/// where the config is empty, so that its absence means only "stored by a
/// build that didn't keep it".
pub const DEPENDENCY_CONFIG_ENTRY: &str = "config";

/// The `dep_<name>` entry holding the frozen configs of the dependency's own
/// dependencies, as a list of `(name, units)` in the order the compile saw
/// them. Written only where there are any.
pub const DEPENDENCY_CONFIGS_ENTRY: &str = "dependency_configs";

/// Store the frozen configs of a dependency's own dependencies as a blob,
/// for a `dep_<name>` tree's [`DEPENDENCY_CONFIGS_ENTRY`].
pub fn write_dependency_configs(
    configs: &[(String, Vec<FrozenUnit>)],
    store: &ObjectStore,
) -> Result<Hash> {
    let blob = Blob::new(to_cbor(configs)?);
    store.write(&blob.to_bytes()?)
}

/// Load a [`DEPENDENCY_CONFIGS_ENTRY`] blob back from the store.
pub fn read_dependency_configs(
    store: &ObjectStore,
    hash: &Hash,
) -> Result<Vec<(String, Vec<FrozenUnit>)>> {
    let blob = Blob::from_bytes(&store.read(hash)?)?;
    from_cbor(&blob.content)
}

/// The frozen configs a stored version holds: the package's own, and each
/// vendored dependency's (with the ones its own dependencies bring), in the
/// order a compile saw them, so that [`Encodings`] built from them is the one
/// [`Encodings::of`] builds from the live package.
///
/// A version stored before dependencies' configs were kept (core#251) has
/// none of theirs, and one from before configs were kept at all not even its
/// own. What isn't stored is **unseen**, not empty: [`complete`] says whether
/// anything is, and whatever judges a guarantee from these has to read a
/// name it can't find as unknown rather than as undeclared.
///
/// [`complete`]: StoredConfigs::complete
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredConfigs {
    /// The package's own frozen config; empty where it wasn't stored.
    pub own: Vec<FrozenUnit>,
    /// Whether the tree holds the package's own config.
    pub own_stored: bool,
    /// Each vendored dependency's config under its declared name, then its
    /// own dependencies', for each in turn.
    pub dependencies: Vec<(String, Vec<FrozenUnit>)>,
    /// The vendored dependencies whose config the tree doesn't hold.
    pub unstored: Vec<String>,
}

impl StoredConfigs {
    /// What the root tree `tree` holds.
    pub fn read(store: &ObjectStore, tree: &Tree) -> Result<Self> {
        let mut stored = StoredConfigs::default();
        if let Some(entry) = tree
            .entries
            .iter()
            .find(|e| e.name == CONFIG_ENTRY && e.mode == EntryMode::Blob)
        {
            stored.own = read_config(store, &entry.hash)?;
            stored.own_stored = true;
        }
        for entry in &tree.entries {
            let Some(name) = entry.name.strip_prefix(DEPENDENCY_PREFIX) else {
                continue;
            };
            if entry.mode != EntryMode::Tree {
                continue;
            }
            let dependency = Tree::from_bytes(&store.read(&entry.hash)?)?;
            let config = dependency
                .entries
                .iter()
                .find(|e| e.name == DEPENDENCY_CONFIG_ENTRY && e.mode == EntryMode::Blob);
            let Some(config) = config else {
                stored.unstored.push(name.to_string());
                continue;
            };
            stored
                .dependencies
                .push((name.to_string(), read_config(store, &config.hash)?));
            if let Some(theirs) = dependency
                .entries
                .iter()
                .find(|e| e.name == DEPENDENCY_CONFIGS_ENTRY && e.mode == EntryMode::Blob)
            {
                stored
                    .dependencies
                    .extend(read_dependency_configs(store, &theirs.hash)?);
            }
        }
        Ok(stored)
    }

    /// Whether the version holds every config there is to see: its own, and
    /// each vendored dependency's.
    pub fn complete(&self) -> bool {
        self.own_stored && self.unstored.is_empty()
    }

    /// The encoding declarations these configs make, the way
    /// [`Encodings::of`] makes them from a compiled package. Not
    /// [complete](Encodings::complete) where a config wasn't stored.
    pub fn encodings(&self) -> Encodings<'_> {
        Encodings::from_config(
            &self.own,
            self.dependencies.iter().map(|(_, units)| units.as_slice()),
            self.complete(),
        )
    }

    /// The stored config of the vendored dependency `name`, if there is one.
    pub fn dependency(&self, name: &str) -> Option<&[FrozenUnit]> {
        self.dependencies
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, units)| units.as_slice())
    }
}
