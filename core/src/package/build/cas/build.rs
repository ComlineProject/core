// CAS-based build process implementation
// Mirrors basic_storage but uses content-addressable storage
//
// Build Process: Append-Only Commit Chain
// - Each build creates a new commit pointing to previous
// - History is never modified or deleted
// - refs/heads/main always moves forward, never rewinds

use super::objects::{Commit, Tree, EntryMode};
use super::object_store::ObjectStore;
use super::refs::{main_ref, ref_exists, read_ref, update_ref};
use super::version::VersionBump;
use crate::package::config::ir::context::ProjectContext;
use crate::schema::ir::diff::SchemaChanges;
use crate::schema::ir::frozen::cas::blob::build_tree_from_schema;
use crate::schema::ir::frozen::cas::commit::{create_initial_commit, create_version_commit};
use eyre::Result;
use std::path::Path;

/// Information returned from build processing
pub struct BuildInfo {
    pub version_bump: VersionBump,
    pub previous_version: Option<String>,
    pub current_version: String,
    pub schema_changes: Option<SchemaChanges>,
}

/// Process initial freezing using CAS (first build)
pub fn process_initial_freezing(
    project_path: &Path,
    latest_project: &ProjectContext,
) -> Result<BuildInfo> {
    tracing::debug!("CAS: Processing initial freezing");
    
    let store = ObjectStore::new(project_path);
    store.init()?;

    // Build tree from schemas
    let mut root_tree = Tree::new();
    
    for (idx, schema_ctx) in latest_project.schema_contexts.iter().enumerate() {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        
        if let Some(frozen_schema) = frozen_ref.as_ref() {
            // Build subtree for this schema
            let schema_tree = build_tree_from_schema(frozen_schema, &store)?;
            let tree_bytes = schema_tree.to_bytes()?;
            let tree_hash = store.write(&tree_bytes)?;
            
            // Use index as name since path field doesn't exist
            let name = format!("schema_{}", idx);
            
            root_tree.add_entry(EntryMode::Tree, name, tree_hash);
        }
    }
    
    // Write root tree
    let root_tree_bytes = root_tree.to_bytes()?;
    let root_tree_hash = store.write(&root_tree_bytes)?;
    
    // Create initial commit
    let initial_version = "0.0.1";
    let commit = create_initial_commit(root_tree_hash, initial_version);
    let commit_bytes = commit.to_bytes()?;
    let commit_hash = store.write(&commit_bytes)?;
    
    // Update main ref to point to this commit
    update_ref(project_path, main_ref(), &commit_hash)?;
    
    tracing::info!("CAS: Initial commit {} created", commit_hash);
    
    Ok(BuildInfo {
        version_bump: VersionBump::None,
        previous_version: None,
        current_version: initial_version.to_string(),
        schema_changes: None,
    })
}

