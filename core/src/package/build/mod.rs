// Relative Modules
pub mod cas;  // CAS module (public for tests)

// Standard Uses
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

// Crate Uses
use crate::codelib_gen::{find_generator, GeneratorFn};
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
use eyre::{bail, Result};
use handlebars::{Handlebars, RenderError};
use serde_derive::{Deserialize, Serialize};

/// Builds the package, which step-by-step means:
/// - Compile configuration and schemas
/// - Freeze the results into CAS (immutable storage)
/// - Generate code for targets (optional)
/// - Document changes (optional)
pub fn build(package_path: &Path) -> Result<BuildResult> {
    let config_path = package_path.join(format!("config.{}", CONGREGATION_EXTENSION));
    let config_name = config_path.file_name().unwrap().to_str().unwrap();

    if !config_path.exists() {
        bail!(
            "Package at '{}' has no configuration file '{}'",
            package_path.display(),
            config_name
        )
    }

    let latest_project = ProjectInterpreter::from_origin(&config_path)?;

    unsafe {
        interpret_schemas(&latest_project, package_path)?;
    }

    // Use CAS for immutable version storage
    let build_info = if cas::refs::ref_exists(package_path, cas::refs::main_ref()) {
        cas::build::process_changes(&package_path, &latest_project)?
    } else {
        cas::build::process_initial_freezing(&package_path, &latest_project)?
    };

    // generate_code_for_targets(&latest_project, project_path)?;

    Ok(BuildResult {
        previous_version: build_info.previous_version,
        current_version: build_info.current_version,
        schema_changes: build_info.schema_changes,
        version_bump: build_info.version_bump,
        context: latest_project,
    })
}

/// Safety: This assumes caller handles mutability properly
unsafe fn interpret_schemas(compiled_project: &ProjectContext, package_path: &Path) -> Result<()> {
    // TODO: Decide if package configurations should be able to change the source of schemas
    //       and/or how to look for them
    /*
    let schema_paths = frozen_project::schema_paths(
        compiled_project.config_frozen.as_ref().unwrap()
    );
    */
    let schemas_path = format!("{}/src/", package_path.display());
    let schemas_path = Path::new(&*schemas_path);
    let mut schema_paths = vec![];

    let pattern = format!("{}/**/*.{}", schemas_path.display(), SCHEMA_EXTENSION);
    for result in glob::glob(&*pattern)? {
        let schema_path = result?;
        if !schema_path.is_file() {
            bail!(
                "Expected a schema file but got a directory at '{}'",
                schema_path.display()
            )
        }
        let relative_path = schema_path.strip_prefix(schemas_path)?.to_path_buf();

        let parts = relative_path
            .with_extension("")
            .components()
            .map(|c| format!("{}", c.as_os_str().to_str().unwrap()))
            .collect::<Vec<_>>();

        schema_paths.push((relative_path, parts));
    }

    for relative in schema_paths {
        let concrete_path = schemas_path.join(relative.0);

        let source = std::fs::read_to_string(&concrete_path)?;

        // Initialize CodeMap for error reporting
        let mut codemap = crate::utils::codemap::CodeMap::new();
        codemap.insert_file(concrete_path.to_string_lossy().to_string(), source.clone());

        match crate::schema::idl::grammar::parse(&source) {
            Ok(document) => {
                let context = SchemaContext::with_declarations(document.0, relative.1, codemap);
                unsafe {
                    let ptr = compiled_project as *const ProjectContext;
                    let ptr_mut = ptr as *mut ProjectContext;
                    (*ptr_mut).add_schema_context(Rc::new(RefCell::new(context)));
                }
            }
            Err(e) => {
                bail!(
                    "Failed to parse schema at {}: {:?}",
                    concrete_path.display(),
                    e
                );
            }
        }
    }

    compiler::interpret::interpret_context(compiled_project)
}

// Removed: freeze_project_auto() - no longer needed with CAS
// CAS automatically handles freezing via process_initial_freezing/process_changes

#[allow(unused)]
fn generate_code_for_targets(compiled_project: &ProjectContext, base_path: &Path) -> Result<()> {
    use crate::package::config::ir::frozen::FrozenUnit;

    for item in compiled_project.config_frozen.as_ref().unwrap().iter() {
        if let FrozenUnit::CodeGeneration(details) = item {
            let Some((name, version)) = details.name.split_once('#') else {
                panic!()
            };

            let args = Args {
                default_path: "generated/{{language}}/{{version}}".to_owned(),
                language: name.to_owned(),
                version: version.to_owned(),
            };

            let path = resolve_path_query(&details.generation_path, args).unwrap();
            let path = base_path.join(path);

            let Some((gen_fn, extension)) = find_generator(name, version) else {
                panic!(
                    "No generator found for language named '{}' with version '{}'",
                    name, version
                )
            };

            generate_code_for_context(compiled_project, gen_fn, extension, &path)?;
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct Args {
    default_path: String,
    language: String,
    version: String,
}

pub fn resolve_path_query(query: &Option<String>, args: Args) -> Result<String, RenderError> {
    let mut reg = Handlebars::new();
    reg.set_strict_mode(true);

    if query.is_some() {
        reg.render_template(&query.clone().unwrap(), &args)
    } else {
        reg.render_template(&args.default_path, &args)
    }
}

pub fn generate_code_for_context(
    context: &ProjectContext,
    generator: &GeneratorFn,
    extension: &str,
    target_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(target_path)?;

    for schema_context in context.schema_contexts.iter() {
        let schema_ctx = schema_context.borrow();
        let frozen_schema_opt = schema_ctx.frozen_schema.borrow();
        let frozen_schema = frozen_schema_opt.as_ref().unwrap();
        let file_path =
            target_path.join(format!("{}.{}", &schema_ctx.namespace.join("/"), extension));

        let code = &*generator(frozen_schema);

        std::fs::write(file_path, code).unwrap();
    }

    Ok(())
}

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
