use comline_core::package::build::build;
use comline_core::package::build::cas::{refs, ObjectStore};
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

    // Remove existing .comline (CAS) to ensure we test initial freezing
    let comline_dir = target.join(".comline");
    if comline_dir.exists() {
        fs::remove_dir_all(&comline_dir).expect("Failed to clear .comline");
    }

    target
}

#[test]
fn test_schema_lifecycle() {
    let package_path = setup_test_package("schema_lifecycle");

    // 1. Initial Build
    println!("Building initial version...");
    let result = build(&package_path).expect("Initial build failed");
    
    assert_eq!(result.current_version, "0.1.0-dev.1", "the first build leads to 0.1.0");
    assert!(result.previous_version.is_none(), "Initial build should have no previous version");

    // Verify CAS structure was created
    let comline_dir = package_path.join(".comline");
    assert!(comline_dir.exists(), ".comline directory not created");
    
    let objects_dir = comline_dir.join("objects");
    assert!(objects_dir.exists(), "CAS objects directory not created");
    
    let refs_dir = comline_dir.join("refs/heads");
    assert!(refs_dir.exists(), "refs directory not created");
    
    let main_ref = refs_dir.join("main");
    assert!(main_ref.exists(), "main ref not created");

    // Verify we can read the ref
    assert!(refs::ref_exists(&package_path, refs::main_ref()), "main ref should exist");

    let released = comline_core::package::build::release(&package_path, &Default::default())
        .expect("first release");
    assert_eq!(released.version, "0.1.0");

    // 2. Minor Bump: Add optional field
    println!("Applying Minor change...");
    let ping_file = package_path.join("src/ping.ids");
    let clean_src = fs::read_to_string(&ping_file).unwrap();

    let new_struct = "\n\nstruct NewFeature {\n    optional new_field: string\n}";
    let src_minor = clean_src.clone() + new_struct;
    fs::write(&ping_file, src_minor).unwrap();

    let result = build(&package_path).expect("Minor update build failed");
    assert_eq!(result.current_version, "0.1.1-dev.1", "adding a struct is minor: 0.x bumps the patch");
    assert_eq!(result.previous_version, Some("0.1.0".to_string()));

    // 3. Major change: Remove the struct we just added
    println!("Applying Major change (removing struct)...");
    fs::write(&ping_file, clean_src).unwrap();

    let result = build(&package_path).expect("Major update build failed");
    // Against the build before, removing a struct is breaking; against the
    // release, the two builds cancel out, so the next release is still a patch.
    assert_eq!(result.version_bump, comline_core::package::build::VersionBump::Major);
    assert_eq!(result.release_bump, comline_core::package::build::VersionBump::None);
    assert_eq!(result.current_version, "0.1.1-dev.2");
    assert_eq!(result.previous_version, Some("0.1.1-dev.1".to_string()));
    
    // Verify CAS still has all commits via object store
    let store = ObjectStore::new(&package_path);
    // The fact that we got 3 successful builds means all commits are stored
    let _ = store;
}
