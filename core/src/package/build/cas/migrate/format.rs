// The two things the engine is generic over: a format (how one version's
// payloads are read from and written to a store's objects) and a step (a
// function from one format's version to the next's, with its witness and the
// unknowns it writes), and the registry that holds them.
//
// Steps are typed on the formats' own payloads. The engine carries versions
// through a chain as `Erased` payloads, and the adapters here are the only
// code that downcasts; a payload of the wrong type is a misbuilt registry and
// an error, never a panic.

use std::collections::BTreeMap;
use std::fmt::Debug;

use comline_ir_formats::v3::types::UnknownRecord;
use eyre::{bail, eyre, Result};

use super::super::objects::{Blob, Tree};
use super::super::storage::Hash;
use super::super::ObjectStore;
use super::error::MigrateError;
use super::model::{Erased, UnknownDecl, Version};

/// Read access to a store's objects, by hash. The container is the same in
/// every format ([formats.md]), so this is what a format's decoder is given.
///
/// [formats.md]: https://github.com/ComlineProject/docs/blob/main/docs/docs/proposals/ir-migrations/formats.md
pub trait Objects {
    /// The `content` of the blob `hash`.
    fn blob(&self, hash: &Hash) -> Result<Vec<u8>>;
    /// The tree `hash`.
    fn tree(&self, hash: &Hash) -> Result<Tree>;
}

/// Where a format's encoder puts the objects it makes. It returns each
/// object's hash, so a format never computes one itself.
pub trait Sink {
    fn blob(&mut self, content: Vec<u8>) -> Result<Hash>;
    fn tree(&mut self, tree: &Tree) -> Result<Hash>;
}

impl Objects for ObjectStore {
    fn blob(&self, hash: &Hash) -> Result<Vec<u8>> {
        Ok(Blob::from_bytes(&self.read(hash)?)?.content)
    }

    fn tree(&self, hash: &Hash) -> Result<Tree> {
        Tree::from_bytes(&self.read(hash)?)
    }
}

/// Objects held in memory, by hash: what a plan encodes into, and what its
/// checks decode from. The bytes are the objects' CBOR, which is what a hash
/// covers; compression belongs to the files.
#[derive(Debug, Clone, Default)]
pub struct MemObjects {
    objects: BTreeMap<Hash, Vec<u8>>,
}

impl MemObjects {
    /// Add an object's CBOR and return its hash.
    pub fn put(&mut self, bytes: Vec<u8>) -> Hash {
        let hash = Hash::from_bytes(&bytes);
        self.objects.entry(hash).or_insert(bytes);
        hash
    }

    pub fn get(&self, hash: &Hash) -> Option<&[u8]> {
        self.objects.get(hash).map(Vec::as_slice)
    }

