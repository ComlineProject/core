// Relative Modules
pub mod cas; // CAS module (public for tests)

// Standard Uses
#[cfg(feature = "compiler")]
use std::cell::RefCell;
#[cfg(feature = "deps")]
use std::collections::HashSet;
#[cfg(feature = "compiler")]
use std::path::Path;
#[cfg(feature = "deps")]
use std::path::PathBuf;
#[cfg(feature = "compiler")]
use std::rc::Rc;

// Crate Uses
#[cfg(feature = "compiler")]
use crate::package::config::idl::constants::CONGREGATION_EXTENSION;
#[cfg(feature = "compiler")]
use crate::package::config::ir::interpreter::ProjectInterpreter;
#[cfg(feature = "compiler")]
use crate::package::config::ir::{compiler, context::ProjectContext};
#[cfg(feature = "compiler")]
use crate::schema::idl::constants::SCHEMA_EXTENSION;
#[cfg(feature = "compiler")]
use crate::schema::ir::{context::SchemaContext, diff::SchemaChanges};

// External Uses
#[cfg(feature = "compiler")]
use eyre::{bail, eyre, Result};

#[cfg(feature = "compiler")]
/// Compile and validate a package without touching CAS.
///
/// This is the front half of [`build`]: it reads `config.<ext>`, interprets the
/// project configuration, then parses, resolves and validates every schema under
/// `src/`. It performs **no** content-addressable-storage writes and does **not**
/// bump the package version, so it is the right entry point for a fast
/// validation-only "check" (editors, pre-commit hooks, CI lint steps).
///
/// On success the returned [`ProjectContext`] carries the frozen schema units for
/// every schema (see `SchemaContext::frozen_schema`), ready for code generation.
///
/// This is the on-disk entry point; [`PackageSources`] is the in-memory twin for
/// embedders (the playground) that never touch the filesystem. Both share the
/// same interpretation + validation pass.
pub fn compile_package(package_path: &Path) -> Result<ProjectContext> {
    #[cfg(feature = "deps")]
    {
        let mut in_progress = HashSet::new();
        in_progress.insert(canonical(package_path));
        compile_package_with(package_path, &mut in_progress)
    }
    #[cfg(not(feature = "deps"))]
    {
        compile_package_inner(package_path)
    }
}

#[cfg(feature = "compiler")]
/// [`compile_package`], resolving dependencies only from what's already on
/// disk (see [`crate::package::deps::without_fetching`]): nothing is
/// fetched, and nothing in a dependency cache is changed. Without the `deps`
/// feature, [`compile_package`] resolves no dependencies at all, so the two
/// are the same.
pub fn compile_package_without_fetching(package_path: &Path) -> Result<ProjectContext> {
    #[cfg(feature = "deps")]
    {
        crate::package::deps::without_fetching(|| compile_package(package_path))
    }
    #[cfg(not(feature = "deps"))]
    {
        compile_package(package_path)
    }
}

/// [`compile_package`], threading the dependency-cycle-guard set — shared
/// across every recursive dependency compile in one traversal (a cross-
/// package `settings` reference's resolution, and ordinary schema-level
/// dependency merging, both go through this same set). Callers are
/// responsible for having already inserted their own canonical path before
/// calling this (see [`crate::package::deps::resolve_with`], which does so
/// for every dependency it recurses into) — [`compile_package`] itself does
/// that for the top-level package being built.
#[cfg(feature = "deps")]
pub(crate) fn compile_package_with(
    package_path: &Path,
    in_progress: &mut HashSet<PathBuf>,
) -> Result<ProjectContext> {
    let config_path = require_config_path(package_path)?;
    let mut latest_project =
        ProjectInterpreter::from_origin_with(&config_path, in_progress)?;
    interpret_schemas(&mut latest_project, package_path, in_progress)?;
    Ok(latest_project)
}

#[cfg(feature = "compiler")]
#[cfg(not(feature = "deps"))]
fn compile_package_inner(package_path: &Path) -> Result<ProjectContext> {
    let config_path = require_config_path(package_path)?;
    let mut latest_project = ProjectInterpreter::from_origin(&config_path)?;
    interpret_schemas(&mut latest_project, package_path)?;
    Ok(latest_project)
}

