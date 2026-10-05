// Dependency resolution — feature = "deps".
//
// Turns a declared `DependencyConfig` into an actually-compiled dependency:
// fetch it (a local path is already "fetched"; a git source is cloned/checked
// out at its pinned commit into a cache dir, by shelling out to `git` — see
// `resolve_git`), compile it the same way any Comline package compiles, and
// verify its content against the declared hash. Registry sources are parsed
// but rejected here — there's no registry server yet to resolve against.
//
// This module is opt-in (not part of `default`): only a native consumer that
// actually runs `comline build`/`check` (the CLI) needs it. Every other
// consumer of `comline-core` — including the wasm32 ones (the playground
// editor, the language server's analysis-only build) — simply doesn't enable
// the feature and is unaffected.

// Standard Uses
use std::path::{Path, PathBuf};

// Crate Uses
use crate::package::build::cas::storage::Hash;
use crate::package::build::compile_package;
use crate::package::config::dependency::{git_checkout_dir, DependencyConfig, DependencySource, DEPS_CACHE_DIR};
use crate::package::config::ir::context::ProjectContext;

// External Uses
use eyre::{bail, eyre, Result};

/// A dependency, fetched and compiled.
pub struct ResolvedDependency {
    pub name: String,
    /// Where it ended up on disk — a `Path` dependency's target directory, or
    /// a `Git` dependency's cache checkout.
    pub resolved_path: PathBuf,
    /// The compiled dependency — its own schemas, ready to be merged into
    /// the consuming project's schema sources or vendored into its CAS tree.
    pub context: ProjectContext,
    /// blake3 hash over the dependency's frozen schema content (all schema
    /// contexts, in order) — stable across incidental source formatting.
    /// Compared against `DependencyConfig::declared_hash()` when the author
    /// wrote one; always computed and recorded either way so drift is
    /// detectable on a later build even without a declared hash.
    pub content_hash: Hash,
    /// The dependency's effective version: what its own CAS history reports
    /// if it has one (`comline build` has been run inside it at least once),
    /// else the `MINIMUM_VERSION` placeholder used for anything unbuilt.
    pub version: String,
}

/// Resolve one dependency: fetch its source (if needed), compile it, and
/// verify its content hash. `project_root` is the *consuming* package's
/// directory (a `Path` dependency resolves relative to it); `cache_dir` is
/// where `Git` sources get cloned/checked out (conventionally
/// `<project_root>/.comline/deps-cache/`).
pub fn resolve(
    dep: &DependencyConfig,
    project_root: &Path,
    cache_dir: &Path,
) -> Result<ResolvedDependency> {
    let resolved_path = match &dep.source {
        DependencySource::Path { path, .. } => project_root.join(path),
        DependencySource::Git { uri, commit, .. } => resolve_git(uri, commit, cache_dir)?,
        DependencySource::Registry { .. } => bail!(
            "dependency '{}': registry dependencies aren't supported yet — \
             no registry server exists to resolve against",
            dep.name
        ),
    };

    let context = compile_package(&resolved_path).map_err(|e| {
        eyre!(
            "dependency '{}': failed to compile at '{}': {e}",
            dep.name,
            resolved_path.display()
        )
    })?;

    let content_hash = hash_frozen_content(&context)?;

    match dep.declared_hash() {
        Some(declared) => {
            let computed = format!("blake3:{}", content_hash.to_hex());
            if declared != computed {
                bail!(
                    "dependency '{}': hash mismatch — declared {declared}, resolved to {computed}. \
                     Either the source changed without updating the pin, or something tampered \
                     with it.",
                    dep.name
                );
            }
        }
        None => {
            tracing::warn!(
                "dependency '{}' has no declared hash — trusting this resolution. \
                 Consider pinning: hash = \"blake3:{}\"",
                dep.name,
                content_hash.to_hex()
            );
        }
    }

    let version = resolved_version(&resolved_path);

    Ok(ResolvedDependency {
        name: dep.name.clone(),
        resolved_path,
        context,
        content_hash,
        version,
    })
}

