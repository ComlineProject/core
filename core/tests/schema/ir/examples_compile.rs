// Compiles the schemas under `examples/` to make sure they stay in sync with
// the current grammar and compiler: standalone `.ids` files through the
// single-file path, and the `imports/` mini-package (which exercises
// cross-file `use` resolution) through the real project compile path.

use comline_core::schema::idl::constants::SCHEMA_EXTENSION;
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

#[test]
fn test_standalone_examples_compile() {
    for name in ["dev", "example", "greet", "simple", "test"] {
        let path = examples_dir().join(format!("{}.{}", name, SCHEMA_EXTENSION));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));

        let parsed = grammar::parse(&source);
        assert!(
            parsed.is_ok(),
            "examples/{}.{} failed to parse: {:?}",
            name,
            SCHEMA_EXTENSION,
            parsed.err()
        );

        // Should generate IR without panicking.
        let units = IncrementalInterpreter::from_source(&source);
        assert!(
            !units.is_empty(),
            "examples/{}.{} produced no IR units",
            name,
            SCHEMA_EXTENSION
        );
    }
}

#[test]
fn test_imports_package_compiles_with_resolved_cross_file_use() {
    // The real compile: the package's config is frozen first, so its
    // settings (the default bounds among them) apply.
    let project = comline_core::package::build::compile_package_without_fetching(&examples_dir().join("imports"))
        .expect("imports example package should compile");

    let hole = project
        .schema_contexts
        .iter()
        .find(|schema| schema.borrow().namespace_joined() == "hole")
        .expect("hole schema should be present");

    let frozen = hole
        .borrow()
        .frozen_schema
        .borrow()
        .clone()
        .expect("hole schema should be frozen");

    let import_paths: Vec<&str> = frozen
        .iter()
        .filter_map(|unit| match unit {
            FrozenUnit::Import(path, _, _) => Some(path.as_str()),
            _ => None,
        })
        .collect();

    for expected in [
        "ant",
        "food",
        "ant::Ant",
        "ant::Mood",
        "ant::CantCarryError",
        "food::Food",
        "food::State",
        "food::NotEdibleError",
    ] {
        assert!(
            import_paths.contains(&expected),
            "Expected resolved import '{}', got {:?}",
            expected,
            import_paths
        );
    }

    assert!(
        import_paths.iter().all(|path| !path.starts_with("<unresolved:")),
        "Expected all imports in hole.ids to resolve, got {:?}",
        import_paths
    );
}
