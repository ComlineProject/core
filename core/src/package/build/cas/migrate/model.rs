// What a step works on: one version of a store, in the model of one format
// (steps.md, "The model"). A step never sees a hash. A reference to another
// version, a parent or a release's `since`, is a `VersionId`, the version's
// place in the chain; the engine turns ids back into hashes when it encodes,
// so a step author can't break the Merkle structure.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use super::super::version::VersionBump;

/// A version's place in the store's history: parents come before children,
/// so a parent's or a `since`'s id is always smaller than the version's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionId(pub usize);

/// What a release was, as the commit records it, with `since` as an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRecord {
    pub computed: VersionBump,
    pub applied: VersionBump,
    pub reason: Option<String>,
    pub covered: bool,
    pub stable: bool,
    pub since: Option<VersionId>,
}

/// One version of a store: the commit's own fields, which no step changes
/// (the engine checks that they come out as they went in), and the decoded
/// payloads of its format, `P`. What a format could not know is part of `P`:
/// a `Known<T>` field, or the format's `unknown` list (steps.md, "Unknown").
#[derive(Debug, Clone, PartialEq)]
pub struct Version<P> {
    pub id: VersionId,
    /// First parent first; a second parent is what a reconcile recorded.
    pub parents: Vec<VersionId>,
    pub version: String,
    pub release: Option<ReleaseRecord>,
    pub author: String,
    pub timestamp: i64,
    pub message: String,
    pub payload: P,
}

impl<P> Version<P> {
    /// The same version with another payload.
    pub fn with_payload<Q>(self, payload: Q) -> Version<Q> {
        Version {
            id: self.id,
            parents: self.parents,
            version: self.version,
            release: self.release,
            author: self.author,
            timestamp: self.timestamp,
            message: self.message,
            payload,
        }
    }

    /// The commit's own fields, without the payload.
    pub(crate) fn same_commit_fields<Q>(&self, other: &Version<Q>) -> bool {
        self.id == other.id
            && self.parents == other.parents
            && self.version == other.version
            && self.release == other.release
            && self.author == other.author
            && self.timestamp == other.timestamp
            && self.message == other.message
    }
}

/// A payload of some format, with its type forgotten: how the engine carries
/// a version through a chain of steps whose formats it doesn't know the types
/// of. Only the typed adapters of [`Registry`](super::Registry) look inside.
#[derive(Clone)]
pub struct Erased(Arc<dyn Any + Send + Sync>);

impl Erased {
    pub fn new<P: Any + Send + Sync>(payload: P) -> Self {
        Self(Arc::new(payload))
    }

    pub fn get<P: Any>(&self) -> Option<&P> {
        self.0.downcast_ref::<P>()
    }
}

impl fmt::Debug for Erased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Erased(..)")
    }
}

/// A path a step writes as unknown, because the format it reads never
/// recorded it. A pattern over JSON pointers into the new format's model:
/// `*` stands for one segment and `**` for any number, so
/// `/protocols/*/given_up/*/chain` covers every chain. Written down in the
/// format's `SCHEMA.md`, and what the engine holds a step to: an unknown at a
/// path no pattern of the step covers fails the migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownDecl {
    pub pattern: String,
    pub reason: String,
}

impl UnknownDecl {
    pub fn new(pattern: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            reason: reason.into(),
        }
    }
}

/// Whether the JSON pointer `path` is one `pattern` covers (`*`: one segment,
/// `**`: any number of them, including none).
pub fn path_matches(pattern: &str, path: &str) -> bool {
    fn go(pattern: &[&str], path: &[&str]) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => (0..=path.len()).any(|skipped| go(rest, &path[skipped..])),
            Some((&"*", rest)) => !path.is_empty() && go(rest, &path[1..]),
            Some((segment, rest)) => path.first() == Some(segment) && go(rest, &path[1..]),
        }
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    go(&pattern, &path)
}

#[cfg(test)]
mod tests {
    use super::path_matches;

    #[test]
    fn a_pattern_covers_the_paths_it_names() {
        assert!(path_matches("/label", "/label"));
        assert!(!path_matches("/label", "/labels"));
        assert!(!path_matches("/label", "/label/x"));
        assert!(path_matches("/a/*/c", "/a/b/c"));
        assert!(!path_matches("/a/*/c", "/a/c"));
        assert!(!path_matches("/a/*/c", "/a/b/d/c"));
        assert!(path_matches("/a/**/c", "/a/c"));
        assert!(path_matches("/a/**/c", "/a/b/d/c"));
        assert!(path_matches("/**", "/anything/at/all"));
        assert!(!path_matches("/a/**/c", "/b/c"));
    }
}
