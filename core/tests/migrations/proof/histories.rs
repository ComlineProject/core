// Scripted histories (core#268, step 1.4; proof.md, "The fixture corpus"):
// a sequence of builds and releases of a `Source`, and what the core of each
// test-only format writes for it. The same script is written by the core of
// the old format (the store that is migrated) and by the core of the current
// one (the fresh build the migrated store must equal, `forget` aside), so
// the numbering of versions, the commits made and the release records are
// one function of the script, shared: the comparison is fair by construction
// (proof.md: "a fixture's history must be one on which today's rules number
// the versions as the old core did").
//
// What a script can do:
//   Build(source)  a build: a commit, unless the source is the one the last
//                  build had ("a rebuild that changes nothing makes no
//                  commit"), numbered `0.<k+1>.0-dev.<n>` after `k` releases
//   Rebuild        a build of the source the last build had: no commit
//   Release        a release commit of the head's tree, `0.<k>.0`, with a
//                  release record whose `since` is the previous release
//   fork           the history a Git merge left with two heads: a sibling of
//                  the head, built on the head's parent, and a conflicted ref
//
// The fake clock: a commit's timestamp is a function of its place in the
// script and its author is "tester", so every write of a script is the same
// bytes.

use std::fs;
use std::path::Path;

use comline_core::package::build::cas::migrate::{Format, Sink};
use comline_core::package::build::cas::objects::{Blob, Commit, Release, Tree};
use comline_core::package::build::cas::storage::Hash;
use comline_core::package::build::cas::{
    update_ref, ObjectStore, VersionBump, REF_HEAD_MAIN_FILENAME,
};
use eyre::Result;

use super::formats::*;

#[derive(Debug, Clone)]
pub enum Event {
    Build(Source),
    Rebuild,
    Release {
        bump: VersionBump,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct History {
    pub name: &'static str,
    pub events: Vec<Event>,
    /// A build of this source that a second branch made from the head's
    /// parent: the store is left with two heads and a conflicted ref.
    pub fork: Option<Source>,
}

struct StoreSink<'a>(&'a ObjectStore);

impl Sink for StoreSink<'_> {
    fn blob(&mut self, content: Vec<u8>) -> Result<Hash> {
        self.0.write(&Blob::new(content).to_bytes()?)
    }

    fn tree(&mut self, tree: &Tree) -> Result<Hash> {
        self.0.write(&tree.to_bytes()?)
    }
}

const EPOCH: i64 = 1_700_000_000;

fn store_commit(commit: Commit, store: &ObjectStore) -> Hash {
    store.write(&commit.to_bytes().unwrap()).unwrap()
}

fn commit(
    tree: Hash,
    parents: Vec<Hash>,
    count: usize,
    message: String,
    version: String,
) -> Commit {
    Commit::new(
        tree,
        parents,
        "tester".into(),
        EPOCH + count as i64,
        message,
        version,
    )
}

/// Write `history` as the core of `format` would: a store of that format in
/// `root/.comline`.
fn write_with<F: Format>(
    format: &F,
    root: &Path,
    history: &History,
    project: impl Fn(&Source) -> F::Payload,
) {
    let store = ObjectStore::new(root);
    store.init().unwrap();
    fs::write(
        root.join(".comline/FORMAT_VERSION"),
        format.number().to_string(),
    )
    .unwrap();
    let mut sink = StoreSink(&store);
    let mut commits: Vec<Hash> = vec![];
    let mut trees: Vec<Hash> = vec![];
    let mut last_source: Option<&Source> = None;
    let mut last_release: Option<Hash> = None;
    let (mut releases, mut builds) = (0u32, 0u32);

    for event in &history.events {
        match event {
            Event::Rebuild => {}
            Event::Build(source) => {
                if last_source == Some(source) {
                    continue;
                }
                builds += 1;
                let tree = format.encode(&project(source), &mut sink).unwrap();
                let message = format!("commit {}", commits.len());
                let version = format!("0.{}.0-dev.{builds}", releases + 1);
                let built = commit(
                    tree,
                    commits.last().copied().into_iter().collect(),
                    commits.len(),
                    message,
                    version,
                );
                commits.push(store_commit(built, &store));
                trees.push(tree);
                last_source = Some(source);
            }
            Event::Release { bump, reason } => {
                let (Some(head), Some(tree)) = (commits.last().copied(), trees.last().copied())
                else {
                    continue;
                };
                releases += 1;
                builds = 0;
                let message = format!("commit {}", commits.len());
                let mut released = commit(
                    tree,
                    vec![head],
                    commits.len(),
                    message,
                    format!("0.{releases}.0"),
                );
                released.release = Some(Release {
                    computed: *bump,
                    applied: *bump,
                    reason: reason.clone(),
                    covered: false,
                    stable: false,
                    since: last_release,
                });
                let hash = store_commit(released, &store);
                commits.push(hash);
                trees.push(tree);
                last_release = Some(hash);
            }
        }
    }

    let head = *commits.last().expect("a history has a commit");
    match (&history.fork, commits.len()) {
        (Some(source), n) if n >= 2 => {
            let tree = format.encode(&project(source), &mut sink).unwrap();
            let message = "fork".to_string();
            let sibling = commit(
                tree,
                vec![commits[n - 2]],
                n,
                message,
                // Two people built the next version on their own branch.
                format!("0.{}.0-dev.{}", releases + 1, builds),
            );
            let sibling = store_commit(sibling, &store);
            commits.push(sibling);
            fs::create_dir_all(root.join(".comline/refs/heads")).unwrap();
            fs::write(
                root.join(".comline/refs/heads/main"),
                format!("<<<<<<< ours\n{head}\n=======\n{sibling}\n>>>>>>> theirs\n"),
            )
            .unwrap();
        }
        _ => update_ref(root, REF_HEAD_MAIN_FILENAME, &head).unwrap(),
    }
}