/// Process changes using CAS (subsequent builds)
pub fn process_changes(
    project_path: &Path,
    latest_project: &ProjectContext,
) -> Result<BuildInfo> {
    tracing::debug!("CAS: Processing changes");
    
    let store = ObjectStore::new(project_path);
    store.init()?;
    
    // Read previous commit
    if !ref_exists(project_path, main_ref()) {
        // No previous commit, treat as initial
        return process_initial_freezing(project_path, latest_project);
    }
    
    let parent_hash = read_ref(project_path, main_ref())?;
    let parent_bytes = store.read(&parent_hash)?;
    let parent_commit = Commit::from_bytes(&parent_bytes)?;
    
    // Load previous schema from parent commit's tree
    let prev_tree_bytes = store.read(&parent_commit.tree)?;
    let prev_tree = Tree::from_bytes(&prev_tree_bytes)?;
    
    // Build new tree from current schemas
    let mut root_tree = Tree::new();
    let mut current_schemas = vec![];
    
    for (idx, schema_ctx) in latest_project.schema_contexts.iter().enumerate() {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        
        if let Some(frozen_schema) = frozen_ref.as_ref() {
            current_schemas.push(frozen_schema.clone());
            
            // Build subtree for this schema
            let schema_tree = build_tree_from_schema(frozen_schema, &store)?;
            let tree_bytes = schema_tree.to_bytes()?;
            let tree_hash = store.write(&tree_bytes)?;
            
            // Use index as name (stable across builds for same file set)
            let name = format!("schema_{}", idx);
            root_tree.add_entry(EntryMode::Tree, name, tree_hash);
        }
    }
    
    // Check if tree changed
    let root_tree_bytes = root_tree.to_bytes()?;
    let root_tree_hash = store.write(&root_tree_bytes)?;
    
    if root_tree_hash == parent_commit.tree {
        // No changes
        tracing::debug!("CAS: No changes detected");
        return Ok(BuildInfo {
            version_bump: VersionBump::None,
            previous_version: Some(parent_commit.version.clone()),
            current_version: parent_commit.version.clone(),
            schema_changes: None,
        });
    }
    
    // Analyze schema changes using proper multi-file diffing
    use crate::schema::ir::diff::{analyze_schema_changes, NewFeature, BreakingChange};
    use crate::schema::ir::frozen::cas::blob::load_schema_from_tree;
    use crate::schema::ir::frozen::unit::FrozenUnit;
    
    let mut aggregated_bump = VersionBump::None;
    let mut all_changes = SchemaChanges::default();
    
    // Load all previous schemas
    let mut prev_schemas = vec![];
    for entry in &prev_tree.entries {
        if entry.mode == EntryMode::Tree {
            let prev_schema_tree_bytes = store.read(&entry.hash)?;
            let prev_schema_tree = Tree::from_bytes(&prev_schema_tree_bytes)?;
            let prev_schema = load_schema_from_tree(&store, &prev_schema_tree)?;
            prev_schemas.push(prev_schema);
        }
    }
    
    let prev_count = prev_schemas.len();
    let current_count = current_schemas.len();
    
    // 1. Compare schemas that exist in both (min of the two counts)
    let common_count = prev_count.min(current_count);
    for idx in 0..common_count {
        let file_changes = analyze_schema_changes(&prev_schemas[idx], &current_schemas[idx]);
        
        let schema_bump = if file_changes.is_breaking() {
            VersionBump::Major
        } else if file_changes.is_feature() {
            VersionBump::Minor
        } else if !file_changes.modifications.is_empty() {
            VersionBump::Patch
        } else {
            VersionBump::None
        };
        
        aggregated_bump = aggregated_bump.max(schema_bump);
        all_changes.breaking_changes.extend(file_changes.breaking_changes);
        all_changes.new_features.extend(file_changes.new_features);
        all_changes.modifications.extend(file_changes.modifications);
    }
    
    // 2. Handle NEW schemas (current_count > prev_count)
    if current_count > prev_count {
        tracing::debug!("New schema files detected: {}", current_count - prev_count);
        
        for idx in prev_count..current_count {
            // All declarations in new files are new features
            for unit in &current_schemas[idx] {
                match unit {
                    FrozenUnit::Struct { name, fields, .. } => {
                        all_changes.new_features.push(NewFeature::AddedStruct {
                            name: name.clone(),
                            field_count: fields.len(),
                        });
                    }
                    FrozenUnit::Enum { name, variants, .. } => {
                        all_changes.new_features.push(NewFeature::AddedEnum {
                            name: name.clone(),
                            variant_count: variants.len(),
                        });
                    }
                    FrozenUnit::Protocol { name, functions, .. } => {
                        all_changes.new_features.push(NewFeature::AddedProtocol {
                            name: name.clone(),
                            function_count: functions.len(),
                        });
                    }
                    _ => {}
                }
            }
        }
        
        aggregated_bump = aggregated_bump.max(VersionBump::Minor);
    }
    
    // 3. Handle REMOVED schemas (prev_count > current_count)
    if prev_count > current_count {
        tracing::debug!("Schema files removed: {}", prev_count - current_count);
        
        for idx in current_count..prev_count {
            // All declarations in removed files are breaking changes
            for unit in &prev_schemas[idx] {
                match unit {
                    FrozenUnit::Struct { name, .. } => {
                        all_changes.breaking_changes.push(BreakingChange::RemovedStruct {
                            name: name.clone(),
                        });
                    }
                    FrozenUnit::Enum { name, .. } => {
                        all_changes.breaking_changes.push(BreakingChange::RemovedEnum {
                            name: name.clone(),
                        });
                    }
                    FrozenUnit::Protocol { name, .. } => {
                        all_changes.breaking_changes.push(BreakingChange::RemovedProtocol {
                            name: name.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
        
        aggregated_bump = VersionBump::Major;
    }
    
    let version_bump = aggregated_bump;
    
    // Parse and bump version
    let prev_version = semver::Version::parse(&parent_commit.version)?;
    let new_version = match version_bump {
        VersionBump::Major => semver::Version::new(prev_version.major + 1, 0, 0),
        VersionBump::Minor => semver::Version::new(prev_version.major, prev_version.minor + 1, 0),
        VersionBump::Patch => semver::Version::new(prev_version.major, prev_version.minor, prev_version.patch + 1),
        VersionBump::None => prev_version.clone(),
    };
    
    // Create new commit
    let commit = create_version_commit(
        root_tree_hash,
        parent_hash,
        &new_version.to_string(),
        &format!("{:?} version bump", version_bump),
    );
    let commit_bytes = commit.to_bytes()?;
    let commit_hash = store.write(&commit_bytes)?;
    
    // Update main ref
    update_ref(project_path, main_ref(), &commit_hash)?;
    
    tracing::info!("CAS: New commit {} created ({})", commit_hash, new_version);
    
    // Return aggregated changes
    let merged_changes = if all_changes.is_empty() {
        None
    } else {
        Some(all_changes)
    };
    
    Ok(BuildInfo {
        version_bump,
        previous_version: Some(parent_commit.version.clone()),
        current_version: new_version.to_string(),
        schema_changes: merged_changes,
    })
}
