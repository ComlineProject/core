//! Path ↔ namespace algebra for a package's schema tree. Pure, filesystem-
//! read-free (`Path` manipulation only) — safe for consumers that only see a
//! schema's path, never its package root on disk (the language server, via
//! an LSP `Url`).
//!
//! [`namespace_for_schema_path`] is the same body [`glob_schema_sources`]
//! used to compute inline before this module existed; it now calls this
//! instead of keeping its own copy.
//!
//! [`glob_schema_sources`]: crate::package::build::glob_schema_sources

use std::path::Path;

/// The conventional name of a package's schema root, relative to the
/// package root (`<package>/src/**/*.ids`).
pub const SCHEMAS_DIR: &str = "src";

/// The namespace segments for a schema file, given the `src/`-equivalent
/// root it lives under — the file's path relative to `schemas_root`, with
/// its extension dropped and path separators turned into segments. `None`
/// if `schema_path` isn't actually under `schemas_root`.
pub fn namespace_for_schema_path(schemas_root: &Path, schema_path: &Path) -> Option<Vec<String>> {
    let relative = schema_path.strip_prefix(schemas_root).ok()?;
    Some(
        relative
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect(),
    )
}

/// The nearest ancestor directory literally named [`SCHEMAS_DIR`] — a schema
/// file's namespace root. `None` if no ancestor is named `src` (a loose file
/// with no package structure around it).
pub fn schemas_root_for(schema_path: &Path) -> Option<&Path> {
    schema_path
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == SCHEMAS_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_for_nested_schema_path() {
        let ns = namespace_for_schema_path(
            Path::new("/pkg/src"),
            Path::new("/pkg/src/chat/admin.ids"),
        );
        assert_eq!(ns, Some(vec!["chat".to_string(), "admin".to_string()]));
    }

    #[test]
    fn namespace_strips_extension() {
        let ns = namespace_for_schema_path(Path::new("/pkg/src"), Path::new("/pkg/src/ping.ids"));
        assert_eq!(ns, Some(vec!["ping".to_string()]));
    }

    #[test]
    fn namespace_for_path_not_under_root_is_none() {
        let ns = namespace_for_schema_path(
            Path::new("/pkg/src"),
            Path::new("/elsewhere/ping.ids"),
        );
        assert_eq!(ns, None);
    }

    #[test]
    fn schemas_root_finds_nearest_src_ancestor() {
        let root = schemas_root_for(Path::new("/pkg/src/chat/admin.ids"));
        assert_eq!(root, Some(Path::new("/pkg/src")));
    }

    #[test]
    fn schemas_root_with_two_src_components_picks_the_nearest() {
        // A vendored dependency nested under another package's `src/` - the
        // file's own namespace root is the inner `src`, not the outer one.
        let root = schemas_root_for(Path::new("/pkg/src/vendor/dep/src/types.ids"));
        assert_eq!(root, Some(Path::new("/pkg/src/vendor/dep/src")));
    }

    #[test]
    fn schemas_root_for_loose_file_is_none() {
        let root = schemas_root_for(Path::new("/tmp/scratch.ids"));
        assert_eq!(root, None);
    }
}
