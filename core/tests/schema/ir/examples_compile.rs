// Compiles the schemas under `examples/` to make sure they stay in sync with
// the current grammar and compiler: standalone `.ids` files through the
// single-file path, and the `imports/` mini-package (which exercises
// cross-file `use` resolution) through the real project compile path.

use comline_core::package::config::idl::grammar as config_grammar;
use comline_core::package::config::ir::compiler::interpret::interpret_context;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::idl::constants::SCHEMA_EXTENSION;
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::context::SchemaContext;
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core::utils::codemap::CodeMap;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// Load a package directory (config.idp + src/**/*.ids) into a ProjectContext,
/// mirroring `package::build`'s private `interpret_schemas` glob-and-parse
/// logic, without touching CAS/.frozen state on disk.
fn load_package(package_path: &Path) -> ProjectContext {
    let config_source = std::fs::read_to_string(package_path.join("config.idp"))
        .expect("package should have a config.idp");
    let congregation =
        config_grammar::parse(&config_source).expect("package config should parse");
    let mut project = ProjectContext::with_config(congregation);

    let schemas_path = package_path.join("src");
    let pattern = format!(
        "{}/**/*.{}",
        schemas_path.display(),
        SCHEMA_EXTENSION
    );

    for entry in glob::glob(&pattern).expect("glob pattern should be valid") {
        let schema_path = entry.expect("glob entry should resolve");
        let relative = schema_path
            .strip_prefix(&schemas_path)
            .expect("schema path should be under src/")
            .with_extension("");
        let namespace: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_str().unwrap().to_string())
            .collect();

        let source = std::fs::read_to_string(&schema_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", schema_path.display(), e));

        let mut codemap = CodeMap::new();
        codemap.insert_file(schema_path.to_string_lossy().to_string(), source.clone());

        let document = grammar::parse(&source)
            .unwrap_or_else(|e| panic!("{} failed to parse: {:?}", schema_path.display(), e));

        let context = SchemaContext::with_declarations(document.0, namespace, codemap);
        project.schema_contexts.push(Rc::new(RefCell::new(context)));
    }

    project
}

#[test]
fn test_imports_package_compiles_with_resolved_cross_file_use() {
    let project = load_package(&examples_dir().join("imports"));

    interpret_context(&project).expect("imports example package should compile");

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
            FrozenUnit::Import(path) => Some(path.as_str()),
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