/// Resolve every dependency declared in `context`'s raw congregation, in a
/// deterministic (name-sorted) order regardless of `HashMap` iteration —
/// matching the freeze order `interpret_assignment_dependencies` already
/// uses. Shared by [`resolve_dependency_sources`] (schema merging, during
/// `compile_package`) and CAS vendoring (during `build`, in
/// `package::build::cas::build`).
pub(crate) fn resolve_all(
    context: &ProjectContext,
    project_root: &Path,
) -> Result<Vec<ResolvedDependency>> {
    let deps = DependencyConfig::parse_dependencies(&context.config.assignments)
        .map_err(|e| eyre!("{e}"))?;
    if deps.is_empty() {
        return Ok(Vec::new());
    }

    let cache_dir = project_root.join(DEPS_CACHE_DIR);

    let mut names: Vec<&String> = deps.keys().collect();
    names.sort();

    names
        .into_iter()
        .map(|name| resolve(&deps[name], project_root, &cache_dir))
        .collect()
}

/// For every declared dependency: resolve it, verify its hash, and produce
/// its schema sources, each namespaced under the dependency's own declared
/// name — `shared_types = { path = "../shared-types" }`'s `foo.ids` becomes
/// `shared_types::foo` in the consuming project, so `use shared_types::foo::X`
/// resolves the normal way. Called from `package::build::interpret_schemas`.
pub(crate) fn resolve_dependency_sources(
    context: &ProjectContext,
    project_root: &Path,
) -> Result<Vec<(Vec<String>, String)>> {
    let mut sources = Vec::new();
    for resolved in resolve_all(context, project_root)? {
        for (namespace, source) in
            crate::package::build::glob_schema_sources(&resolved.resolved_path)?
        {
            let mut prefixed = vec![resolved.name.clone()];
            prefixed.extend(namespace);
            sources.push((prefixed, source));
        }
    }
    Ok(sources)
}

/// A resolved dependency's effective version: whatever its own `.comline/`
/// history's `refs/heads/main` currently points to, or a fixed placeholder
/// if it has never been built. Declared `version` strings on `Git`/`Registry`
/// sources describe the *pin*, not necessarily what the resolved content's
/// own CAS history says — this reads the ground truth.
fn resolved_version(package_path: &Path) -> String {
    use crate::package::build::cas::objects::Commit;
    use crate::package::build::cas::{main_ref, read_ref, ref_exists, ObjectStore};

    if !ref_exists(package_path, main_ref()) {
        return "0.0.0-unbuilt".to_string();
    }

    (|| -> Result<String> {
        let store = ObjectStore::new(package_path);
        let commit_hash = read_ref(package_path, main_ref())?;
        let commit = Commit::from_bytes(&store.read(&commit_hash)?)?;
        Ok(commit.version)
    })()
    .unwrap_or_else(|_| "0.0.0-unbuilt".to_string())
}

/// blake3 hash over every schema context's frozen units, concatenated in
/// order. Mirrors the shape that becomes the dependency's vendored CAS
/// subtree, so this hash and that tree's content agree by construction.
fn hash_frozen_content(context: &ProjectContext) -> Result<Hash> {
    let mut all_units = Vec::new();
    for schema_ctx in &context.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        if let Some(units) = frozen_ref.as_ref() {
            all_units.push(units.clone());
        }
    }

    let bytes = bincode::serialize(&all_units)
        .map_err(|e| eyre!("failed to serialize dependency schema content: {e}"))?;
    Ok(Hash::from_bytes(&bytes))
}

