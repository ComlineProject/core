// Relative Modules
pub mod cas;  // CAS module (public for tests)

// Standard Uses
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

// Crate Uses
use crate::package::config::idl::constants::CONGREGATION_EXTENSION;
use crate::package::config::ir::interpreter::ProjectInterpreter;
use crate::package::config::ir::{
    compiler,
    context::ProjectContext,
};
use crate::schema::idl::constants::SCHEMA_EXTENSION;
use crate::schema::ir::{
    context::SchemaContext, diff::SchemaChanges,
};

// External Uses
use eyre::{bail, eyre, Result};

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
    let config_path = package_path.join(format!("config.{}", CONGREGATION_EXTENSION));
    let config_name = config_path.file_name().unwrap().to_str().unwrap();

    if !config_path.exists() {
        bail!(
            "Package at '{}' has no configuration file '{}'",
            package_path.display(),
            config_name
        )
    }

    let mut latest_project = ProjectInterpreter::from_origin(&config_path)?;
    interpret_schemas(&mut latest_project, package_path)?;

    Ok(latest_project)
}

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
        self.schemas
            .push((namespace.into_iter().map(Into::into).collect(), source.into()));
        self
    }

    /// Parse, interpret and validate. The returned [`ProjectContext`] has
    /// `config_frozen` set and a `SchemaContext` per schema, ready for codegen.
    pub fn compile(self) -> Result<ProjectContext> {
        let config = self.config.unwrap_or_else(default_congregation);

        let mut context = ProjectInterpreter::from_config_source(&config)?;
        context.config_frozen = Some(
            crate::package::config::ir::interpreter::interpret::interpret_context(&context)
                .map_err(|e| eyre!("{:?}", e))?,
        );

        interpret_schema_sources(&mut context, &self.schemas)?;
        Ok(context)
    }
}

/// A minimal congregation for the "just paste a schema" case — enough to
/// interpret schemas and generate `rust`.
fn default_congregation() -> String {
    "congregation playground\n\
     specification_version = 1\n\
     \n\
     code_generation = {\n    languages = {\n        rust#1.70.0 = {}\n    }\n}\n"
        .to_string()
}

/// Builds the package, which step-by-step means:
/// - Compile configuration and schemas
/// - Freeze the results into CAS (immutable storage)
/// - Generate code for targets (optional)
/// - Document changes (optional)
pub fn build(package_path: &Path) -> Result<BuildResult> {
    let latest_project = compile_package(package_path)?;

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
        config_changes: build_info.config_changes,
        version_bump: build_info.version_bump,
        context: latest_project,
    })
}

/// Glob `<package>/src/**/*.<ext>`, read each schema, and hand the
/// `(namespace segments, source)` pairs to [`interpret_schema_sources`]. The
/// namespace is the file's path under `src/`, extension dropped.
fn interpret_schemas(context: &mut ProjectContext, package_path: &Path) -> Result<()> {
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

        let relative = schema_path.strip_prefix(&schemas_path)?;
        let namespace = relative
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        let source = std::fs::read_to_string(&schema_path)?;
        sources.push((namespace, source));
    }

    interpret_schema_sources(context, &sources)
}

/// Parse each `(namespace segments, source)`, register a `SchemaContext` on
/// `context`, then run the project-aware interpretation + validation pass.
/// Filesystem-free; shared by [`compile_package`] and [`PackageSources`].
fn interpret_schema_sources(
    context: &mut ProjectContext,
    schemas: &[(Vec<String>, String)],
) -> Result<()> {
    for (namespace, source) in schemas {
        let name = format!("{}.{}", namespace.join("/"), SCHEMA_EXTENSION);

        let mut codemap = crate::utils::codemap::CodeMap::new();
        codemap.insert_file(name.clone(), source.clone());

        match crate::schema::idl::grammar::parse(source) {
            Ok(document) => {
                let schema_ctx =
                    SchemaContext::with_declarations(document.0, namespace.clone(), codemap);
                context.add_schema_context(Rc::new(RefCell::new(schema_ctx)));
            }
            Err(e) => bail!("Failed to parse schema '{}': {:?}", name, e),
        }
    }

    compiler::interpret::interpret_context(context)
}

// Removed: freeze_project_auto() - no longer needed with CAS
// CAS automatically handles freezing via process_initial_freezing/process_changes

pub struct BuildOptions {}

/// Re-export VersionBump from CAS for public API
pub use cas::VersionBump;

/// Result of a successful build operation
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// The version before this build (None if initial build)
    pub previous_version: Option<String>,
    /// The version after this build
    pub current_version: String,
    /// Detected schema changes (None if initial build)
    pub schema_changes: Option<SchemaChanges>,
    /// Detected congregation changes that moved the version (empty if none / initial build)
    pub config_changes: crate::package::config::ir::diff::ConfigChanges,
    /// The type of version bump applied
    pub version_bump: VersionBump,
    /// The underlying project context
    pub context: ProjectContext,
}

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
