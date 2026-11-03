// Reconciling a history that diverged after a Git merge (core#160): two
// copies of one package build (and release) on their own, their
// `.comline/objects` are merged and the ref left conflicted, as Git leaves
// them; the next build joins the two into one line.

use std::fs;
use std::path::Path;

use comline_core::compat;
use comline_core::package::build::cas::objects::Commit;
use comline_core::package::build::cas::reconcile::{mark_diverged, read_heads, Heads, Reconciled};
use comline_core::package::build::cas::{main_ref, read_ref, Hash, ObjectStore};
use comline_core::package::build::{build, latest_release, release, ReleaseRequest};
use tempfile::TempDir;

const BASE: &str = "struct User {\n    id: u64\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n}\n";
const OURS: &str = "struct User {\n    id: u64\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n    function count() -> u32;\n}\n";
const THEIRS: &str = "struct Team {\n    id: u64\n}\n\nprotocol Teams {\n    function get(id: u64) -> Team;\n}\n";

fn package() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.idp"), "congregation test\nspecification_version = 1\nsettings = {\n    bounds = {\n        string = {\n            max = 256\n        }\n        array = {\n            max = 1024\n        }\n    }\n}\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/users.ids"), BASE).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else if !target.exists() {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A second branch: a copy of the package as it stands.
fn branch(dir: &Path) -> TempDir {
    let copy = TempDir::new().unwrap();
    copy_dir(dir, copy.path());
    copy
}

fn head(dir: &Path) -> Hash {
    read_ref(dir, main_ref()).unwrap()
}

fn commit(dir: &Path, hash: &Hash) -> Commit {
    Commit::from_bytes(&ObjectStore::new(dir).read(hash).unwrap()).unwrap()
}

/// Merge `theirs` into `ours` as Git would: the object files union
/// cleanly and the ref conflicts. The working tree is both sides' files,
/// ours where both have one; a test writes whatever else the merge made.
fn merge(ours: &Path, theirs: &Path) -> (Hash, Hash) {
    let (a, b) = (head(ours), head(theirs));
    copy_dir(&theirs.join(".comline/objects"), &ours.join(".comline/objects"));
    fs::write(
        ours.join(".comline").join(main_ref()),
        format!("<<<<<<< HEAD\n{a}\n=======\n{b}\n>>>>>>> theirs\n"),
    )
    .unwrap();
    copy_dir(&theirs.join("src"), &ours.join("src"));
    (a, b)
}

fn since(dir: &Path, protocol: &str, function: &str) -> String {
    let facts = compat::facts(dir, None).unwrap();
    let p = facts.protocols.iter().find(|p| p.name == protocol).unwrap();
    p.functions.iter().find(|f| f.name == function).unwrap().since.clone()
}

/// The two parents of the new head, and that the ref is clean again.
fn parents(dir: &Path) -> Vec<Hash> {
    let text = fs::read_to_string(dir.join(".comline").join(main_ref())).unwrap();
    assert_eq!(text.trim().len(), 64, "the ref is one hash again: {text}");
    assert!(matches!(read_heads(dir).unwrap(), Heads::One(_)));
    commit(dir, &head(dir)).parents
}

#[test]
fn without_releases_the_dev_counts_are_renumbered_onto_one_line() {
    let ours = package();
    assert_eq!(build(ours.path()).unwrap().current_version, "0.1.0-dev.1");
    let theirs = branch(ours.path());

    fs::write(ours.path().join("src/users.ids"), OURS).unwrap();
    assert_eq!(build(ours.path()).unwrap().current_version, "0.1.0-dev.2");
    fs::write(theirs.path().join("src/teams.ids"), THEIRS).unwrap();
    assert_eq!(build(theirs.path()).unwrap().current_version, "0.1.0-dev.2");
    fs::write(theirs.path().join("src/teams.ids"), THEIRS.replace("Team {", "Team {\n    name: string")).unwrap();
    assert_eq!(build(theirs.path()).unwrap().current_version, "0.1.0-dev.3");

    let (a, b) = merge(ours.path(), theirs.path());
    fs::write(ours.path().join("src/teams.ids"), THEIRS.replace("Team {", "Team {\n    name: string")).unwrap();
    let built = build(ours.path()).unwrap();

    // One line: it continues from one head (the first built: same second,
    // so the lower hash), the other's net change as one build on top.
    let parents = parents(ours.path());
    assert_eq!(parents.len(), 2);
    let (onto, other) = (parents[0], parents[1]);
    assert_eq!({ let mut p = vec![onto, other]; p.sort_by_key(|h| h.to_hex()); p }, {
        let mut p = vec![a, b];
        p.sort_by_key(|h| h.to_hex());
        p
    });
    let onto_version = commit(ours.path(), &onto).version;
    let onto_count: u64 = onto_version.rsplit('.').next().unwrap().parse().unwrap();
    assert_eq!(built.previous_version.as_deref(), Some(onto_version.as_str()));
    assert_eq!(built.current_version, format!("0.1.0-dev.{}", onto_count + 1));
    assert_eq!(built.last_release, None);
    assert_eq!(built.reconciled, Some(Reconciled { onto, second_parent: Some(other) }));

    // The facts are the merged tree's, against no release: everything is
    // the first release's.
    assert_eq!(since(ours.path(), "Users", "count"), "0.1.0");
    assert_eq!(since(ours.path(), "Teams", "get"), "0.1.0");

    // A build after that is an ordinary one.
    let again = build(ours.path()).unwrap();
    assert_eq!(again.current_version, built.current_version);
    assert_eq!(again.reconciled, None);
}

