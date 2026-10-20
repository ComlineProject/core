// CAS-based build process implementation
// Mirrors basic_storage but uses content-addressable storage
//
// Build Process: Append-Only Commit Chain
// - Each build creates a new commit pointing to previous
// - History is never modified or deleted
// - refs/heads/main always moves forward, never rewinds

// Standard Uses
use std::path::Path;

// Crate Uses
use super::object_store::ObjectStore;
use super::objects::{Commit, EntryMode, Tree};
use super::refs::{main_ref, read_ref, ref_exists, update_ref};
use super::version::VersionBump;
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::diff::{analyze_config_changes, ConfigChanges};
use crate::package::config::ir::frozen::cas::blob as config_blob;
use crate::schema::ir::diff::SchemaChanges;
use crate::schema::ir::frozen::cas::blob::build_tree_from_schema;
use crate::schema::ir::frozen::cas::commit::{create_initial_commit, create_version_commit};

// External Uses
use eyre::Result;

/// Information returned from build processing
pub struct BuildInfo {
    pub version_bump: VersionBump,
    pub previous_version: Option<String>,
    pub current_version: String,
    pub schema_changes: Option<SchemaChanges>,
    /// Changes inside dependencies present in both builds, per dependency.
    pub dependency_changes: Vec<crate::schema::ir::diff::DependencyChanges>,
    pub config_changes: ConfigChanges,
}

/// Vendor one resolved dependency's own frozen schema into the commit: a
/// `schema_{idx}` subtree per schema file in the dependency (same shape a
/// local package's schemas get), plus a `manifest` blob recording the
/// verified content hash. Returns the tree ready to be written and added to
/// the root tree under `dep_<name>`.
#[cfg(feature = "deps")]
fn build_dependency_tree(
    resolved: &crate::package::deps::ResolvedDependency,
    store: &ObjectStore,
) -> Result<Tree> {
    let mut dep_tree = Tree::new();

    for (idx, schema_ctx) in resolved.context.schema_contexts.iter().enumerate() {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        if let Some(frozen_schema) = frozen_ref.as_ref() {
            let schema_tree = build_tree_from_schema(frozen_schema, store)?;
            let tree_hash = store.write(&schema_tree.to_bytes()?)?;
            dep_tree.add_entry(EntryMode::Tree, format!("schema_{}", idx), tree_hash);
        }
    }

    let manifest_hash = store.write(
        &super::objects::Blob::new(resolved.content_hash.to_hex().into_bytes()).to_bytes()?,
    )?;
    dep_tree.add_entry(EntryMode::Blob, "manifest".to_string(), manifest_hash);

    Ok(dep_tree)
}

/// Resolve and vendor every declared dependency into `root_tree`, one
/// `dep_<name>` subtree each, returning the resolved dependencies so a
/// caller that also needs to *diff* them (see `process_changes`) doesn't
/// have to resolve everything a second time. A no-op (empty vec) when the
/// project declares none. Building without `feature = "deps"` at all skips
/// this entirely — the frozen `config` blob still carries each dependency's
/// declared identity/version either way (`interpret_assignment_dependencies`
/// doesn't need this feature), just not its vendored schema content.
#[cfg(feature = "deps")]
fn vendor_dependencies(
    project_path: &Path,
    latest_project: &ProjectContext,
    store: &ObjectStore,
    root_tree: &mut Tree,
) -> Result<Vec<crate::package::deps::ResolvedDependency>> {
    let mut in_progress = std::collections::HashSet::new();
    in_progress.insert(project_path.canonicalize().unwrap_or_else(|_| project_path.to_path_buf()));
    let resolved_deps =
        crate::package::deps::resolve_all(latest_project, project_path, &mut in_progress)?;
    for resolved in &resolved_deps {
        let dep_tree = build_dependency_tree(resolved, store)?;
        let tree_hash = store.write(&dep_tree.to_bytes()?)?;
        root_tree.add_entry(EntryMode::Tree, format!("dep_{}", resolved.name), tree_hash);
    }
    Ok(resolved_deps)
}

/// A vendored dependency's own schema files, one entry per file, for
/// `analyze_package_changes` to pair by namespace.
#[cfg(feature = "deps")]
fn load_dependency_schemas(
    store: &ObjectStore,
    dep_tree: &Tree,
) -> Result<Vec<Vec<crate::schema::ir::frozen::unit::FrozenUnit>>> {
    use crate::schema::ir::frozen::cas::blob::load_schema_from_tree;

    let mut schemas = Vec::new();
    for entry in &dep_tree.entries {
        if entry.mode == EntryMode::Tree && entry.name.starts_with("schema_") {
            let schema_tree = Tree::from_bytes(&store.read(&entry.hash)?)?;
            schemas.push(load_schema_from_tree(store, &schema_tree)?);
        }
    }
    Ok(schemas)
}

/// The bump a set of changes calls for on its own.
fn changes_bump(changes: &SchemaChanges) -> VersionBump {
    if changes.is_breaking() {
        VersionBump::Major
    } else if changes.is_feature() {
        VersionBump::Minor
    } else if !changes.modifications.is_empty() {
        VersionBump::Patch
    } else {
        VersionBump::None
    }
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

    // Vendor every declared dependency's own frozen schema (feature = "deps")
    // so this commit is reproducible on its own, without re-resolving them.
    #[cfg(feature = "deps")]
    vendor_dependencies(project_path, latest_project, &store, &mut root_tree)?;

    // Record the frozen congregation alongside the schemas so a version is
    // reproducible from the commit alone.
    if let Some(config) = latest_project.config_frozen.as_ref() {
        let config_hash = config_blob::write_config(config, &store)?;
        root_tree.add_entry(EntryMode::Blob, "config".to_string(), config_hash);
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
        dependency_changes: vec![],
        config_changes: ConfigChanges::default(),
    })
}

