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
use super::storage::Hash;
use super::version::{dev_version, next_release, VersionBump};
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
    /// The bump this build's own changes call for, against the build before.
    pub version_bump: VersionBump,
    /// The bump everything since the last release calls for: what
    /// `comline release` would apply now.
    pub release_bump: VersionBump,
    /// The last release, if there is one.
    pub last_release: Option<String>,
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

/// What changed between two stored trees, and the bump that calls for.
#[derive(Debug)]
pub struct Comparison {
    pub schema_changes: SchemaChanges,
    /// Changes inside dependencies present in both trees, per dependency.
    pub dependency_changes: Vec<crate::schema::ir::diff::DependencyChanges>,
    pub config_changes: ConfigChanges,
    pub bump: VersionBump,
}

/// Compare two root trees from the store: the package's own schema files
/// (paired by namespace), every dependency vendored into both, and the
/// congregation.
pub fn compare(store: &ObjectStore, old: &Tree, new: &Tree) -> Result<Comparison> {
    use crate::schema::ir::diff::{
        analyze_package_changes_with, reached_by, within_reach, DependencyChanges, WireFacts,
    };
    use crate::schema::ir::frozen::cas::blob::load_schema_from_tree;
    use std::collections::{BTreeMap, BTreeSet};

    // `schema_{idx}` entries only: a `dep_<name>` entry is a vendored
    // dependency's own copy, kept so the commit is reproducible.
    let local = |tree: &Tree| -> Result<Vec<_>> {
        let mut schemas = vec![];
        for entry in &tree.entries {
            if entry.mode == EntryMode::Tree && entry.name.starts_with("schema_") {
                let schema_tree = Tree::from_bytes(&store.read(&entry.hash)?)?;
                schemas.push(load_schema_from_tree(store, &schema_tree)?);
            }
        }
        Ok(schemas)
    };
    let config = |tree: &Tree| -> Result<Vec<_>> {
        Ok(tree
            .entries
            .iter()
            .find(|e| e.name == "config" && e.mode == EntryMode::Blob)
            .map(|e| config_blob::read_config(store, &e.hash))
            .transpose()?
            .unwrap_or_default())
    };
    let (old_config, new_config) = (config(old)?, config(new)?);

    // A package's compiled schemas include every dependency's, each under
    // the dependency's name (`shared_types::models`): those are split off
    // and compared by reach below.
    let dependency_names: BTreeSet<&str> = old
        .entries
        .iter()
        .chain(&new.entries)
        .filter(|e| e.mode == EntryMode::Tree)
        .filter_map(|e| e.name.strip_prefix("dep_"))
        .collect();
    type Schemas = Vec<Vec<crate::schema::ir::frozen::unit::FrozenUnit>>;
    let split = |schemas: Schemas| -> (Schemas, BTreeMap<String, Schemas>) {
        let mut own = vec![];
        let mut dependencies: BTreeMap<String, Schemas> = BTreeMap::new();
        for units in schemas {
            let first = crate::schema::ir::frozen::unit::schema_namespace(&units)
                .and_then(|ns| ns.split_once("::").map(|(first, _)| first))
                .filter(|first| dependency_names.contains(first))
                .map(str::to_string);
            match first {
                Some(dependency) => dependencies.entry(dependency).or_default().push(units),
                None => own.push(units),
            }
        }
        (own, dependencies)
    };
    let (old_own, old_dependencies) = split(local(old)?);
    let (new_own, new_dependencies) = split(local(new)?);

    // Files are paired by namespace, not by their position in the glob:
    // adding, renaming or reordering a file must not pair the wrong schemas.
    // The format-dependent rules read the facts of the encodings in play.
    let facts = WireFacts::of((&old_own, &old_config), (&new_own, &new_config));
    let schema_changes = analyze_package_changes_with(&old_own, &new_own, &facts);
    let mut bump = changes_bump(&schema_changes);

    // A dependency counts only as far as this package reaches into it: the
    // types its own declarations use or re-export, and what those use,
    // compared with the same rules (under the dependency's own protocols and
    // core's defaults: its config isn't stored). A change anywhere else in
    // it doesn't move the version, and adding or removing one is a patch
    // (`analyze_config_changes`).
    let mut dependency_changes = vec![];
    let names: BTreeSet<&String> = old_dependencies.keys().chain(new_dependencies.keys()).collect();
    for name in names {
        let before = old_dependencies.get(name).cloned().unwrap_or_default();
        let after = new_dependencies.get(name).cloned().unwrap_or_default();
        let reach = reached_by(&[&old_own, &new_own], &[&before, &after]);
        let (before, after) = (within_reach(&before, &reach), within_reach(&after, &reach));
        let facts = WireFacts::of((&before, &[]), (&after, &[]));
        let changes = analyze_package_changes_with(&before, &after, &facts);
        bump = bump.max(changes_bump(&changes));
        if !changes.is_empty() {
            dependency_changes.push(DependencyChanges { name: name.to_string(), changes });
        }
    }

    // Congregation changes. Only units that affect the schema API count
    // (spec version, dependencies). A tree from before config was stored
    // has none.
    let config_changes = analyze_config_changes(&old_config, &new_config);
    bump = bump.max(config_changes.bump());

    Ok(Comparison { schema_changes, dependency_changes, config_changes, bump })
}