/// Write `history` as the core of format `number` would.
pub fn write_history(root: &Path, number: u32, history: &History) {
    match number {
        T0 => write_with(&Format0, root, history, fresh0),
        T1 => write_with(&Format1, root, history, fresh1),
        T2 => write_with(&Format2, root, history, fresh2),
        other => panic!("no test-only format {other}"),
    }
}

// ---------- the corpus ----------

pub fn scalar(name: &str, size: u32, kind: SourceKind, tags: &[&str]) -> SourceScalar {
    SourceScalar {
        name: name.into(),
        size,
        kind,
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
    }
}

pub fn source(title: &str, owner: &str, scalars: Vec<SourceScalar>) -> Source {
    Source {
        title: title.into(),
        owner: owner.into(),
        scalars,
    }
}

fn orders() -> Source {
    source(
        "Orders",
        "ana",
        vec![
            scalar("Order Id", 8, SourceKind::Int, &["key"]),
            scalar("total", 8, SourceKind::Float64, &["money", "fine"]),
            scalar("Note", 120, SourceKind::Text, &[]),
        ],
    )
}

fn build(source: Source) -> Event {
    Event::Build(source)
}

/// The corpus: histories that between them use every kind, a float of each
/// width, no floats at all, a rebuild that makes no commit, releases (with
/// and without a reason), two heads, and nothing in the package.
pub fn corpus() -> Vec<History> {
    let first = orders();
    let mut wider = first.clone();
    wider
        .scalars
        .push(scalar("ratio", 4, SourceKind::Float32, &["sensor", "unit"]));
    let mut resized = wider.clone();
    resized.scalars[0].size = 16;
    resized.scalars[2].tags = vec!["free text".to_string()];
    let mut renamed = resized.clone();
    renamed.title = "Orders v2".into();
    renamed.owner = "ben".into();
    renamed.scalars.remove(2);

    let mut ints = source(
        "Counters",
        "ana",
        vec![
            scalar("hits", 4, SourceKind::Int, &[]),
            scalar("Label", 32, SourceKind::Text, &["x"]),
        ],
    );
    let ints_first = ints.clone();
    ints.scalars[0].size = 8;

    vec![
        History {
            name: "grows",
            events: vec![
                build(first.clone()),
                build(wider.clone()),
                Event::Rebuild,
                build(wider.clone()),
                build(resized.clone()),
                build(renamed.clone()),
            ],
            fork: None,
        },
        History {
            name: "releases",
            events: vec![
                build(first.clone()),
                build(wider.clone()),
                Event::Release {
                    bump: VersionBump::Minor,
                    reason: None,
                },
                build(resized.clone()),
                Event::Release {
                    bump: VersionBump::Patch,
                    reason: Some("raised".into()),
                },
                build(renamed.clone()),
            ],
            fork: None,
        },
        History {
            name: "fork",
            events: vec![
                build(first.clone()),
                build(wider.clone()),
                build(resized.clone()),
            ],
            fork: Some(renamed),
        },
        History {
            name: "no_floats",
            events: vec![
                build(ints_first),
                build(ints),
                Event::Release {
                    bump: VersionBump::Minor,
                    reason: None,
                },
            ],
            fork: None,
        },
        History {
            name: "empty",
            events: vec![build(source("Nothing", "nobody", vec![]))],
            fork: None,
        },
        // A scalar with many tags: what a step that orders by a hash map's
        // iteration order shows itself on.
        History {
            name: "tagged",
            events: vec![build(source(
                "Tagged",
                "ana",
                vec![scalar(
                    "Many",
                    1,
                    SourceKind::Float32,
                    &[
                        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o",
                        "p", "q", "r", "s", "t",
                    ],
                )],
            ))],
            fork: None,
        },
    ]
}

/// The corpus history called `name`.
pub fn history(name: &str) -> History {
    corpus()
        .into_iter()
        .find(|history| history.name == name)
        .unwrap_or_else(|| panic!("no history called {name}"))
}

/// A project with `history` written as the core of format `number` would,
/// and a fetched dependency in `deps-cache/` (which is not part of a store
/// and has to come through a migration where it was).
pub fn project(history: &History, number: u32) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    write_history(dir.path(), number, history);
    let cached = dir.path().join(".comline/deps-cache/dep-1234");
    fs::create_dir_all(&cached).unwrap();
    fs::write(cached.join("config.idp"), "a fetched dependency").unwrap();
    dir
}
