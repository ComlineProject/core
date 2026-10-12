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

/// Why a namespace segment can't be used, so the error can explain itself -
/// the two groups are refused for genuinely different reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedReason {
    /// A keyword of the IDL itself. A schema named `self` can't be named in
    /// a `use` at all - `use self::Thing` already means something else.
    Keyword,
    /// Legal Comline, broken output: a filename that means something
    /// particular to a target language's module system.
    ModuleName,
}

impl ReservedReason {
    /// A one-line `help:` explaining the refusal to whoever named the file.
    pub const fn help(self) -> &'static str {
        match self {
            ReservedReason::Keyword => {
                "rename it - this is a keyword of the schema language, so no \
                 `use` could ever name this schema"
            }
            ReservedReason::ModuleName => {
                "rename it - a schema name becomes a module name in generated \
                 code, and this one already means something there (`main` is a \
                 binary target in Rust, `lib` is the crate root, `index` is a \
                 directory's entry point in JavaScript, ...)"
            }
        }
    }
}

/// Names a target language's module system already spends on something else.
/// Short on purpose: each entry is a name that *breaks* generated output, not
/// merely one that reads oddly.
const RESERVED_MODULE_NAMES: &[&str] =
    &["crate", "index", "lib", "main", "mod", "super", "__init__", "__main__"];

/// Whether `segment` can be one segment of a schema's namespace, and if not,
/// why.
///
/// A schema's path becomes its namespace (`src/chat/admin.ids` ->
/// `chat::admin`), and that namespace becomes an identifier path both in
/// Comline and in every language generated from it. Some names don't survive
/// that trip: `src/lib.ids` generates `src/lib.rs`, overwriting the crate root
/// the Rust generator wrote there, and `src/main.ids` generates a file Cargo
/// treats as a binary target rather than a module.
///
/// Checked per segment, not per filename: `src/main/foo.ids` is `main::foo`,
/// which in Rust needs a `src/main.rs` module file next to `src/main/foo.rs` -
/// the same collision, one directory up.
pub fn reserved_segment(segment: &str) -> Option<ReservedReason> {
    use crate::schema::idl::vocabulary;

    // `vocabulary` is the canonical description of the surface syntax, so
    // the keyword half asks it rather than keeping a second list to drift
    // from - the same source `package::config::dependency`'s own reserved
    // check already consults. Primitives count too: `str.ids` would make
    // `use str::Thing` unwritable just as surely as `struct.ids` would.
    if vocabulary::keyword(segment).is_some() || vocabulary::primitive(segment).is_some() {
        Some(ReservedReason::Keyword)
    } else if RESERVED_MODULE_NAMES.contains(&segment) {
        Some(ReservedReason::ModuleName)
    } else {
        None
    }
}

/// The first segment of `namespace` that [`reserved_segment`] refuses, if any.
pub fn reserved_in_namespace(namespace: &[String]) -> Option<(&str, ReservedReason)> {
    namespace
        .iter()
        .find_map(|s| reserved_segment(s).map(|reason| (s.as_str(), reason)))
}

#[cfg(test)]
mod reserved_tests {
    use super::*;

    #[test]
    fn an_idl_keyword_is_refused_as_unnameable() {
        for keyword in ["self", "parent", "package", "use", "struct", "error", "type"] {
            assert_eq!(
                reserved_segment(keyword),
                Some(ReservedReason::Keyword),
                "{keyword} is a keyword of the IDL",
            );
        }
    }

    #[test]
    fn a_target_module_name_is_refused_for_what_it_breaks() {
        for name in ["main", "lib", "mod", "crate", "super", "index", "__init__"] {
            assert_eq!(
                reserved_segment(name),
                Some(ReservedReason::ModuleName),
                "{name} means something to a target's module system",
            );
        }
    }

    #[test]
    fn an_ordinary_name_is_fine() {
        for name in ["chat", "types", "greeting", "admin", "std", "main_api", "libs"] {
            assert_eq!(reserved_segment(name), None, "{name} should be usable");
        }
    }

    /// A directory segment is as much a module name as the filename is:
    /// `src/main/foo.ids` needs a `src/main.rs` beside `src/main/foo.rs`.
    #[test]
    fn a_directory_segment_counts_too() {
        let namespace = ["main".to_string(), "foo".to_string()];
        assert_eq!(
            reserved_in_namespace(&namespace),
            Some(("main", ReservedReason::ModuleName)),
        );
    }

    #[test]
    fn the_first_offending_segment_is_the_one_reported() {
        let namespace = ["chat".to_string(), "self".to_string(), "lib".to_string()];
        assert_eq!(
            reserved_in_namespace(&namespace),
            Some(("self", ReservedReason::Keyword)),
        );
    }

    #[test]
    fn a_clean_namespace_reports_nothing() {
        let namespace = ["chat".to_string(), "admin".to_string()];
        assert_eq!(reserved_in_namespace(&namespace), None);
    }
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
