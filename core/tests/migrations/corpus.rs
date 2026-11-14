// The fixture packages, and a fresh build of each.

use std::fs;
use std::path::{Path, PathBuf};

use comline_core::package::build::build;
use comline_core::package::build::cas::objects::{Commit, Tree};
use comline_core::package::build::cas::{REF_HEAD_MAIN_FILENAME, read_ref, ObjectStore};
use tempfile::TempDir;

/// One package a migration is proved on.
pub struct Fixture {
    /// The name its golden file has (`expected/<name>.txt`).
    pub name: &'static str,
    /// The directory it is copied from, under `core/tests/`.
    pub source: &'static str,
    /// The package inside the copy that is built: `.` for the directory
    /// itself, or one of several that depend on each other.
    pub package: &'static str,
    /// Whether it needs dependency resolution (the `deps` feature).
    pub deps: bool,
}

/// The corpus, in the order it is documented. Only ever added to.
pub const FIXTURES: &[Fixture] = &[
    // A schema that uses every construct the grammar has.
    Fixture {
        name: "tour",
        source: "migrations/corpus/tour",
        package: ".",
        deps: false,
    },
    // The package the CAS tests build.
    Fixture {
        name: "test",
        source: "fixtures/packages/test",
        package: ".",
        deps: false,
    },
    // `//!` headers: the `docs` blob.
    Fixture {
        name: "headers",
        source: "migrations/corpus/headers",
        package: ".",
        deps: false,
    },
    // `@unverified` at each scope, `handshake = "none"` and a `wire`.
    Fixture {
        name: "unverified",
        source: "migrations/corpus/unverified",
        package: ".",
        deps: false,
    },
    // A dependency that declares an encoding and has one of its own that
    // declares another: `dep_<name>` trees, `config` and `dependency_configs`.
    Fixture {
        name: "dependencies",
        source: "migrations/corpus/dependencies",
        package: "app",
        deps: true,
    },
];

/// The fixtures this build of the tests can build.
pub fn available() -> impl Iterator<Item = &'static Fixture> {
    FIXTURES
        .iter()
        .filter(|f| !f.deps || cfg!(feature = "deps"))
}

pub fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A fixture, copied to a scratch directory and built once.
pub struct Built {
    _dir: TempDir,
    /// The package that was built.
    pub package: PathBuf,
    pub store: ObjectStore,
}

impl Built {
    /// A copy of `fixture`, not built yet.
    pub fn copy(fixture: &Fixture) -> Self {
        let dir = TempDir::new().unwrap();
        copy_dir(&tests_dir().join(fixture.source), dir.path());
        let package = dir.path().join(fixture.package);
        let store = ObjectStore::new(&package);
        Self {
            _dir: dir,
            package,
            store,
        }
    }

    pub fn of(fixture: &Fixture) -> Self {
        let built = Self::copy(fixture);
        build(&built.package).unwrap_or_else(|e| panic!("{} builds: {e:?}", fixture.name));
        built
    }

    /// The commit `refs/heads/main` points to, and its root tree.
    pub fn head(&self) -> (Commit, Tree) {
        let hash = read_ref(&self.package, REF_HEAD_MAIN_FILENAME).unwrap();
        let commit = Commit::from_bytes(&self.store.read(&hash).unwrap()).unwrap();
        let tree = Tree::from_bytes(&self.store.read(&commit.tree).unwrap()).unwrap();
        (commit, tree)
    }
}
