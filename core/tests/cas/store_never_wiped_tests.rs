// Nothing is deleted or changed without the user doing so (core#268, step
// 1.1): a store in another IR format is an error, and the error leaves
// `.comline/` exactly as it was, `deps-cache/` included.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use comline_core::compat;
use comline_core::package::build::cas::object_store::{
    StoreError, StoreState, CURRENT_IR_FORMAT_VERSION,
};
use comline_core::package::build::cas::ObjectStore;
use comline_core::package::build::{
    build, compile_package, latest_release, release, ReleaseRequest,
};
use tempfile::TempDir;

fn package() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.idp"), "congregation test\nspecification_version = 1\nsettings = {\n    bounds = {\n        string = {\n            max = 256\n        }\n        array = {\n            max = 1024\n        }\n    }\n}\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/ping.ids"),
        "struct Foo {\n    a: u32\n}\n",
    )
    .unwrap();
    dir
}

fn stamp(dir: &Path) -> PathBuf {
    dir.join(".comline/FORMAT_VERSION")
}

/// A package built once, with a fetched dependency in `deps-cache/`.
fn built_package() -> TempDir {
    let dir = package();
    build(dir.path()).unwrap();
    let cached = dir.path().join(".comline/deps-cache/dep-1234");
    fs::create_dir_all(cached.join("src")).unwrap();
    fs::write(cached.join("config.idp"), "a fetched dependency").unwrap();
    fs::write(cached.join("src/dep.ids"), "struct Dep {}\n").unwrap();
    dir
}

