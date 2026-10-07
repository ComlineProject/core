// Relative Modules
pub mod interpret;
pub mod freezing;

// Standard Uses
#[cfg(feature = "deps")]
use std::collections::HashSet;
use std::path::Path;
#[cfg(feature = "deps")]
use std::path::PathBuf;

// Crate Uses

// use crate::package::config::idl::parser_new;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
// use crate::schema::idl::ast::unit::*;
// use crate::schema::idl::grammar::Declaration;
use crate::package::config::ir::compiler::Compile;
use crate::package::config::idl::grammar::Congregation;

// External Uses
use eyre::Result;


#[allow(unused)]
pub struct ProjectInterpreter {
    context: ProjectContext
}

// Trait Implementation
impl Compile for ProjectInterpreter {
    type Output = Result<ProjectContext>;

    fn from_congregation(congregation: Congregation) -> Self::Output {
        // Use the existing logic (currently inside generic methods, we might need to expose a helper)
        // Actually, ProjectInterpreter::from_config_source called grammar::parse then logic.
        // We probably want to perform the logic here.
        
        // However, freezing/interpreting logic is tied to Context creation.
        // Let's refactor: ProjectContext::with_config(congregation) does the work.
        
        let context = ProjectContext::with_config(congregation);
        
        // TODO: Is there more interpretation needed here? 
        // interpret_context(&context)?; // This was in from_config_source
        
        crate::package::config::ir::interpreter::interpret::interpret_context(&context, None)
            .map_err(|e| eyre::eyre!("{}", e))?;

        Ok(context)
    }

    fn from_source(source: &str) -> Self::Output {
        Self::from_config_source(source)
    }

    fn from_origin(origin: &Path) -> Self::Output {
        Self::from_origin(origin) // Call the inherent method which handles file reading + parsing
    }
}

// Non-trait method
// Non-trait method
impl ProjectInterpreter {
    pub fn from_config_source(source: &str) -> Result<ProjectContext> {
        let congregation = crate::package::config::idl::grammar::parse(source).map_err(|e| {
            let diagnostic = crate::diagnostics::find_unclosed_bracket(source)
                .unwrap_or_else(|| crate::diagnostics::from_parse_errors(&e));
            eyre::eyre!(
                "Failed to parse config.idp:\n\n{}",
                crate::diagnostics::render(&diagnostic, "config.idp", source)
            )
        })?;

        Ok(ProjectContext::with_config(congregation))
    }

    pub fn from_origin(origin: &Path) -> Result<ProjectContext> {
        #[cfg(feature = "deps")]
        {
            Self::from_origin_with(origin, &mut HashSet::new())
        }
        #[cfg(not(feature = "deps"))]
        {
            Self::from_origin_inner(origin)
        }
    }

    /// [`from_origin`], threading the dependency-cycle-guard set a
    /// cross-package `settings = <dep>::settings::<name>` reference's
    /// resolution needs — see
    /// `crate::package::config::ir::interpreter::interpret::resolve_cross_package_settings`.
    #[cfg(feature = "deps")]
    pub(crate) fn from_origin_with(
        origin: &Path,
        in_progress: &mut HashSet<PathBuf>,
    ) -> Result<ProjectContext> {
        let project_root = origin.parent().unwrap_or_else(|| Path::new("."));
        let mut context = Self::read_and_parse(origin)?;

        let cross_package_override =
            interpret::resolve_cross_package_settings(&context, project_root, in_progress)
                .map_err(|e| eyre::eyre!("{}", e))?;

        context.config_frozen = Some(
            interpret::interpret_context(&context, cross_package_override.as_ref())
                .map_err(|e| eyre::eyre!("{}", e))?,
        );

        Ok(context)
    }

    #[cfg(not(feature = "deps"))]
    fn from_origin_inner(origin: &Path) -> Result<ProjectContext> {
        let mut context = Self::read_and_parse(origin)?;
        context.config_frozen = Some(
            interpret::interpret_context(&context, None).map_err(|e| eyre::eyre!("{}", e))?,
        );
        Ok(context)
    }

    /// Read `origin` and parse it into a [`ProjectContext`] with its
    /// `origin`/`config` set — not yet interpreted (`config_frozen` is
    /// still `None`).
    fn read_and_parse(origin: &Path) -> Result<ProjectContext> {
        let source = std::fs::read_to_string(origin)
            .map_err(|e| eyre::eyre!("Failed to read file {:?}: {}", origin, e))?;

        let mut context = Self::from_config_source(&source)?;
        // Update origin since from_config_source sets generic Virtual origin
        context.origin = crate::package::config::ir::context::Origin::Disk(origin.to_path_buf());
        Ok(context)
    }
}

