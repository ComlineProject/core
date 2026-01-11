// Schema integration utilities for CAS
// Converts FrozenUnits to CAS blobs and builds trees

use super::objects::{Blob, Tree, TreeEntry, EntryMode};
use super::object_store::ObjectStore;
use super::storage::Hash;
use crate::schema::ir::frozen::unit::FrozenUnit;
use eyre::{eyre, Result};

/// Convert a FrozenUnit to a Blob
pub fn frozen_unit_to_blob(unit: &FrozenUnit) -> Result<Blob> {
    // Serialize FrozenUnit using bincode
    let serialized = bincode::serialize(unit)
        .map_err(|e| eyre!("Failed to serialize FrozenUnit: {}", e))?;
    
    Ok(Blob::new(serialized))
}

/// Deserialize a Blob back to FrozenUnit
pub fn blob_to_frozen_unit(blob: &Blob) -> Result<FrozenUnit> {
    bincode::deserialize(&blob.content)
        .map_err(|e| eyre!("Failed to deserialize FrozenUnit from blob: {}", e))
}

/// Build a tree from a collection of FrozenUnits
/// Each FrozenUnit becomes a blob entry in the tree
pub fn build_tree_from_schema(
    schema: &[FrozenUnit],
    store: &ObjectStore,
) -> Result<Tree> {
    let mut tree = Tree::new();
    
    for (index, unit) in schema.iter().enumerate() {
        // Convert unit to blob
        let blob = frozen_unit_to_blob(unit)?;
        let blob_bytes = blob.to_bytes()?;
        
        // Write blob to store and get hash
        let hash = store.write(&blob_bytes)?;
        
        // Generate a name for this entry
        // Use the unit's name if available, otherwise use index
        let name = get_unit_name(unit, index);
        
        tree.add_entry(EntryMode::Blob, name, hash);
    }
    
    Ok(tree)
}

/// Extract a meaningful name from a FrozenUnit
fn get_unit_name(unit: &FrozenUnit, index: usize) -> String {
    match unit {
        FrozenUnit::Namespace(ns) => format!("namespace_{}", ns),
        FrozenUnit::Struct { name, .. } => format!("struct_{}", name),
        FrozenUnit::Enum { name, .. } => format!("enum_{}", name),
        FrozenUnit::Protocol { name, .. } => format!("protocol_{}", name),
        FrozenUnit::Constant { name, .. } => format!("const_{}", name),
        FrozenUnit::Import { .. } => format!("import_{}", index),
    }
}

/// Load a FrozenUnit from the store by its hash
pub fn load_frozen_unit_from_store(store: &ObjectStore, hash: &Hash) -> Result<FrozenUnit> {
    // Read blob bytes from store
    let blob_bytes = store.read(hash)?;
    
    // Deserialize blob
    let blob = Blob::from_bytes(&blob_bytes)?;
    
    // Convert blob to FrozenUnit
    blob_to_frozen_unit(&blob)
}

/// Load an entire schema from a tree
pub fn load_schema_from_tree(store: &ObjectStore, tree: &Tree) -> Result<Vec<FrozenUnit>> {
    let mut schema = Vec::new();
    
    for entry in &tree.entries {
        if entry.mode == EntryMode::Blob {
            let unit = load_frozen_unit_from_store(store, &entry.hash)?;
            schema.push(unit);
        }
    }
    
    Ok(schema)
}