/// Every file and directory under `root`, by relative path, with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if path.is_dir() {
                into.insert(relative, None);
                walk(root, &path, into);
            } else {
                into.insert(relative, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

/// Every way a store can be refused, with the error that names it.
fn refused_stores() -> Vec<(&'static str, Box<dyn Fn(&Path)>, StoreError)> {
    let older = CURRENT_IR_FORMAT_VERSION - 1;
    vec![
        (
            "older",
            Box::new(move |d| fs::write(stamp(d), older.to_string()).unwrap()),
            StoreError::Older { found: older },
        ),
        (
            "much older",
            Box::new(|d| fs::write(stamp(d), "1").unwrap()),
            StoreError::Older { found: 1 },
        ),
        (
            "newer",
            Box::new(|d| fs::write(stamp(d), (CURRENT_IR_FORMAT_VERSION + 1).to_string()).unwrap()),
            StoreError::Newer {
                found: CURRENT_IR_FORMAT_VERSION + 1,
            },
        ),
        (
            "missing",
            Box::new(|d| fs::remove_file(stamp(d)).unwrap()),
            StoreError::Unstamped,
        ),
        (
            "garbage",
            Box::new(|d| fs::write(stamp(d), "not a number").unwrap()),
            StoreError::InvalidStamp {
                text: "not a number".into(),
            },
        ),
        (
            "empty",
            Box::new(|d| fs::write(stamp(d), "").unwrap()),
            StoreError::InvalidStamp {
                text: String::new(),
            },
        ),
        (
            "unreadable",
            Box::new(|d| {
                fs::remove_file(stamp(d)).unwrap();
                fs::create_dir(stamp(d)).unwrap();
            }),
            StoreError::UnreadableStamp {
                reason: String::new(),
            },
        ),
    ]
}

fn same_kind(found: &StoreError, wanted: &StoreError) -> bool {
    match (found, wanted) {
        // The reason is the operating system's wording.
        (StoreError::UnreadableStamp { .. }, StoreError::UnreadableStamp { .. }) => true,
        _ => found == wanted,
    }
}

fn store_error(error: &eyre::Report) -> &StoreError {
    error
        .downcast_ref::<StoreError>()
        .unwrap_or_else(|| panic!("not a StoreError: {error:#}"))
}

#[test]
fn a_build_refuses_an_older_store_and_changes_nothing() {
    let dir = built_package();
    fs::write(
        stamp(dir.path()),
        (CURRENT_IR_FORMAT_VERSION - 1).to_string(),
    )
    .unwrap();
    let before = snapshot(dir.path());

    let error = build(dir.path()).unwrap_err();

    assert_eq!(
        store_error(&error),
        &StoreError::Older {
            found: CURRENT_IR_FORMAT_VERSION - 1
        }
    );
    assert!(
        error.to_string().contains("`comline store reset`"),
        "{error}"
    );
    assert_eq!(
        snapshot(dir.path()),
        before,
        "files and contents, `.comline/` and all"
    );
}

#[test]
fn a_build_refuses_a_newer_store_and_changes_nothing() {
    let dir = built_package();
    fs::write(
        stamp(dir.path()),
        (CURRENT_IR_FORMAT_VERSION + 1).to_string(),
    )
    .unwrap();
    let before = snapshot(dir.path());

    let error = build(dir.path()).unwrap_err();

    assert_eq!(
        store_error(&error),
        &StoreError::Newer {
            found: CURRENT_IR_FORMAT_VERSION + 1
        }
    );
    assert!(
        error.to_string().contains("a newer comline wrote"),
        "{error}"
    );
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_build_refuses_a_missing_or_garbage_stamp_and_changes_nothing() {
    for (name, damage, wanted) in refused_stores()
        .into_iter()
        .filter(|(name, ..)| ["missing", "garbage", "empty", "unreadable"].contains(name))
    {
        let dir = built_package();
        damage(dir.path());
        let before = snapshot(dir.path());

        let error = build(dir.path()).unwrap_err();

        assert!(same_kind(store_error(&error), &wanted), "{name}: {error:#}");
        assert_eq!(snapshot(dir.path()), before, "{name}");
    }
}

#[test]
fn deps_cache_survives_every_refusal() {
    for (name, damage, wanted) in refused_stores() {
        let dir = built_package();
        damage(dir.path());
        let cached = dir.path().join(".comline/deps-cache/dep-1234");

        let error = build(dir.path()).unwrap_err();
        assert!(same_kind(store_error(&error), &wanted), "{name}: {error:#}");

        assert_eq!(
            fs::read_to_string(cached.join("config.idp")).unwrap(),
            "a fetched dependency",
            "{name}"
        );
        assert!(cached.join("src/dep.ids").is_file(), "{name}");
    }
}

#[test]
fn a_current_store_builds_as_before() {
    let dir = built_package();
    assert_eq!(
        fs::read_to_string(stamp(dir.path())).unwrap(),
        CURRENT_IR_FORMAT_VERSION.to_string()
    );
    let objects = ObjectStore::new(dir.path()).list_objects().unwrap();

    fs::write(
        dir.path().join("src/ping.ids"),
        "struct Foo {\n    a: u32\n}\nstruct Bar {\n    b: u32\n}\n",
    )
    .unwrap();
    let built = build(dir.path()).unwrap();

    assert_eq!(built.previous_version.as_deref(), Some("0.1.0-dev.1"));
    assert_eq!(built.current_version, "0.1.0-dev.2");
    // The history kept what was there, and the cache is where it was.
    let after = ObjectStore::new(dir.path()).list_objects().unwrap();
    assert!(objects.iter().all(|hash| after.contains(hash)));
    assert!(dir
        .path()
        .join(".comline/deps-cache/dep-1234/config.idp")
        .is_file());
}

#[test]
fn no_comline_dir_builds_and_stamps_as_before() {
    let dir = package();
    assert!(!dir.path().join(".comline").exists());

    let built = build(dir.path()).unwrap();

    assert_eq!(built.current_version, "0.1.0-dev.1");
    assert_eq!(
        fs::read_to_string(stamp(dir.path())).unwrap(),
        CURRENT_IR_FORMAT_VERSION.to_string()
    );
}

#[test]
fn a_comline_dir_holding_only_a_deps_cache_builds_and_keeps_it() {
    // What a package with a git dependency looks like at its first build:
    // the dependency was fetched into `.comline/` first.
    let dir = package();
    let cached = dir.path().join(".comline/deps-cache/dep-1234");
    fs::create_dir_all(&cached).unwrap();
    fs::write(cached.join("config.idp"), "a fetched dependency").unwrap();

    build(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(stamp(dir.path())).unwrap(),
        CURRENT_IR_FORMAT_VERSION.to_string()
    );
    assert_eq!(
        fs::read_to_string(cached.join("config.idp")).unwrap(),
        "a fetched dependency"
    );
}

/// `latest_release` and `compat::facts` read; `facts_of_working_tree` reads
/// and works out what a build would store; `release` and `build` write. On a
/// store they refuse, none of them writes anything, and each fails with the
/// same typed error. (`inspect` and `doc::model` work on a compiled project,
/// and `doc::model_frozen` on a store and tree the caller opened: they never
/// look in `.comline/` themselves.)
#[test]
fn no_entry_point_writes_to_a_store_it_refuses() {
    for (name, damage, wanted) in refused_stores() {
        let dir = built_package();
        let context = compile_package(dir.path()).unwrap();
        damage(dir.path());
        let before = snapshot(dir.path());

        let errors = [
            (
                "facts",
                compat::facts(dir.path(), None).map(drop).unwrap_err(),
            ),
            (
                "facts of a version",
                compat::facts(dir.path(), Some("0.1.0-dev.1"))
                    .map(drop)
                    .unwrap_err(),
            ),
            (
                "facts_of_working_tree",
                compat::facts_of_working_tree(dir.path(), &context)
                    .map(drop)
                    .unwrap_err(),
            ),
            (
                "latest_release",
                latest_release(dir.path()).map(drop).unwrap_err(),
            ),
            (
                "release",
                release(dir.path(), &ReleaseRequest::default())
                    .map(drop)
                    .unwrap_err(),
            ),
            ("build", build(dir.path()).map(drop).unwrap_err()),
            ("init", ObjectStore::new(dir.path()).init().unwrap_err()),
        ];
        for (entry, error) in &errors {
            assert!(
                same_kind(store_error(error), &wanted),
                "{name}, {entry}: {error:#}"
            );
        }

        assert_eq!(snapshot(dir.path()), before, "{name}");
    }
}

#[test]
fn the_read_only_entry_points_read_a_current_store_as_before() {
    let dir = built_package();
    let context = compile_package(dir.path()).unwrap();
    let before = snapshot(dir.path());

    assert_eq!(
        compat::facts(dir.path(), None).unwrap().version,
        "0.1.0-dev.1"
    );
    assert!(compat::facts_of_working_tree(dir.path(), &context).is_ok());
    assert!(latest_release(dir.path()).unwrap().is_none());

    assert_eq!(snapshot(dir.path()), before, "reading wrote nothing");
    assert_eq!(
        comline_core::package::build::cas::object_store::ensure_compatible_store(dir.path()),
        Ok(StoreState::Current)
    );
}

/// The one rule in code form: core's source deletes or moves nothing that
/// could be a store. Deleting a store is for an explicit user command (the
/// CLI's `comline reset`, `comline store reset`), never a side effect of a
/// build or a read.
#[test]
fn core_deletes_and_moves_no_store() {
    const CALLS: &[&str] = &[
        "remove_dir_all",
        "fs::remove_dir(",
        "fs::remove_file(",
        "fs::rename(",
    ];

    fn sources(dir: &Path, into: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, into);
            } else if path.extension().is_some_and(|e| e == "rs") {
                into.push(path);
            }
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    sources(&src, &mut files);
    assert!(files.len() > 50, "found the source tree");

    let mut found = std::collections::BTreeSet::new();
    for file in files {
        let relative = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for line in fs::read_to_string(&file).unwrap().lines() {
            let code = line.trim_start();
            if !code.starts_with("//") && CALLS.iter().any(|call| code.contains(call)) {
                found.insert(relative.clone());
            }
        }
    }

    // Only `package/deps.rs` deletes or renames anything, and only the
    // checkout of one git dependency and its `.partial` staging directory
    // inside `deps-cache/`: a cache entry that is re-fetched when it is
    // dirty or half made. It never touches the store or the cache as a
    // whole. A new file here is a new way to delete something: review it
    // against the rule before adding it.
    let allowed = std::collections::BTreeSet::from(["package/deps.rs".to_string()]);
    assert_eq!(
        found, allowed,
        "files that delete or move files in core's source"
    );
}
