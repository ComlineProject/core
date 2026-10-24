use comline_core::package::build::build;
use std::fs;
use std::path::PathBuf;

fn copy_dir_all(src: impl AsRef<std::path::Path>, dst: impl AsRef<std::path::Path>) -> std::io::Result<()> {
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

#[test]
fn test_no_changes_no_version_bump() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/packages/test");
    let target = root.join("target/tmp/no_changes_test");

    // Clean up
    if target.exists() {
        fs::remove_dir_all(&target).unwrap();
    }

    // Setup test package
    copy_dir_all(&source, &target).unwrap();
    
    // Remove .frozen if exists
    let frozen = target.join(".frozen");
    if frozen.exists() {
        fs::remove_dir_all(&frozen).unwrap();
    }

    // First build
    println!("=== First Build ===");
    let result1 = build(&target).expect("First build failed");
    println!("Version: {}", result1.current_version);
    assert_eq!(result1.current_version, "0.1.0-dev.1");
    assert!(result1.is_initial_build());

    // Second build with NO CHANGES
    println!("\n=== Second Build (No Changes) ===");
    let result2 = build(&target).expect("Second build failed");
    println!("Previous: {:?}", result2.previous_version);
    println!("Current: {}", result2.current_version);
    println!("Bump: {:?}", result2.version_bump);
    
    // Version should NOT change
    assert_eq!(result2.previous_version, Some("0.1.0-dev.1".to_string()));
    assert_eq!(result2.current_version, "0.1.0-dev.1"); // Should stay the same!
    assert_eq!(result2.version_bump, comline_core::package::build::VersionBump::None);
    
    // Schema changes should be empty
    if let Some(changes) = result2.schema_changes {
        assert!(changes.is_empty(), "Should have no changes");
    }

    println!("✅ Test passed: No changes = no version bump");
}

/// `build_with`'s check runs on the compiled package before anything is
/// frozen: an error from it fails the build, and `.comline/` isn't written.
#[test]
fn a_failing_build_check_freezes_nothing() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = root.join("target/tmp/failing_build_check");
    if target.exists() {
        fs::remove_dir_all(&target).unwrap();
    }
    copy_dir_all(root.join("tests/fixtures/packages/test"), &target).unwrap();

    let mut saw_config = false;
    let error = comline_core::package::build::build_with(&target, |context| {
        saw_config = context.config_frozen.is_some();
        Err(eyre::eyre!("`rust#1.70.0` is older than the rust generator handles"))
    })
    .expect_err("the check's error fails the build");

    assert!(saw_config, "the check sees the compiled package");
    assert!(error.to_string().contains("older than the rust generator"), "{error}");
    assert!(!target.join(".comline").exists(), "nothing was frozen");

    // And with a check that passes, it builds as `build` does.
    let result = comline_core::package::build::build_with(&target, |_| Ok(())).expect("builds");
    assert!(result.is_initial_build());
    assert!(target.join(".comline").exists());
}
