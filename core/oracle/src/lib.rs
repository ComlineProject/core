//! An independent oracle for the guarantee-safety invariant (core#223), the
//! package graphs it is checked over, and the generators that draw them.
//!
//! If a package compiles, every guarantee given up anywhere a protocol
//! reaches, through any chain of types, schemas and packages, has been
//! acknowledged in that package and accepted by every consumer of that
//! protocol. The check is that a build succeeds exactly when [`oracle`] says
//! it must, and when it does, each protocol's frozen given-up set is the
//! oracle's.
//!
//! - [`model`]: a graph, concretely, and the files it renders to.
//! - [`spec`]: the raw description a generator draws, and how it resolves.
//! - [`oracle`]: the prediction. Reads only the model.
//! - [`check`]: the comparison, and a whole run of a spec.
//! - `gen` (feature `proptest`) and `arb` (feature `arbitrary`): the same
//!   [`Spec`](spec::Spec), drawn by proptest's strategies and by the fuzzer.
//!
//! No dependency on `comline-core`: the oracle shares no code with the
//! compiler. The side that builds the files with core is `harness.rs`, which
//! each caller includes (a test and a fuzz target that can't both depend on a
//! crate that depends on core).

pub mod check;
pub mod model;
pub mod oracle;
pub mod spec;

#[cfg(feature = "arbitrary")]
pub mod arb;
#[cfg(feature = "proptest")]
pub mod gen;
