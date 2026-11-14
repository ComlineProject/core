// Golden fresh-build hashes (core#268, step 1.2).
//
// A fresh build of each fixture package writes the same objects as the file
// in `expected/` says, to the byte: one line for every entry of the version's
// root tree, recursively, with its hash. Without it nothing notices a change
// to what a build writes for a package that doesn't use the thing that
// changed, which is what a field added without `skip_serializing_if` is.
//
// A line that moves is a ledger row (and the same lines in the frozen copy)
// or a format bump, and `cargo xtask check-format-bump` says so on the pull
// request. The standard library's schemas are compiled into every build and
// are not recorded: a change to `std` is not a change to the format. To
// regenerate after such a change:
//
//     COMLINE_UPDATE_GOLDEN=1 cargo test -p comline-core --features deps --test mod migrations::golden

use std::fs;

use comline_core::package::build::cas::objects::{EntryMode, Tree};
use comline_core::package::build::cas::ObjectStore;

use super::corpus::{self, Built, Fixture};

const HEADER: &str = "\
# The objects a fresh build of this fixture package writes (core#268): each
# entry of the version's root tree, recursively, with its hash, as
# `<hash>  <path>` (a tree's path ends in `/`). The standard library's
# schemas are compiled into every build and aren't recorded.
# These move only with a ledger row or a format bump. Regenerate:
#   COMLINE_UPDATE_GOLDEN=1 cargo test -p comline-core --features deps --test mod migrations::golden
";

/// A schema tree of the standard library: its first unit is the namespace.
fn is_std(tree: &Tree) -> bool {
    tree.entries
        .first()
        .is_some_and(|e| e.name.starts_with("namespace_std"))
}

fn describe(store: &ObjectStore, tree: &Tree, prefix: &str, lines: &mut Vec<String>) {
    for entry in &tree.entries {
        let path = format!("{prefix}{}", entry.name);
        match entry.mode {
            EntryMode::Blob => lines.push(format!("{}  {path}", entry.hash)),
            EntryMode::Tree => {
                let tree = Tree::from_bytes(&store.read(&entry.hash).unwrap()).unwrap();
                if is_std(&tree) {
                    lines.push(format!("-  {path}/  (standard library, not recorded)"));
                } else {
                    lines.push(format!("{}  {path}/", entry.hash));
                    describe(store, &tree, &format!("{path}/"), lines);
                }
            }
        }
    }
}

fn golden(fixture: &Fixture) -> String {
    let built = Built::of(fixture);
    let (_, tree) = built.head();
    let mut lines = vec![];
    describe(&built.store, &tree, "", &mut lines);
    format!("{HEADER}{}\n", lines.join("\n"))
}

#[test]
fn a_fresh_build_of_each_fixture_writes_the_recorded_objects() {
    let update = std::env::var_os("COMLINE_UPDATE_GOLDEN").is_some();
    for fixture in corpus::available() {
        let path = corpus::tests_dir().join(format!("migrations/expected/{}.txt", fixture.name));
        let fresh = golden(fixture);
        if update {
            fs::write(&path, &fresh).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: no golden file ({e}); COMLINE_UPDATE_GOLDEN=1 writes it",
                fixture.name
            )
        });
        if fresh != expected {
            let moved: Vec<String> = expected
                .lines()
                .zip(fresh.lines())
                .filter(|(a, b)| a != b)
                .map(|(a, b)| format!("  was  {a}\n  now  {b}"))
                .collect();
            panic!(
                "{}: a fresh build writes different objects than {} records ({} line(s) differ, or the number of lines):\n{}\n\
                 This is a change to the format: a ledger row, with the same lines in the frozen copy \
                 (core/formats/src/v{}), or a bump. See core/formats/src/v{}/SCHEMA.md.",
                fixture.name,
                path.display(),
                moved.len(),
                moved.join("\n"),
                comline_ir_formats::CURRENT,
                comline_ir_formats::CURRENT,
            );
        }
    }
}

#[test]
fn a_build_is_the_same_twice() {
    // The golden hashes are only worth anything if a build is deterministic:
    // two builds of one package in two directories write the same objects.
    for fixture in corpus::available() {
        assert_eq!(golden(fixture), golden(fixture), "{}", fixture.name);
    }
}

#[test]
fn every_golden_file_is_a_fixture_of_the_corpus() {
    let dir = corpus::tests_dir().join("migrations/expected");
    let mut files: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    let mut names: Vec<String> = corpus::FIXTURES
        .iter()
        .map(|f| format!("{}.txt", f.name))
        .collect();
    names.sort();
    assert_eq!(
        files, names,
        "a fixture without its golden file, or a golden file without its fixture"
    );
}