#[test]
fn a_release_on_one_side_keeps_its_version_and_the_other_side_builds_on_it() {
    for released_side_is_ours in [true, false] {
        let ours = package();
        build(ours.path()).unwrap();
        release(ours.path(), &ReleaseRequest::default()).unwrap();
        let theirs = branch(ours.path());

        let (releasing, building) = if released_side_is_ours {
            (ours.path(), theirs.path())
        } else {
            (theirs.path(), ours.path())
        };
        fs::write(releasing.join("src/users.ids"), OURS).unwrap();
        assert_eq!(build(releasing).unwrap().current_version, "0.1.1-dev.1");
        assert_eq!(release(releasing, &ReleaseRequest::default()).unwrap().version, "0.1.1");
        fs::write(building.join("src/teams.ids"), THEIRS).unwrap();
        assert_eq!(build(building).unwrap().current_version, "0.1.1-dev.1");
        let released_head = head(releasing);

        merge(ours.path(), theirs.path());
        fs::write(ours.path().join("src/users.ids"), OURS).unwrap();
        fs::write(ours.path().join("src/teams.ids"), THEIRS).unwrap();
        let built = build(ours.path()).unwrap();

        // On the release, and after it: the unreleased side's dev build is
        // renumbered past it, and its function's "since" is the next release.
        let parents = parents(ours.path());
        assert_eq!(parents[0], released_head, "continues from the release");
        assert_eq!(parents.len(), 2);
        assert_eq!(built.last_release.as_deref(), Some("0.1.1"));
        assert_eq!(built.current_version, "0.1.2-dev.1");
        assert_eq!(since(ours.path(), "Users", "count"), "0.1.1");
        assert_eq!(since(ours.path(), "Teams", "get"), "0.1.2");
        assert_eq!(latest_release(ours.path()).unwrap().unwrap().version, "0.1.1");
    }
}

#[test]
fn releases_on_both_sides_refuse_until_one_is_picked() {
    let ours = package();
    build(ours.path()).unwrap();
    release(ours.path(), &ReleaseRequest::default()).unwrap();
    let theirs = branch(ours.path());

    fs::write(ours.path().join("src/users.ids"), OURS).unwrap();
    build(ours.path()).unwrap();
    release(ours.path(), &ReleaseRequest::default()).unwrap();
    fs::write(theirs.path().join("src/teams.ids"), THEIRS).unwrap();
    build(theirs.path()).unwrap();
    release(theirs.path(), &ReleaseRequest::default()).unwrap();

    let (a, b) = merge(ours.path(), theirs.path());
    let conflicted = fs::read_to_string(ours.path().join(".comline").join(main_ref())).unwrap();
    let err = build(ours.path()).unwrap_err().to_string();
    assert!(err.contains("both branches released"), "{err}");
    assert!(err.contains("0.1.0 ("), "names the common ancestor: {err}");
    assert!(err.contains(&a.to_hex()) && err.contains(&b.to_hex()), "{err}");
    assert!(err.contains("Pick the release to keep"), "{err}");
    // Nothing written: the ref is as Git left it.
    let after = fs::read_to_string(ours.path().join(".comline").join(main_ref())).unwrap();
    assert_eq!(after, conflicted);
    // Neither can a release go ahead on a conflicted ref.
    let err = release(ours.path(), &ReleaseRequest::default()).unwrap_err().to_string();
    assert!(err.contains("conflict markers"), "{err}");

    // Picking one: the other side's changes build on top of its release.
    fs::write(ours.path().join(".comline").join(main_ref()), a.to_hex()).unwrap();
    let built = build(ours.path()).unwrap();
    assert_eq!(built.last_release.as_deref(), Some("0.1.1"));
    assert_eq!(built.current_version, "0.1.2-dev.1");
    assert_eq!(parents(ours.path()), vec![a]);
}

#[test]
fn a_ref_resolved_to_one_side_can_be_marked_diverged_again() {
    let ours = package();
    build(ours.path()).unwrap();
    let theirs = branch(ours.path());
    fs::write(ours.path().join("src/users.ids"), OURS).unwrap();
    build(ours.path()).unwrap();
    fs::write(theirs.path().join("src/teams.ids"), THEIRS).unwrap();
    build(theirs.path()).unwrap();

    // Resolved by taking ours: their commit is orphaned.
    let (a, b) = merge(ours.path(), theirs.path());
    fs::write(ours.path().join(".comline").join(main_ref()), a.to_hex()).unwrap();

    mark_diverged(ours.path(), &b).unwrap();
    assert_eq!(read_heads(ours.path()).unwrap(), Heads::Two(a, b));
    let built = build(ours.path()).unwrap();
    let parents = parents(ours.path());
    assert_eq!(parents.len(), 2);
    assert!(parents.contains(&a) && parents.contains(&b));
    assert_eq!(built.current_version, "0.1.0-dev.3");

    // An unknown commit is refused.
    let unknown = Hash::from_hex(&"ab".repeat(32)).unwrap();
    assert!(mark_diverged(ours.path(), &unknown).is_err());
}

#[test]
fn a_head_that_contains_the_other_fast_forwards() {
    let ours = package();
    build(ours.path()).unwrap();
    let theirs = branch(ours.path());
    fs::write(theirs.path().join("src/teams.ids"), THEIRS).unwrap();
    build(theirs.path()).unwrap();

    let (_, b) = merge(ours.path(), theirs.path());
    // Nothing new beyond theirs: the ref just moves to it.
    let built = build(ours.path()).unwrap();
    assert_eq!(built.current_version, "0.1.0-dev.2");
    assert_eq!(head(ours.path()), b);
    assert_eq!(built.reconciled, Some(Reconciled { onto: b, second_parent: None }));
    parents(ours.path());
}