#[cfg(feature = "compiler")]
fn require_config_path(package_path: &Path) -> Result<std::path::PathBuf> {
    let config_path = package_path.join(format!("config.{}", CONGREGATION_EXTENSION));
    let config_name = config_path.file_name().unwrap().to_str().unwrap();

    if !config_path.exists() {
        bail!(
            "Package at '{}' has no configuration file '{}'",
            package_path.display(),
            config_name
        )
    }

    Ok(config_path)
}

#[cfg(feature = "deps")]
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(feature = "compiler")]
/// Compile and validate a package from **in-memory sources** — no filesystem
/// access. This is what the playground and other embedders use; `core` reads no
/// files on this path.
///
/// ```ignore
/// let context = PackageSources::new()
///     .config(config_idp_source)           // optional; a minimal one is synthesised
///     .schema(["chat"], chat_schema_src)   // namespace segments + source
///     .schema(["chat", "admin"], admin_src)
///     .compile()?;
/// ```
///
/// The namespace segments are what the on-disk layout would derive from a
/// schema's path under `src/` — structure comes from layout (Rust-module style),
/// not from a keyword inside the file. Cross-schema `use` resolves across every
/// schema added here, exactly as on disk.
#[derive(Debug, Default)]
pub struct PackageSources {
    config: Option<String>,
    schemas: Vec<(Vec<String>, String)>,
}

#[cfg(feature = "compiler")]
impl PackageSources {
    pub fn new() -> Self {
        Self::default()
    }

    /// The `config.<ext>` (congregation) source. If never set, [`compile`] uses
    /// a minimal synthesised congregation.
    ///
    /// [`compile`]: Self::compile
    pub fn config(mut self, source: impl Into<String>) -> Self {
        self.config = Some(source.into());
        self
    }

    /// Add one schema: its namespace segments and its source.
    pub fn schema(
        mut self,
        namespace: impl IntoIterator<Item = impl Into<String>>,
        source: impl Into<String>,
    ) -> Self {
        self.schemas.push((
            namespace.into_iter().map(Into::into).collect(),
            source.into(),
        ));
        self
    }

    /// Parse, interpret and validate. The returned [`ProjectContext`] has
    /// `config_frozen` set and a `SchemaContext` per schema, ready for codegen.
    pub fn compile(self) -> Result<ProjectContext> {
        let config = self.config.unwrap_or_else(default_congregation);

        let mut context = ProjectInterpreter::from_config_source(&config)?;
        context.config_frozen = Some(
            crate::package::config::ir::interpreter::interpret::interpret_context(&context, None)
                .map_err(|e| eyre!("{}", e))?,
        );

        interpret_schema_sources(&mut context, &with_std(self.schemas))?;
        Ok(context)
    }
}

#[cfg(feature = "compiler")]
/// A minimal congregation for the "just paste a schema" case — enough to
/// interpret schemas and generate `rust`.
fn default_congregation() -> String {
    "congregation playground\n\
     specification_version = 1\n\
     \n\
     code_generation = {\n    languages = {\n        rust#1.85.0 = {}\n    }\n}\n"
        .to_string()
}

#[cfg(feature = "compiler")]
/// Builds the package, which step-by-step means:
/// - Compile configuration and schemas
/// - Freeze the results into CAS (immutable storage)
/// - Generate code for targets (optional)
/// - Document changes (optional)
pub fn build(package_path: &Path) -> Result<BuildResult> {
    build_with(package_path, |_| Ok(()))
}

#[cfg(feature = "compiler")]
/// [`build`], with `check` run on the compiled package before anything is
/// frozen: an error from it fails the build and leaves `.comline/`
/// untouched.
///
/// For what core can't judge itself. The CLI checks every target declared
/// under `code_generation.languages` against the generators it was built
/// with, since core knows no generators.
pub fn build_with(
    package_path: &Path,
    check: impl FnOnce(&ProjectContext) -> Result<()>,
) -> Result<BuildResult> {
    let latest_project = compile_package(package_path)?;
    check(&latest_project)?;

    // Discard any `.comline/` left behind by a different IR format before
    // trusting it for anything below - see `ensure_compatible_store`.
    cas::object_store::ensure_compatible_store(package_path)?;

    // Use CAS for immutable version storage
    let build_info = if cas::refs::ref_exists(package_path, cas::refs::main_ref()) {
        cas::build::process_changes(&package_path, &latest_project)?
    } else {
        cas::build::process_initial_freezing(&package_path, &latest_project)?
    };

    // Code generation is a consumer concern and lives entirely in the CLI now;
    // `core` only produces the compiled/frozen context.

    Ok(BuildResult {
        previous_version: build_info.previous_version,
        current_version: build_info.current_version,
        schema_changes: build_info.schema_changes,
        dependency_changes: build_info.dependency_changes,
        config_changes: build_info.config_changes,
        version_bump: build_info.version_bump,
        release_bump: build_info.release_bump,
        last_release: build_info.last_release,
        context: latest_project,
    })
}

