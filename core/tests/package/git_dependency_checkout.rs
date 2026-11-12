//! A git dependency's cached checkout is reused only while it is what its
//! pinned commit has (core#254): HEAD at the commit and nothing edited,
//! added or ignored-and-planted in the working tree. Anything else is
//! restored from the pin, and the log says so.

use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use comline_core::package::config::dependency::{
    git_checkout_dir, DependencyConfig, DependencySource,
};
use comline_core::package::deps::{resolve, ResolvedDependency};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const SCHEMA: &str = "struct Thing {\n    id: u64\n}\n";

/// The log lines of everything run under [`Log::capture`].
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl Subscriber for Log {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut message = Message(String::new());
        event.record(&mut message);
        self.0.lock().unwrap().push(message.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

impl Log {
    /// Run `f`, returning what it returned and every line it logged.
    fn capture<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
        let log = Log::default();
        let result = tracing::subscriber::with_default(log.clone(), f);
        let lines = log.0.lock().unwrap().clone();
        (result, lines)
    }
}

fn restores(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|line| line.contains("restoring"))
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be on PATH to run this test");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A git repository at `dir` with `files` committed in it; its commit.
fn commit_repo(dir: &Path, files: &[(&str, &str)]) -> String {
    for (path, content) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    git(dir, &["init", "--quiet"]);
    git(dir, &["config", "user.email", "test@example.test"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "initial"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A package's `config.idp` text. `uri` is the one thing a path-like
/// string has to be written for: a Windows path's backslashes are escapes.
fn config(extra: &str) -> String {
    format!("congregation shared_types\nspecification_version = 1\n{extra}")
}

/// A local repository holding a minimal package, and a `.gitignore` that
/// ignores a file the dependency's author has no use for, as a dependency
/// is free to.
fn fixture() -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    let sha = commit_repo(
        tmp.path(),
        &[
            ("config.idp", &config("")),
            ("src/api.ids", SCHEMA),
            (".gitignore", "src/planted.ids\n"),
        ],
    );
    (tmp, sha)
}

fn dependency(repo: &Path, sha: &str, hash: Option<&str>) -> DependencyConfig {
    DependencyConfig {
        name: "shared_types".to_string(),
        accept_unverified: Default::default(),
        source: DependencySource::Git {
            version: "1.0.0".to_string(),
            uri: repo.to_string_lossy().to_string(),
            commit: sha.to_string(),
            hash: hash.map(str::to_string),
        },
    }
}

/// What the consumer in `root` makes of `dep`, and what it logged.
fn resolve_in(
    root: &Path,
    dep: &DependencyConfig,
) -> (eyre::Result<ResolvedDependency>, Vec<String>) {
    Log::capture(|| resolve(dep, root, &root.join(".comline/deps-cache")))
}

fn checkout_of(root: &Path, repo: &Path, sha: &str) -> PathBuf {
    git_checkout_dir(
        &root.join(".comline/deps-cache"),
        &repo.to_string_lossy(),
        sha,
    )
}

/// A dependency fetched once and pinned to what it was then, so that a
/// later resolve compiling anything else is a hash mismatch.
struct Fetched {
    repo: tempfile::TempDir,
    sha: String,
    consumer: tempfile::TempDir,
    checkout: PathBuf,
    pin: String,
}

impl Fetched {
    fn new() -> Self {
        let (repo, sha) = fixture();
        let consumer = tempfile::tempdir().unwrap();
        let (first, lines) = resolve_in(consumer.path(), &dependency(repo.path(), &sha, None));
        let first = first.expect("the first resolve fetches the pinned commit");
        assert!(
            restores(&lines).is_empty(),
            "nothing to restore the first time: {lines:?}"
        );
        let checkout = checkout_of(consumer.path(), repo.path(), &sha);
        assert_eq!(first.resolved_path, checkout);
        let pin = format!("blake3:{}", first.content_hash.to_hex());
        Fetched {
            repo,
            sha,
            consumer,
            checkout,
            pin,
        }
    }

    /// Resolve again, under the pin the first resolve produced.
    fn resolve_again(&self) -> (ResolvedDependency, Vec<String>) {
        let dep = dependency(self.repo.path(), &self.sha, Some(&self.pin));
        let (resolved, lines) = resolve_in(self.consumer.path(), &dep);
        (resolved.expect("resolves to the pinned content"), lines)
    }
}

#[test]
fn an_edited_file_in_the_cached_checkout_is_restored() {
    let fetched = Fetched::new();
    let api = fetched.checkout.join("src/api.ids");
    std::fs::write(
        &api,
        "struct Thing {\n    id: u64\n}\nstruct Extra {\n    id: u64\n}\n",
    )
    .unwrap();

    // Compiled as it stands, this is another dependency: the pin would fail.
    let (resolved, lines) = fetched.resolve_again();

    // (A Windows checkout may have CRLF line endings, whatever was committed.)
    let restored = std::fs::read_to_string(&api).unwrap().replace("\r\n", "\n");
    assert_eq!(restored, SCHEMA, "back to the pinned commit");
    assert_eq!(resolved.context.schema_contexts.len(), 1);
    assert_eq!(restores(&lines).len(), 1, "one line says so: {lines:?}");
    assert_eq!(git(&fetched.checkout, &["rev-parse", "HEAD"]), fetched.sha);
}

#[test]
fn a_deleted_file_in_the_cached_checkout_is_restored() {
    let fetched = Fetched::new();
    std::fs::remove_file(fetched.checkout.join("src/api.ids")).unwrap();

    let (resolved, lines) = fetched.resolve_again();

    assert!(fetched.checkout.join("src/api.ids").exists());
    assert_eq!(resolved.context.schema_contexts.len(), 1);
    assert_eq!(restores(&lines).len(), 1, "{lines:?}");
}

#[test]
fn an_added_schema_in_the_cached_checkout_is_removed() {
    let fetched = Fetched::new();
    let planted = fetched.checkout.join("src/planted_by_hand.ids");
    std::fs::write(&planted, "struct Planted {\n    id: u64\n}\n").unwrap();

    let (resolved, lines) = fetched.resolve_again();

    assert!(!planted.exists(), "removed");
    assert_eq!(
        resolved.context.schema_contexts.len(),
        1,
        "and not compiled"
    );
    assert_eq!(restores(&lines).len(), 1, "{lines:?}");
}

#[test]
fn an_ignored_planted_schema_is_cleaned_too() {
    let fetched = Fetched::new();
    // The dependency's own `.gitignore` names this file, so `git status`
    // without `--ignored` doesn't see it.
    let planted = fetched.checkout.join("src/planted.ids");
    std::fs::write(&planted, "struct Planted {\n    id: u64\n}\n").unwrap();
    assert!(
        git(
            &fetched.checkout,
            &["status", "--porcelain", "--untracked-files=all"]
        )
        .is_empty(),
        "the test's own premise: plain status can't see it"
    );

    let (resolved, lines) = fetched.resolve_again();

    assert!(!planted.exists(), "removed");
    assert_eq!(
        resolved.context.schema_contexts.len(),
        1,
        "and not compiled"
    );
    assert_eq!(restores(&lines).len(), 1, "{lines:?}");
}

#[test]
fn a_clean_cached_checkout_is_reused_as_it_is() {
    let fetched = Fetched::new();
    // A file the restore would take away, if it were taken for a restore:
    // the checkout's own `.git` records that it was not re-cloned.
    let marker = fetched.checkout.join(".git/reused");
    std::fs::write(&marker, "").unwrap();

    let (_, lines) = fetched.resolve_again();
    let (_, again) = fetched.resolve_again();

    assert!(marker.exists(), "not cloned again");
    assert!(restores(&lines).is_empty(), "{lines:?}");
    assert!(restores(&again).is_empty(), "{again:?}");
}

#[test]
fn a_fsmonitor_in_the_checkouts_config_is_not_run() {
    let fetched = Fetched::new();
    let marker = fetched.consumer.path().join("fsmonitor-ran");
    let hook = format!(
        "echo ran > '{}'",
        marker.to_string_lossy().replace('\\', "/")
    );
    git(&fetched.checkout, &["config", "core.fsmonitor", &hook]);

    // The premise: left to its config, git does run it.
    git(&fetched.checkout, &["status", "--porcelain"]);
    assert!(marker.exists(), "the hook is set up to run");
    std::fs::remove_file(&marker).unwrap();

    // Clean, so reused: through the status check, and the compile.
    let (_, lines) = fetched.resolve_again();
    assert!(!marker.exists(), "the checkout's fsmonitor ran");
    assert!(restores(&lines).is_empty(), "{lines:?}");

    // Changed, so restored: the checkout is thrown away, never reset in place.
    std::fs::write(fetched.checkout.join("src/api.ids"), "struct Other {\n}\n").unwrap();
    git(&fetched.checkout, &["config", "core.fsmonitor", &hook]);
    let (_, lines) = fetched.resolve_again();
    assert!(!marker.exists(), "the checkout's fsmonitor ran");
    assert_eq!(restores(&lines).len(), 1, "{lines:?}");
}

/// What a dependency's own git dependency leaves in it: its checkout, under
/// the dependency's `.comline/deps-cache`. Not a change to the dependency.
// Not on Windows: the dependency's checkout holds the nested one, so the nested
// `.git/hooks/...` paths pass MAX_PATH from a runner's temp dir. That limit
// is the cache layout's, and is the same with or without this check.
#[cfg(not(windows))]
#[test]
fn a_dependencys_own_git_dependency_is_not_a_change_to_it() {
    let inner = tempfile::tempdir().unwrap();
    let inner_sha = commit_repo(
        inner.path(),
        &[("config.idp", &config("")), ("src/inner.ids", SCHEMA)],
    );
    let inner_uri = inner.path().to_string_lossy().replace('\\', "/");

    let outer = tempfile::tempdir().unwrap();
    let outer_config = config(&format!(
        "\ndependencies = {{\n    inner = {{\n        version = \"1.0.0\"\n        \
         uri = \"{inner_uri}\"\n        commit = \"{inner_sha}\"\n    }}\n}}\n"
    ));
    let outer_sha = commit_repo(
        outer.path(),
        &[("config.idp", &outer_config), ("src/outer.ids", SCHEMA)],
    );

    let consumer = tempfile::tempdir().unwrap();
    let dep = dependency(outer.path(), &outer_sha, None);
    let (first, lines) = resolve_in(consumer.path(), &dep);
    first.expect("the first resolve fetches both");
    assert!(restores(&lines).is_empty(), "{lines:?}");

    let checkout = checkout_of(consumer.path(), outer.path(), &outer_sha);
    assert!(
        checkout.join(".comline/deps-cache").is_dir(),
        "the premise: the inner checkout is inside the outer one"
    );

    let (again, lines) = resolve_in(consumer.path(), &dep);
    again.expect("resolves again");
    assert!(
        restores(&lines).is_empty(),
        "reused, not restored: {lines:?}"
    );
}
