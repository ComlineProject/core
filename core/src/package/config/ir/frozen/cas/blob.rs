// Frozen congregation <-> CAS blob.
//
// The whole frozen config (a handful of `FrozenUnit`s) is small, so it is
// stored as one blob per version rather than a unit-per-blob subtree like
// schemas. The blob is added to the commit's root tree under the name `config`.

use crate::package::build::cas::object_store::ObjectStore;
use crate::package::build::cas::objects::Blob;
use crate::package::build::cas::storage::Hash;
use crate::package::config::ir::frozen::FrozenUnit;

use eyre::{eyre, Result};

/// Serialize the frozen congregation into a blob and store it, returning its hash.
pub fn write_config(units: &[FrozenUnit], store: &ObjectStore) -> Result<Hash> {
    let bytes =
        bincode::serialize(units).map_err(|e| eyre!("Failed to serialize frozen config: {e}"))?;
    let blob = Blob::new(bytes);
    store.write(&blob.to_bytes()?)
}

/// Load a frozen congregation blob back from the store.
pub fn read_config(store: &ObjectStore, hash: &Hash) -> Result<Vec<FrozenUnit>> {
    let blob = Blob::from_bytes(&store.read(hash)?)?;
    bincode::deserialize(&blob.content)
        .map_err(|e| eyre!("Failed to deserialize frozen config: {e}"))
}