#[cfg(feature = "compiler")]
/// Glob `<package>/src/**/*.<ext>` and read each schema into a
/// `(namespace segments, source)` pair. The namespace is the file's path
/// under `src/`, extension dropped — shared by local-package loading and (via
/// `package::deps::resolve`, prefixed by the dependency's declared name)
/// dependency loading.
pub(crate) fn glob_schema_sources(package_path: &Path) -> Result<Vec<(Vec<String>, String)>> {
    // TODO: Decide if package configurations should be able to change the source
    //       of schemas and/or how to look for them.
    let schemas_path = package_path.join("src");
    let pattern = format!("{}/**/*.{}", schemas_path.display(), SCHEMA_EXTENSION);

    let mut sources: Vec<(Vec<String>, String)> = Vec::new();
    for result in glob::glob(&pattern)? {
        let schema_path = result?;
        if !schema_path.is_file() {
            bail!(
                "Expected a schema file but got a directory at '{}'",
                schema_path.display()
            )
        }

        let namespace = crate::package::layout::namespace_for_schema_path(
            &schemas_path,
            &schema_path,
        )
        .ok_or_else(|| {
            eyre!(
                "schema path '{}' is not under schemas root '{}'",
                schema_path.display(),
                schemas_path.display()
            )
        })?;

        let source = std::fs::read_to_string(&schema_path)?;
        sources.push((namespace, source));
    }

    Ok(sources)
}

/// Local schemas, plus — behind `feature = "deps"` — every declared
/// dependency's own schemas as its own compile froze them, namespaced under
/// the dependency's name (`shared_types = { path = "../shared-types" }`'s
/// `foo.ids` becomes `shared_types::foo` here), plus the std schemas any of
/// them import (see [`with_std`]). Local schemas are interpreted; a
/// dependency's are already compiled and only resolved against, so
/// cross-package `use` works normally.
#[cfg(feature = "deps")]
fn interpret_schemas(
    context: &mut ProjectContext,
    package_path: &Path,
    in_progress: &mut HashSet<PathBuf>,
) -> Result<()> {
    let local = glob_schema_sources(package_path)?;
    let dependencies =
        crate::package::deps::resolve_dependency_schemas(context, package_path, in_progress)?;

    let mut all = local.clone();
    all.extend(dependencies.sources);
    let std = crate::package::stdlib::used_by(&all);

    context.dependency_configs = dependencies.configs;

    // Same order as before: local, dependencies, std.
    add_schema_sources(context, &local)?;
    for schema in dependencies.schemas {
        context.add_schema_context(Rc::new(RefCell::new(schema)));
    }
    add_schema_sources(context, &std)?;
    compiler::interpret::interpret_context(context)
}

#[cfg(feature = "compiler")]
#[cfg(not(feature = "deps"))]
fn interpret_schemas(context: &mut ProjectContext, package_path: &Path) -> Result<()> {
    let sources = glob_schema_sources(package_path)?;
    interpret_schema_sources(context, &with_std(sources))
}

#[cfg(feature = "compiler")]
/// `sources`, then the std schemas they import (`use std::…`), directly or
/// through each other, under the `std` namespace. Appended after them, so a
/// package's own schemas keep their order (and their CAS entry names).
fn with_std(mut sources: Vec<(Vec<String>, String)>) -> Vec<(Vec<String>, String)> {
    let std = crate::package::stdlib::used_by(&sources);
    sources.extend(std);
    sources
}