    /// Every object, by hash.
    pub fn iter(&self) -> impl Iterator<Item = (&Hash, &[u8])> {
        self.objects
            .iter()
            .map(|(hash, bytes)| (hash, bytes.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    fn need(&self, hash: &Hash) -> Result<&[u8]> {
        self.get(hash)
            .ok_or_else(|| eyre!("object {hash} is not in the new store"))
    }
}

impl Sink for MemObjects {
    fn blob(&mut self, content: Vec<u8>) -> Result<Hash> {
        Ok(self.put(Blob::new(content).to_bytes()?))
    }

    fn tree(&mut self, tree: &Tree) -> Result<Hash> {
        Ok(self.put(tree.to_bytes()?))
    }
}

impl Objects for MemObjects {
    fn blob(&self, hash: &Hash) -> Result<Vec<u8>> {
        Ok(Blob::from_bytes(self.need(hash)?)?.content)
    }

    fn tree(&self, hash: &Hash) -> Result<Tree> {
        Tree::from_bytes(self.need(hash)?)
    }
}

/// What a payload has to be to travel through the engine.
pub trait PayloadBound: Clone + PartialEq + Debug + Send + Sync + 'static {}
impl<T: Clone + PartialEq + Debug + Send + Sync + 'static> PayloadBound for T {}

/// One IR format: how a version's root tree becomes its payload and back.
/// `decode` and `encode` are the only code that touches the format's bytes;
/// they never guess (bytes that don't decode are an error, not an empty
/// value), and `encode(decode(root)) == root`: the encoding is canonical.
pub trait Format: Send + Sync + 'static {
    /// The decoded payloads of one version (everything under its root tree).
    type Payload: PayloadBound;

    /// The number `FORMAT_VERSION` holds.
    fn number(&self) -> u32;

    /// The payload of the version whose root tree is `root`.
    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<Self::Payload>;

    /// Write the payload's objects and return the root tree's hash.
    fn encode(&self, payload: &Self::Payload, sink: &mut dyn Sink) -> Result<Hash>;

    /// Every part of the payload the format records as unknown, typed
    /// (`Known<T>`) or listed in its `unknown` entry. Each record's `path` is
    /// a JSON pointer into the payload's model, and `step` the step that
    /// wrote it, `"<from>-<to>"`.
    fn unknowns(&self, payload: &Self::Payload) -> Vec<UnknownRecord>;
}

/// A step `N -> N+1` (steps.md): total and deterministic over the formats'
/// payloads, with the witness that shows it lost nothing and the list of what
/// it writes as unknown. It reads only the version it is given.
pub trait Step: Send + Sync + 'static {
    type From: PayloadBound;
    type To: PayloadBound;

    /// `N`.
    fn from_format(&self) -> u32;

    /// `N + 1`.
    fn to_format(&self) -> u32 {
        self.from_format() + 1
    }

    /// The paths this step writes as unknown, because `N` never recorded
    /// them, each with the reason.
    fn unknown(&self) -> Vec<UnknownDecl>;

    /// `N` to `N+1`. An error means the step could not produce what a version
    /// can't be without (its `version`, `release`, parents): the migration is
    /// refused.
    fn up(&self, version: Version<Self::From>) -> Result<Version<Self::To>>;

    /// The witness: `down(up(v)) == v`. Defined on the image of `up`.
    fn down(&self, version: Version<Self::To>) -> Result<Version<Self::From>>;
}

pub(super) trait DynFormat: Send + Sync {
    fn number(&self) -> u32;
    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<Erased>;
    fn encode(&self, payload: &Erased, sink: &mut dyn Sink) -> Result<Hash>;
    fn unknowns(&self, payload: &Erased) -> Result<Vec<UnknownRecord>>;
    /// Decoding `root` from `objects` gives `payload`, and encoding that
    /// again gives `root`: the canonical-encoding check.
    fn canonical(&self, payload: &Erased, objects: &dyn Objects, root: &Hash) -> Result<()>;
}

struct FormatAdapter<F>(F);

fn typed<P: PayloadBound>(payload: &Erased, number: u32) -> Result<&P> {
    payload
        .get::<P>()
        .ok_or_else(|| eyre!("a payload that is not format {number}'s reached its codec"))
}

impl<F: Format> DynFormat for FormatAdapter<F> {
    fn number(&self) -> u32 {
        self.0.number()
    }

    fn decode(&self, objects: &dyn Objects, root: &Hash) -> Result<Erased> {
        Ok(Erased::new(self.0.decode(objects, root)?))
    }

    fn encode(&self, payload: &Erased, sink: &mut dyn Sink) -> Result<Hash> {
        self.0
            .encode(typed::<F::Payload>(payload, self.number())?, sink)
    }

    fn unknowns(&self, payload: &Erased) -> Result<Vec<UnknownRecord>> {
        Ok(self
            .0
            .unknowns(typed::<F::Payload>(payload, self.number())?))
    }

    fn canonical(&self, payload: &Erased, objects: &dyn Objects, root: &Hash) -> Result<()> {
        let payload = typed::<F::Payload>(payload, self.number())?;
        let decoded = self.0.decode(objects, root)?;
        if &decoded != payload {
            bail!("decoding what was encoded gives a different value");
        }
        let mut scratch = MemObjects::default();
        let again = self.0.encode(&decoded, &mut scratch)?;
        if again != *root {
            bail!("encoding it again gives another root tree ({again}, not {root})");
        }
        Ok(())
    }
}

pub(super) trait DynStep: Send + Sync {
    fn from_format(&self) -> u32;
    fn to_format(&self) -> u32;
    fn unknown(&self) -> Vec<UnknownDecl>;
    fn up(&self, version: &Version<Erased>) -> Result<Version<Erased>>;
    /// Whether `down(after) == before`.
    fn witness(&self, before: &Version<Erased>, after: &Version<Erased>) -> Result<bool>;
}

struct StepAdapter<S>(S);

fn typed_version<P: PayloadBound>(version: &Version<Erased>, number: u32) -> Result<Version<P>> {
    let payload = typed::<P>(&version.payload, number)?.clone();
    Ok(version.clone().with_payload(payload))
}

impl<S: Step> DynStep for StepAdapter<S> {
    fn from_format(&self) -> u32 {
        self.0.from_format()
    }