/// Fetch (or reuse a cached checkout of) a git dependency at its pinned
/// commit, by shelling out to the system `git` binary. Cache key is a hash of
/// `uri` + `commit`, so distinct pins never collide and re-resolving the same
/// pin is a no-op after the first fetch.
///
/// Deliberately not a `git2`/`libgit2` dependency: `libgit2`'s HTTPS
/// transport links OpenSSL, a C library that isn't reliably available to
/// build against everywhere this crate builds (and would need vendoring or a
/// system install either way). Shelling out to `git` needs nothing but `git`
/// itself already being on `PATH` — true of essentially every dev machine
/// and CI image, and the same assumption the rest of this toolchain already
/// makes (`comline new --git` runs `git init` the same way).
///
/// The checkout is made next to its final place and renamed into it only once
/// `git checkout` succeeded, so a fetch that dies partway (remote unreachable,
/// dropped connection, a commit that doesn't exist) leaves nothing behind for
/// the next attempt to mistake for a finished checkout. A checkout in the
/// cache that is *not* at the pinned commit (one an earlier version left
/// half-made) is thrown away and fetched again rather than reused.
fn resolve_git(uri: &str, commit: &str, cache_dir: &Path) -> Result<PathBuf> {
    let checkout_path = git_checkout_dir(cache_dir, uri, commit);

    if checkout_path.join(".git").exists() {
        // Already fetched for this exact uri+commit pin — reuse it. The pin
        // is the commit, which is immutable, so there is nothing to update.
        if is_checked_out_at(&checkout_path, commit) {
            return Ok(checkout_path);
        }
        remove_dir(&checkout_path)?;
    }

    let mut staging_name = checkout_path.file_name().unwrap_or_default().to_os_string();
    staging_name.push(".partial");
    let staging_path = checkout_path.with_file_name(staging_name);

    // A staging dir left by an interrupted run (killed mid-fetch) starts over.
    remove_dir(&staging_path)?;
    std::fs::create_dir_all(&staging_path).map_err(|e| {
        eyre!(
            "failed to create dependency checkout dir '{}': {e}",
            staging_path.display()
        )
    })?;

    if let Err(error) = fetch_commit(&staging_path, uri, commit) {
        let _ = std::fs::remove_dir_all(&staging_path);
        return Err(error);
    }

    // `rename` won't replace an existing directory on every platform.
    remove_dir(&checkout_path)?;
    std::fs::rename(&staging_path, &checkout_path).map_err(|e| {
        eyre!(
            "failed to move dependency checkout '{}' into place at '{}': {e}",
            staging_path.display(),
            checkout_path.display()
        )
    })?;

    Ok(checkout_path)
}

/// Fetch `commit` from `uri` into a new repository at `dir` and check it out.
fn fetch_commit(dir: &Path, uri: &str, commit: &str) -> Result<()> {
    run_git(dir, &["init", "--quiet"])?;
    run_git(dir, &["remote", "add", "origin", uri])?;
    // `--depth 1` works for a branch/tag ref on most forges; a bare commit
    // SHA needs the full history on servers that don't support fetching an
    // arbitrary commit directly (classic `git`/`smart HTTP` does; some do
    // not) — fall back to a full fetch if the shallow one fails.
    if run_git_allow_failure(dir, &["fetch", "--depth", "1", "origin", commit]).is_err() {
        run_git(dir, &["fetch", "origin"])?;
    }
    run_git(dir, &["checkout", "--quiet", commit])
}

/// Whether the repository at `dir` has `commit` checked out: its `HEAD` is
/// the commit `commit` names. A repository that was `init`ed but never
/// checked out has an unborn `HEAD` and answers no.
fn is_checked_out_at(dir: &Path, commit: &str) -> bool {
    let rev_parse = |rev: &str| git_output(dir, &["rev-parse", "--verify", "--quiet", rev]);
    match (rev_parse("HEAD"), rev_parse(&format!("{commit}^{{commit}}"))) {
        (Ok(head), Ok(pinned)) => head == pinned,
        _ => false,
    }
}

/// Remove `dir` and everything in it; a directory that isn't there is fine.
fn remove_dir(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(eyre!("failed to remove '{}': {e}", dir.display())),
    }
}

fn run_git(dir: &Path, args: &[&str]) -> Result<()> {
    run_git_allow_failure(dir, args)
        .map_err(|e| eyre!("git {} (in '{}'): {e}", args.join(" "), dir.display()))
}

fn run_git_allow_failure(dir: &Path, args: &[&str]) -> Result<()> {
    git_output(dir, args).map(|_| ())
}