/// Process changes using CAS (subsequent builds)
pub fn process_changes(project_path: &Path, latest_project: &ProjectContext) -> Result<BuildInfo> {
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

    // Vendor every declared dependency's own frozen schema (feature = "deps"),
    // same as process_initial_freezing. Kept around (rather than discarded
    // like the initial-freezing call) so the diffing pass below can compare
    // against last build's vendored content without re-resolving everything.
    #[cfg(feature = "deps")]
    let resolved_deps = vendor_dependencies(project_path, latest_project, &store, &mut root_tree)?;

    // Record the frozen congregation (see process_initial_freezing). A config
    // change alters the root tree, so a rebuild that only touches config still
    // produces a new commit; version-bump semantics for config changes are a
    // follow-up (see the codegen output-config planning doc).
    if let Some(config) = latest_project.config_frozen.as_ref() {
        let config_hash = config_blob::write_config(config, &store)?;
        root_tree.add_entry(EntryMode::Blob, "config".to_string(), config_hash);
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
            dependency_changes: vec![],
            config_changes: ConfigChanges::default(),
        });
    }

    // Analyze schema changes, pairing schema files by namespace
    use crate::schema::ir::diff::{analyze_package_changes, DependencyChanges};
    use crate::schema::ir::frozen::cas::blob::load_schema_from_tree;

    // Load all previous *local* schemas — `schema_{idx}` entries only. A
    // `dep_<name>` entry is also `EntryMode::Tree` but is a vendored
    // dependency, not a local schema file; it gets its own diffing pass
    // below, keyed by name.
    let mut prev_schemas = vec![];
    for entry in &prev_tree.entries {
        if entry.mode == EntryMode::Tree && entry.name.starts_with("schema_") {
            let prev_schema_tree_bytes = store.read(&entry.hash)?;
            let prev_schema_tree = Tree::from_bytes(&prev_schema_tree_bytes)?;
            let prev_schema = load_schema_from_tree(&store, &prev_schema_tree)?;
            prev_schemas.push(prev_schema);
        }
    }

    // Files are paired by namespace, not by their position in the glob:
    // adding, renaming or reordering a file must not pair the wrong schemas.
    let all_changes = analyze_package_changes(&prev_schemas, &current_schemas);
    let mut aggregated_bump = changes_bump(&all_changes);

    // 3b. Dependency content changes (feature = "deps") — this is the piece
    // `ConfigChange::DependencyVersionChanged`'s doc comment calls out as
    // pending: "conservatively breaking until dependency resolution can diff
    // the two dependency schemas (core#6)". A dependency present in both the
    // parent commit and this build gets the *real* bump its own schema
    // changes imply, and its changes are listed, not only counted. A
    // brand-new or fully-removed dependency needs no extra signal here:
    // `analyze_config_changes`'s `DependencyAdded`/`DependencyRemoved`
    // (Minor/Major) already covers those from the frozen config alone.
    #[allow(unused_mut)]
    let mut dependency_changes: Vec<DependencyChanges> = vec![];
    #[cfg(feature = "deps")]
    for resolved in &resolved_deps {
        let entry_name = format!("dep_{}", resolved.name);
        let Some(prev_entry) = prev_tree
            .entries
            .iter()
            .find(|e| e.name == entry_name && e.mode == EntryMode::Tree)
        else {
            continue; // new dependency — DependencyAdded already covers it
        };

        let prev_dep_tree = Tree::from_bytes(&store.read(&prev_entry.hash)?)?;
        let prev_schemas = load_dependency_schemas(&store, &prev_dep_tree)?;
        let cur_schemas = load_dependency_schemas(
            &store,
            &Tree::from_bytes(
                &store.read(
                    &root_tree
                        .entries
                        .iter()
                        .find(|e| e.name == entry_name)
                        .expect("just vendored this dependency above")
                        .hash,
                )?,
            )?,
        )?;

        let changes = analyze_package_changes(&prev_schemas, &cur_schemas);
        aggregated_bump = aggregated_bump.max(changes_bump(&changes));
        if !changes.is_empty() {
            dependency_changes.push(DependencyChanges {
                name: resolved.name.clone(),
                changes,
            });
        }
    }

    // 4. Congregation changes. Only units that affect the schema API count
    //    (spec version, dependencies); code_generation / publish_registries /
    //    namespace are excluded. `prev_config` is empty for a commit that
    //    predates config being stored in CAS.
    let prev_config = prev_tree
        .entries
        .iter()
        .find(|e| e.name == "config" && e.mode == EntryMode::Blob)
        .map(|e| config_blob::read_config(&store, &e.hash))
        .transpose()?
        .unwrap_or_default();
    let cur_config = latest_project.config_frozen.as_deref().unwrap_or_default();
    let config_changes = analyze_config_changes(&prev_config, cur_config);
    aggregated_bump = aggregated_bump.max(config_changes.bump());

    let version_bump = aggregated_bump;

    // Parse and bump version
    let prev_version = semver::Version::parse(&parent_commit.version)?;
    let new_version = match version_bump {
        VersionBump::Major => semver::Version::new(prev_version.major + 1, 0, 0),
        VersionBump::Minor => semver::Version::new(prev_version.major, prev_version.minor + 1, 0),
        VersionBump::Patch => semver::Version::new(
            prev_version.major,
            prev_version.minor,
            prev_version.patch + 1,
        ),
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
        dependency_changes,
        config_changes,
    })
}
