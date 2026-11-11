//! An import the build can't check is never taken as fine: a type behind it
//! would count as bounded without anyone having looked at it (core#248,
//! core#249, core#250).

use std::path::Path;

use comline_core::package::build::compile_package;

fn write_package(dir: &Path, name: &str, dependencies: &str, schemas: &[(&str, &str)]) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("config.idp"),
        format!("congregation {name}\nspecification_version = 1\n{dependencies}"),
    )
    .unwrap();
    for (file, source) in schemas {
        let path = dir.join("src").join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
}

const DEPENDS_ON_SHARED: &str = "dependencies = { shared = { path = \"../shared\" } }\n";
const USES_BIG: &str =
    "use shared::models::Big\nstruct W { b: Big }\nprotocol P { function g(w: W); }\n";

fn error_of(package: &Path) -> String {
    format!(
        "{:?}",
        compile_package(package).expect_err("the build should refuse this")
    )
}

/// A build without `deps` never merges a dependency, so `Big` would not
/// resolve and would count as bounded (core#248).
#[cfg(not(feature = "deps"))]
#[test]
fn a_package_with_dependencies_is_refused_without_the_deps_feature() {
    let tmp = tempfile::tempdir().unwrap();
    write_package(
        &tmp.path().join("shared"),
        "shared",
        "",
        &[("models.ids", "struct Big { s: string }\n")],
    );
    write_package(
        &tmp.path().join("app"),
        "app",
        DEPENDS_ON_SHARED,
        &[("api.ids", USES_BIG)],
    );

    let error = error_of(&tmp.path().join("app"));
    assert!(error.contains("declares dependencies (shared)"), "{error}");
    assert!(error.contains("`deps` feature"), "{error}");
}

/// Even one that imports nothing from them: the package can't be checked as
/// it is declared.
#[cfg(not(feature = "deps"))]
#[test]
fn the_in_memory_build_refuses_declared_dependencies_too() {
    let error = comline_core::package::build::PackageSources::new()
        .config(format!(
            "congregation app\nspecification_version = 1\n{DEPENDS_ON_SHARED}"
        ))
        .schema(["api"], USES_BIG)
        .compile()
        .expect_err("declared dependencies can't be resolved without `deps`");
    assert!(format!("{error:?}").contains("`deps` feature"), "{error:?}");
}

/// A dependency with no schemas gives `use shared::models::Big` nothing to
/// resolve against (core#249).
#[cfg(feature = "deps")]
#[test]
fn a_dependency_with_no_schemas_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    write_package(&tmp.path().join("shared"), "shared", "", &[]);
    write_package(
        &tmp.path().join("app"),
        "app",
        DEPENDS_ON_SHARED,
        &[("api.ids", USES_BIG)],
    );

    let error = error_of(&tmp.path().join("app"));
    assert!(error.contains("dependency 'shared': no schemas"), "{error}");
}

/// A dependency that is declared but whose schemas never reached the build
/// (here: sources given in memory, which resolve no dependencies) leaves the
/// import unresolved rather than skipped (core#249).
#[cfg(feature = "deps")]
#[test]
fn an_import_of_a_dependency_that_was_not_merged_is_unresolved() {
    let error = comline_core::package::build::PackageSources::new()
        .config(format!(
            "congregation app\nspecification_version = 1\n{DEPENDS_ON_SHARED}"
        ))
        .schema(["api"], USES_BIG)
        .compile()
        .expect_err("nothing merged the dependency, so its import can't be checked");
    assert!(
        format!("{error:?}").contains("Unresolved import"),
        "{error:?}"
    );
}

/// `src/shared/mine.ids` would be taken for the dependency's own schema, and
/// its `use nowhere::Big` never checked (core#250).
#[test]
fn an_own_module_named_like_a_dependency_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    write_package(
        &tmp.path().join("shared"),
        "shared",
        "",
        &[("models.ids", "struct T { id: u8 }\n")],
    );
    write_package(
        &tmp.path().join("app"),
        "app",
        DEPENDS_ON_SHARED,
        &[(
            "shared/mine.ids",
            "use nowhere::Big\nstruct W { b: Big }\nprotocol P { function g(w: W); }\n",
        )],
    );

    let error = error_of(&tmp.path().join("app"));
    assert!(
        error.contains("schema 'shared/mine.ids' is named like the dependency 'shared'"),
        "{error}"
    );
}

/// The same for `std`, which `check_imports` also takes for another package's
/// (core#250).
#[test]
fn an_own_module_named_std_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    write_package(
        tmp.path(),
        "app",
        "",
        &[(
            "std/mine.ids",
            "use nowhere::Big\nstruct W { b: Big }\nprotocol P { function g(w: W); }\n",
        )],
    );

    let error = error_of(tmp.path());
    assert!(
        error.contains("is named like the standard library"),
        "{error}"
    );
}
