// Builds and releases: builds carry pre-release versions, `release` turns
// everything since the last release into a version, with a record of why.

use std::fs;
use std::path::Path;

use comline_core::package::build::{build, latest_release, release, ReleaseRequest, VersionBump};
use tempfile::TempDir;

fn package() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.idp"), "congregation test\nspecification_version = 1\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    schema(dir.path(), "struct Foo {\n    a: u32\n}\n");
    dir
}

fn schema(dir: &Path, source: &str) {
    fs::write(dir.join("src/ping.ids"), source).unwrap();
}

fn raise(bump: VersionBump, reason: &str) -> ReleaseRequest {
    ReleaseRequest { raise: Some(bump), reason: Some(reason.to_string()), stable: false }
}

#[test]
fn the_first_release_is_0_1_0_and_nothing_new_is_nothing_to_release() {
    let dir = package();
    assert_eq!(build(dir.path()).unwrap().current_version, "0.1.0-dev.1");
    assert!(latest_release(dir.path()).unwrap().is_none());

    let first = release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(first.version, "0.1.0");
    assert_eq!(first.previous_release, None);
    assert_eq!(latest_release(dir.path()).unwrap().unwrap().version, "0.1.0");

    let err = release(dir.path(), &ReleaseRequest::default()).unwrap_err().to_string();
    assert!(err.contains("0.1.0 is already released"), "{err}");

    // A build with nothing new stays on the release.
    let again = build(dir.path()).unwrap();
    assert_eq!(again.current_version, "0.1.0");
    assert_eq!(again.last_release.as_deref(), Some("0.1.0"));
}

#[test]
fn a_release_takes_the_highest_bump_since_the_last_one() {
    let dir = package();
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // A feature, then a breaking change: two builds, one release.
    schema(dir.path(), "struct Foo {\n    a: u32\n    optional b: string\n}\n");
    assert_eq!(build(dir.path()).unwrap().current_version, "0.1.1-dev.1");
    schema(dir.path(), "struct Foo {\n    b: string\n}\n");
    let built = build(dir.path()).unwrap();
    assert_eq!(built.current_version, "0.2.0-dev.2");
    assert_eq!(built.release_bump, VersionBump::Major);

    let released = release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(released.version, "0.2.0");
    assert_eq!(released.previous_release.as_deref(), Some("0.1.0"));
    assert_eq!(released.record.computed, VersionBump::Major);
    assert_eq!(released.record.applied, VersionBump::Major);
    assert!(released.changes.unwrap().schema_changes.is_breaking());
}

#[test]
fn raising_needs_a_reason_and_a_covered_raise_is_kept_as_a_note() {
    let dir = package();
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // A docstring change calls for a patch; the behaviour change behind it
    // calls for more, which the schema can't show.
    schema(dir.path(), "/// Now counts from one.\nstruct Foo {\n    a: u32\n}\n");
    build(dir.path()).unwrap();
    let no_reason = ReleaseRequest { raise: Some(VersionBump::Minor), ..Default::default() };
    assert!(release(dir.path(), &no_reason).unwrap_err().to_string().contains("needs a reason"));

    let raised = release(dir.path(), &raise(VersionBump::Major, "`a` now counts from one")).unwrap();
    assert_eq!(raised.version, "0.2.0");
    assert_eq!(raised.record.computed, VersionBump::Patch);
    assert_eq!(raised.record.applied, VersionBump::Major);
    assert!(!raised.record.covered);
    assert_eq!(raised.record.reason.as_deref(), Some("`a` now counts from one"));

    // A raise the changes already reach is recorded as covered.
    schema(dir.path(), "/// Now counts from one.\nstruct Foo {\n    c: u32\n}\n");
    build(dir.path()).unwrap();
    let covered = release(dir.path(), &raise(VersionBump::Minor, "renamed for clarity")).unwrap();
    assert_eq!(covered.version, "0.3.0");
    assert_eq!(covered.record.applied, VersionBump::Major);
    assert!(covered.record.covered);
    assert_eq!(covered.record.reason.as_deref(), Some("renamed for clarity"));
}

#[test]
fn releasing_1_0_is_its_own_step_and_happens_once() {
    let dir = package();
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    let stable = ReleaseRequest { stable: true, reason: Some("in production".into()), ..Default::default() };
    let one = release(dir.path(), &stable).unwrap();
    assert_eq!(one.version, "1.0.0");
    assert!(one.record.stable);
    assert!(release(dir.path(), &stable).unwrap_err().to_string().contains("already stable"));

    // From 1.0 on, a breaking change is a major.
    schema(dir.path(), "struct Foo {\n    b: u32\n}\n");
    assert_eq!(build(dir.path()).unwrap().current_version, "2.0.0-dev.1");
    assert_eq!(release(dir.path(), &ReleaseRequest::default()).unwrap().version, "2.0.0");
}

#[test]
fn nothing_that_calls_for_a_bump_needs_a_raise_to_release() {
    let dir = package();
    fs::write(
        dir.path().join("config.idp"),
        "congregation test\nspecification_version = 1\n",
    )
    .unwrap();
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // A config-only change: a new build, but nothing to bump.
    fs::write(
        dir.path().join("config.idp"),
        "congregation test\nspecification_version = 1\ncode_generation = { languages = { rust#1.70.0 = {} } }\n",
    )
    .unwrap();
    assert_eq!(build(dir.path()).unwrap().current_version, "0.1.1-dev.1");
    let err = release(dir.path(), &ReleaseRequest::default()).unwrap_err().to_string();
    assert!(err.contains("nothing since 0.1.0 calls for a release"), "{err}");
    assert_eq!(release(dir.path(), &raise(VersionBump::Patch, "new target")).unwrap().version, "0.1.1");
}

#[test]
fn an_old_history_s_plain_versions_count_as_releases() {
    use comline_core::package::build::cas::objects::Commit;
    use comline_core::package::build::cas::{main_ref, read_ref, update_ref, ObjectStore};

    // A history built before builds and releases were separate: its head is
    // a plain 0.0.3, written without a release record.
    let dir = package();
    build(dir.path()).unwrap();
    let store = ObjectStore::new(dir.path());
    let head = Commit::from_bytes(&store.read(&read_ref(dir.path(), main_ref()).unwrap()).unwrap()).unwrap();
    let old = Commit::new(head.tree, vec![], "someone".into(), 0, "Patch version bump".into(), "0.0.3".into());
    let old_hash = store.write(&old.to_bytes().unwrap()).unwrap();
    update_ref(dir.path(), main_ref(), &old_hash).unwrap();
    assert_eq!(latest_release(dir.path()).unwrap().unwrap().version, "0.0.3");

    // Anything new leaves the 0.0 era for 0.1.0.
    schema(dir.path(), "/// docs\nstruct Foo {\n    a: u32\n}\n");
    assert_eq!(build(dir.path()).unwrap().current_version, "0.1.0-dev.1");
    let released = release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!((released.previous_release.as_deref(), released.version.as_str()), (Some("0.0.3"), "0.1.0"));
}