/// The last release at or before `from`, and how many builds come after it
/// up to and including `from`. `None` when the history has no release yet.
pub fn last_release(store: &ObjectStore, from: Hash) -> Result<(Option<(Hash, Commit)>, u64)> {
    let mut hash = from;
    let mut builds = 0;
    loop {
        let commit = Commit::from_bytes(&store.read(&hash)?)?;
        if commit.is_release() {
            return Ok((Some((hash, commit)), builds));
        }
        builds += 1;
        match commit.parents.first() {
            Some(parent) => hash = *parent,
            None => return Ok((None, builds)),
        }
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

    // The `//!` headers, so documenting this version has them (core#145).
    // Only stored when there are any, so a package without headers keeps
    // the tree it had.
    let docs = crate::doc::frozen_docs(latest_project);
    if !docs.is_empty() {
        let docs_hash = crate::doc::write_docs(&docs, &store)?;
        root_tree.add_entry(EntryMode::Blob, crate::doc::DOCS_ENTRY.to_string(), docs_hash);
    }

    // Write root tree
    let root_tree_bytes = root_tree.to_bytes()?;
    let root_tree_hash = store.write(&root_tree_bytes)?;

    // Create initial commit: a build leading to the first release, 0.1.0.
    let initial_version = dev_version(&next_release(None, VersionBump::None), 1).to_string();
    let commit = create_initial_commit(root_tree_hash, &initial_version);
    let commit_bytes = commit.to_bytes()?;
    let commit_hash = store.write(&commit_bytes)?;

    // Update main ref to point to this commit
    update_ref(project_path, main_ref(), &commit_hash)?;

    tracing::info!("CAS: Initial commit {} created", commit_hash);

    Ok(BuildInfo {
        version_bump: VersionBump::None,
        release_bump: VersionBump::None,
        last_release: None,
        previous_version: None,
        current_version: initial_version,
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

    for (idx, schema_ctx) in latest_project.schema_contexts.iter().enumerate() {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();

        if let Some(frozen_schema) = frozen_ref.as_ref() {
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
    // same as process_initial_freezing; `compare` diffs the vendored content.
    #[cfg(feature = "deps")]
    vendor_dependencies(project_path, latest_project, &store, &mut root_tree)?;

    // Record the frozen congregation (see process_initial_freezing). A config
    // change alters the root tree, so a rebuild that only touches config still
    // produces a new commit; version-bump semantics for config changes are a
    // follow-up (see the codegen output-config planning doc).
    if let Some(config) = latest_project.config_frozen.as_ref() {
        let config_hash = config_blob::write_config(config, &store)?;
        root_tree.add_entry(EntryMode::Blob, "config".to_string(), config_hash);
    }

    // The `//!` headers, so documenting this version has them (core#145).
    // Only stored when there are any, so a package without headers keeps
    // the tree it had.
    let docs = crate::doc::frozen_docs(latest_project);
    if !docs.is_empty() {
        let docs_hash = crate::doc::write_docs(&docs, &store)?;
        root_tree.add_entry(EntryMode::Blob, crate::doc::DOCS_ENTRY.to_string(), docs_hash);
    }

    // Check if tree changed
    let root_tree_bytes = root_tree.to_bytes()?;
    let root_tree_hash = store.write(&root_tree_bytes)?;

    if root_tree_hash == parent_commit.tree {
        // No changes
        tracing::debug!("CAS: No changes detected");
        let (release, _) = last_release(&store, parent_hash)?;
        return Ok(BuildInfo {
            version_bump: VersionBump::None,
            release_bump: VersionBump::None,
            last_release: release.map(|(_, commit)| commit.version),
            previous_version: Some(parent_commit.version.clone()),
            current_version: parent_commit.version.clone(),
            schema_changes: None,
            dependency_changes: vec![],
            config_changes: ConfigChanges::default(),
        });
    }

    // What this build changed, against the one before it.
    let Comparison { schema_changes: all_changes, dependency_changes, config_changes, bump } =
        compare(&store, &prev_tree, &root_tree)?;
    let version_bump = bump;

    // Its version: the release it leads to - the last release bumped by
    // everything since - plus a count of builds since that release. A build
    // whose changes call for no bump is still a patch ahead, so it never
    // sorts before the release it follows.
    let (release, builds_since) = last_release(&store, parent_hash)?;
    let (release_bump, last_release_version) = match &release {
        Some((_, commit)) => {
            let release_tree = Tree::from_bytes(&store.read(&commit.tree)?)?;
            let bump = compare(&store, &release_tree, &root_tree)?.bump;
            (bump, Some(semver::Version::parse(&commit.version)?))
        }
        None => (VersionBump::None, None),
    };
    let target = next_release(last_release_version.as_ref(), release_bump.max(VersionBump::Patch));
    let new_version = dev_version(&target, builds_since + 1);

    // Create new commit
    let commit = create_version_commit(
        root_tree_hash,
        parent_hash,
        &new_version.to_string(),
        &format!("Build ({version_bump:?} change)"),
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
        release_bump,
        last_release: release.map(|(_, commit)| commit.version),
        previous_version: Some(parent_commit.version.clone()),
        current_version: new_version.to_string(),
        schema_changes: merged_changes,
        dependency_changes,
        config_changes,
    })
}