/// Run `git` with `args` in `dir`: its trimmed stdout, or an error carrying
/// its stderr.
fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| {
            eyre!(
                "failed to run `git {}`: {e} (is git installed and on PATH?)",
                args.join(" ")
            )
        })?;

    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End to end: a consumer package whose schema `use`s a type from a
    /// `Path` dependency compiles, and the cross-package reference resolves
    /// — this is the actual point of vendoring a dependency's schema into
    /// the same `interpret_schema_sources` pass, not just fetching it.
    #[test]
    fn a_consumer_resolves_a_type_from_its_path_dependency() {
        let tmp = tempfile::tempdir().unwrap();

        let dep_dir = tmp.path().join("shared-types");
        std::fs::create_dir_all(dep_dir.join("src")).unwrap();
        std::fs::write(
            dep_dir.join("config.idp"),
            "congregation shared_types\nspecification_version = 1\n",
        )
        .unwrap();
        std::fs::write(
            dep_dir.join("src/models.ids"),
            "struct Thing {\n    id: u64\n}\n",
        )
        .unwrap();

        let consumer_dir = tmp.path().join("consumer");
        std::fs::create_dir_all(consumer_dir.join("src")).unwrap();
        std::fs::write(
            consumer_dir.join("config.idp"),
            "congregation consumer\n\
             specification_version = 1\n\
             \n\
             dependencies = {\n    \
                 shared_types = {\n        \
                     path = \"../shared-types\"\n    \
                 }\n\
             }\n",
        )
        .unwrap();
        std::fs::write(
            consumer_dir.join("src/main.ids"),
            "use shared_types::models::Thing\n\
             \n\
             struct Holder {\n    \
                 thing: Thing\n\
             }\n",
        )
        .unwrap();

        let context = crate::package::build::compile_package(&consumer_dir)
            .expect("consumer package with a resolvable dependency should compile");
        assert_eq!(
            context.schema_contexts.len(),
            2,
            "the consumer's own schema plus the dependency's, merged into one pass"
        );
    }

    fn write_minimal_package(dir: &Path, congregation_name: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("config.idp"),
            format!("congregation {congregation_name}\nspecification_version = 1\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/main.ids"), "struct Thing {\n    id: u64\n}\n").unwrap();
    }

    fn path_dep(name: &str, path: &str, hash: Option<&str>) -> DependencyConfig {
        DependencyConfig {
            name: name.to_string(),
            source: DependencySource::Path {
                path: PathBuf::from(path),
                hash: hash.map(str::to_string),
            },
        }
    }

    /// A local git repo with one commit containing a minimal package —
    /// returns (repo path, commit sha). No network access needed; `git`
    /// happily takes a plain filesystem path as a remote.
    fn local_git_fixture() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_package(tmp.path(), "shared_types");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(tmp.path())
                .output()
                .expect("git must be on PATH to run this test");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.test"]);
        git(&["config", "user.name", "Test"]);
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "initial"]);
        let sha = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let sha = String::from_utf8(sha.stdout).unwrap().trim().to_string();
        (tmp, sha)
    }

    #[test]
    fn resolves_a_git_dependency_from_a_local_remote() {
        let (repo, sha) = local_git_fixture();
        let consumer = tempfile::tempdir().unwrap();

        let dep = DependencyConfig {
            name: "shared_types".to_string(),
            source: DependencySource::Git {
                version: "1.0.0".to_string(),
                uri: repo.path().to_string_lossy().to_string(),
                commit: sha,
                hash: None,
            },
        };

        let resolved = resolve(
            &dep,
            consumer.path(),
            &consumer.path().join(".comline/deps-cache"),
        )
        .expect("resolves a pinned commit from a local remote");
        assert_eq!(resolved.context.schema_contexts.len(), 1);
        assert_eq!(
            Some(resolved.resolved_path),
            dep.package_dir(consumer.path()),
            "fetched exactly where `package_dir` says, so an editor finds it there"
        );
    }

    fn git_dep(repo: &Path, sha: &str) -> DependencyConfig {
        DependencyConfig {
            name: "shared_types".to_string(),
            source: DependencySource::Git {
                version: "1.0.0".to_string(),
                uri: repo.to_string_lossy().to_string(),
                commit: sha.to_string(),
                hash: None,
            },
        }
    }

    /// A fetch that fails partway (remote unreachable, flaky network) must
    /// not leave a checkout the next attempt mistakes for a finished one.
    #[test]
    fn a_failed_fetch_is_retried_instead_of_reusing_a_half_made_checkout() {
        let (repo, sha) = local_git_fixture();
        let consumer = tempfile::tempdir().unwrap();
        let cache = consumer.path().join(".comline/deps-cache");
        let dep = git_dep(repo.path(), &sha);

        // Take the remote away: `git init` and `remote add` succeed, the fetch fails.
        let away = repo.path().with_extension("away");
        std::fs::rename(repo.path(), &away).unwrap();
        let Err(error) = resolve(&dep, consumer.path(), &cache) else { panic!("the remote is gone") };
        assert!(error.to_string().contains("git fetch"), "names the failing git step: {error}");

        // Bring it back: the same pin now resolves, with nothing left over.
        std::fs::rename(&away, repo.path()).unwrap();
        resolve(&dep, consumer.path(), &cache).expect("the retry fetches again");
        let leftovers: Vec<_> = std::fs::read_dir(&cache)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(leftovers.len(), 1, "only the finished checkout remains: {leftovers:?}");
    }

    /// Checkouts an older version left half-made are repaired, not trusted.
    #[test]
    fn a_checkout_that_never_reached_the_commit_is_fetched_again() {
        let (repo, sha) = local_git_fixture();
        let consumer = tempfile::tempdir().unwrap();
        let cache = consumer.path().join(".comline/deps-cache");
        let dep = git_dep(repo.path(), &sha);

        let checkout = git_checkout_dir(&cache, &repo.path().to_string_lossy(), &sha);
        std::fs::create_dir_all(&checkout).unwrap();
        run_git(&checkout, &["init", "--quiet"]).unwrap();
        run_git(&checkout, &["remote", "add", "origin", &repo.path().to_string_lossy()]).unwrap();

        let resolved = resolve(&dep, consumer.path(), &cache).expect("repairs the half-made checkout");
        assert_eq!(resolved.context.schema_contexts.len(), 1);
    }

    #[test]
    fn a_wrong_commit_reports_the_git_error_every_time() {
        let (repo, _) = local_git_fixture();
        let consumer = tempfile::tempdir().unwrap();
        let cache = consumer.path().join(".comline/deps-cache");
        let dep = git_dep(repo.path(), &"0".repeat(40));

        for _ in 0..2 {
            let Err(error) = resolve(&dep, consumer.path(), &cache) else { panic!("no such commit") };
            assert!(error.to_string().contains("git "), "a git error, not a compile error: {error}");
        }
    }

    #[test]
    fn resolves_a_path_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let dep_dir = tmp.path().join("shared-types");
        write_minimal_package(&dep_dir, "shared_types");

        let dep = path_dep("shared_types", "shared-types", None);

        let resolved = resolve(&dep, tmp.path(), &tmp.path().join(".comline/deps-cache"))
            .expect("resolves cleanly");
        assert_eq!(resolved.name, "shared_types");
        assert_eq!(resolved.context.schema_contexts.len(), 1);
        assert_eq!(resolved.version, "0.0.0-unbuilt");
    }

    #[test]
    fn a_correct_declared_hash_is_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let dep_dir = tmp.path().join("shared-types");
        write_minimal_package(&dep_dir, "shared_types");

        // Resolve once, hash-free, to learn the real content hash — mirrors
        // how an author would pin it the first time.
        let unpinned = path_dep("shared_types", "shared-types", None);
        let first = resolve(
            &unpinned,
            tmp.path(),
            &tmp.path().join(".comline/deps-cache"),
        )
        .expect("resolves cleanly");
        let pin = format!("blake3:{}", first.content_hash.to_hex());

        let pinned = path_dep("shared_types", "shared-types", Some(&pin));
        let second = resolve(&pinned, tmp.path(), &tmp.path().join(".comline/deps-cache"))
            .expect("the just-computed pin must verify against itself");
        assert_eq!(second.content_hash.to_hex(), first.content_hash.to_hex());
    }

    #[test]
    fn a_wrong_declared_hash_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let dep_dir = tmp.path().join("shared-types");
        write_minimal_package(&dep_dir, "shared_types");

        let dep = path_dep(
            "shared_types",
            "shared-types",
            Some("blake3:0000000000000000000000000000000000000000000000000000000000000000"),
        );

        let err = match resolve(&dep, tmp.path(), &tmp.path().join(".comline/deps-cache")) {
            Err(e) => e,
            Ok(_) => panic!("a wrong pin must fail the build, not silently pass"),
        };
        assert!(err.to_string().contains("hash mismatch"), "got: {err}");
    }

    #[test]
    fn registry_sources_are_rejected_with_a_clear_error() {
        let dep = DependencyConfig {
            name: "std".to_string(),
            source: DependencySource::Registry {
                version: "1.0.0".to_string(),
                uri: "comline://registry.comline.io/std".to_string(),
                hash: None,
                signature: None,
            },
        };
        let tmp = tempfile::tempdir().unwrap();
        let err = match resolve(&dep, tmp.path(), &tmp.path().join(".comline/deps-cache")) {
            Err(e) => e,
            Ok(_) => panic!("registry sources aren't supported yet"),
        };
        assert!(err
            .to_string()
            .contains("registry dependencies aren't supported yet"));
    }
}