#[cfg(feature = "compiler")]
/// Parse each `(namespace segments, source)`, register a `SchemaContext` on
/// `context`, then run the project-aware interpretation + validation pass.
/// Filesystem-free; shared by [`compile_package`] and [`PackageSources`].
fn interpret_schema_sources(
    context: &mut ProjectContext,
    schemas: &[(Vec<String>, String)],
) -> Result<()> {
    add_schema_sources(context, schemas)?;
    compiler::interpret::interpret_context(context)
}

#[cfg(feature = "compiler")]
/// Parse each `(namespace segments, source)` and register a `SchemaContext`
/// for it on `context`, uninterpreted.
fn add_schema_sources(context: &mut ProjectContext, schemas: &[(Vec<String>, String)]) -> Result<()> {
    for (namespace, source) in schemas {
        let name = format!("{}.{}", namespace.join("/"), SCHEMA_EXTENSION);

        // Refused here, before anything is parsed, rather than at the
        // generator - where it is already too late to say anything useful.
        // `lib.ids` writes `src/lib.rs` straight over the crate root the Rust
        // generator put there, losing the whole module tree silently, and
        // `main.ids` writes a file Cargo treats as a binary target. Every
        // schema reaches this function whatever its origin (a globbed `src/`
        // tree, `PackageSources`, a dependency), so one check covers them all.
        if let Some((segment, reason)) =
            crate::package::layout::reserved_in_namespace(namespace)
        {
            bail!(
                "schema '{name}' uses the reserved name '{segment}'\n  help: {}",
                reason.help(),
            )
        }

        let mut codemap = crate::utils::codemap::CodeMap::new();
        codemap.insert_file(name.clone(), source.clone());

        match crate::schema::idl::grammar::parse(source) {
            Ok(document) => {
                let schema_ctx =
                    SchemaContext::with_declarations(document.0, namespace.clone(), codemap);
                context.add_schema_context(Rc::new(RefCell::new(schema_ctx)));
            }
            Err(e) => {
                let diagnostic = crate::diagnostics::from_schema_parse_failure(source, &e);
                bail!(
                    "Failed to parse schema '{}':\n\n{}",
                    name,
                    crate::diagnostics::render(&diagnostic, &name, source)
                )
            }
        }
    }
    Ok(())
}

// Removed: freeze_project_auto() - no longer needed with CAS
// CAS automatically handles freezing via process_initial_freezing/process_changes

#[cfg(feature = "compiler")]
pub struct BuildOptions {}

#[cfg(feature = "compiler")]
/// Re-export VersionBump from CAS for public API
pub use cas::VersionBump;
#[cfg(feature = "compiler")]
pub use cas::release::{latest_release, release, ReleaseInfo, ReleaseRequest};
#[cfg(feature = "compiler")]
pub use cas::objects::Release;

#[cfg(feature = "compiler")]
/// Result of a successful build operation
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// The version before this build (None if initial build)
    pub previous_version: Option<String>,
    /// The version after this build
    pub current_version: String,
    /// Detected schema changes (None if initial build)
    pub schema_changes: Option<SchemaChanges>,
    /// Changes inside dependencies present in both builds, per dependency
    /// (empty if none / initial build). Their bump is already part of
    /// `version_bump`.
    pub dependency_changes: Vec<crate::schema::ir::diff::DependencyChanges>,
    /// Detected congregation changes that moved the version (empty if none / initial build)
    pub config_changes: crate::package::config::ir::diff::ConfigChanges,
    /// The bump this build's own changes call for, against the build before
    pub version_bump: VersionBump,
    /// The bump everything since the last release calls for: what
    /// `comline release` would apply now
    pub release_bump: VersionBump,
    /// The last release, if there is one
    pub last_release: Option<String>,
    /// The underlying project context
    pub context: ProjectContext,
}

#[cfg(feature = "compiler")]
impl BuildResult {
    /// Get the version change as a formatted string (e.g., "0.1.0 → 0.2.0")
    pub fn version_change(&self) -> Option<String> {
        self.previous_version
            .as_ref()
            .map(|prev| format!("{} → {}", prev, self.current_version))
    }

    /// Check if this was an initial build (no previous version)
    pub fn is_initial_build(&self) -> bool {
        self.previous_version.is_none()
    }

    /// Check if the version changed
    pub fn version_changed(&self) -> bool {
        self.previous_version
            .as_ref()
            .map(|prev| prev != &self.current_version)
            .unwrap_or(false)
    }
}
