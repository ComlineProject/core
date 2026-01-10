use comline_core::package::build::build;
use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn setup_test_package(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Fixtures are in core/tests/fixtures
    let source = root.join("tests/fixtures/packages/test");
    let target = root.join("target/tmp").join(name);

    if target.exists() {
        fs::remove_dir_all(&target).expect("Failed to clean temp dir");
    }

    copy_dir_all(&source, &target).expect("Failed to copy test data");

    // Remove existing .frozen to ensure we test initial freezing
    let frozen = target.join(".frozen");
    if frozen.exists() {
        fs::remove_dir_all(&frozen).expect("Failed to clear .frozen");
    }

    target
}

#[test]
fn test_schema_lifecycle() {
    let package_path = setup_test_package("schema_lifecycle");

    // 1. Initial Build
    println!("Building initial version...");
    build(&package_path).expect("Initial build failed");

    // Verify 0.0.1 frozen (MINIMUM_VERSION)
    let frozen_version_path = package_path.join(".frozen/package/versions/0.0.1");
    assert!(frozen_version_path.exists(), "Version 0.0.1 not created");

    let ping_schema_path = frozen_version_path.join("schemas/ping");
    assert!(ping_schema_path.exists(), "Ping schema not frozen");

    // 2. Minor Bump: Add optional field
    println!("Applying Minor change...");
    let ping_file = package_path.join("src/ping.ids");
    let clean_src = fs::read_to_string(&ping_file).unwrap();

    // Inject optional field to Ping protocol (making it a Minor change if we supported Protocol ops,
    // but remember: Protocol changes are currently STRICT MAJOR in our implementation.
    // So let's add a NEW struct to trigger a Minor bump as per our strategy).

    let new_struct = "\n\nstruct NewFeature {\n    optional new_field: string\n}";
    let src_minor = clean_src.clone() + new_struct;
    fs::write(&ping_file, src_minor).unwrap();

    build(&package_path).expect("Minor update build failed");

    let version_0_1_0 = package_path.join(".frozen/package/versions/0.1.0");
    assert!(version_0_1_0.exists(), "Version 0.1.0 (Minor) not created");

    // 3. Major Bump: Remove the struct we just added
    println!("Applying Major change...");
    // Reverting to original source removes "NewFeature", which is a removal of a top-level item -> Major
    fs::write(&ping_file, clean_src).unwrap();

    build(&package_path).expect("Major update build failed");

    let version_1_0_0 = package_path.join(".frozen/package/versions/1.0.0");
    assert!(version_1_0_0.exists(), "Version 1.0.0 (Major) not created");
}