    fn to_format(&self) -> u32 {
        self.0.to_format()
    }

    fn unknown(&self) -> Vec<UnknownDecl> {
        self.0.unknown()
    }

    fn up(&self, version: &Version<Erased>) -> Result<Version<Erased>> {
        let up = self
            .0
            .up(typed_version::<S::From>(version, self.from_format())?)?;
        let payload = Erased::new(up.payload.clone());
        Ok(up.with_payload(payload))
    }

    fn witness(&self, before: &Version<Erased>, after: &Version<Erased>) -> Result<bool> {
        let back = self
            .0
            .down(typed_version::<S::To>(after, self.to_format())?)?;
        Ok(back == typed_version::<S::From>(before, self.from_format())?)
    }
}

/// The formats and steps an engine has, and the format it writes (the
/// `target`). The registry of a release is [`Registry::current`]; test-only
/// formats (core#268, step 1.4) are registered the same way.
pub struct Registry {
    target: u32,
    formats: BTreeMap<u32, Box<dyn DynFormat>>,
    /// Steps by the format they leave.
    steps: BTreeMap<u32, Box<dyn DynStep>>,
}

impl Registry {
    /// An empty registry that writes format `target`.
    pub fn new(target: u32) -> Self {
        Self {
            target,
            formats: BTreeMap::new(),
            steps: BTreeMap::new(),
        }
    }

    /// What this comline writes: format 3, which can be read and written, and
    /// no step: it is the first format with a codec here, so a format-3 store
    /// has an empty plan. The first step arrives with format 4.
    pub fn current() -> Self {
        Self::new(super::super::object_store::CURRENT_IR_FORMAT_VERSION).with_format(super::V3)
    }

    /// Add a format. A second of the same number replaces the first.
    pub fn with_format<F: Format>(mut self, format: F) -> Self {
        self.formats
            .insert(format.number(), Box::new(FormatAdapter(format)));
        self
    }

    /// Add a step. Panics if the step isn't `N -> N+1` or if there is
    /// already one from `N`: a "shortcut" would be a second implementation of
    /// the same function (steps.md, "Chains").
    pub fn with_step<S: Step>(mut self, step: S) -> Self {
        assert_eq!(
            step.to_format(),
            step.from_format() + 1,
            "a step is N -> N+1, not {} -> {}",
            step.from_format(),
            step.to_format()
        );
        let from = step.from_format();
        let earlier = self.steps.insert(from, Box::new(StepAdapter(step)));
        assert!(earlier.is_none(), "two steps from format {from}");
        self
    }

    /// The format this registry writes.
    pub fn target(&self) -> u32 {
        self.target
    }

    /// Whether the steps from `from` reach the target, and every format on
    /// the way (and the target) can be read.
    pub fn can_migrate_from(&self, from: u32) -> bool {
        from <= self.target && self.chain(from).is_ok()
    }

    pub(super) fn format(&self, number: u32) -> Result<&dyn DynFormat, MigrateError> {
        self.formats
            .get(&number)
            .map(|format| &**format)
            .ok_or(MigrateError::NoFormat { number })
    }

    /// The steps from `from` to the target, in order. Empty when `from` is the
    /// target. An error when there is no step from some `n` on the way, or a
    /// format on the way can't be read.
    pub(super) fn chain(&self, from: u32) -> Result<Vec<&dyn DynStep>, MigrateError> {
        let mut steps = vec![];
        for number in from..self.target {
            let step = self
                .steps
                .get(&number)
                .ok_or(MigrateError::NoStep { from: number })?;
            steps.push(&**step);
        }
        for number in from..=self.target {
            self.format(number)?;
        }
        Ok(steps)
    }
}
