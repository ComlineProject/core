//! The format gate's checks that look at the repository and not at a build
//! (core#268, step 1.2): the seal of a superseded format, and what a format
//! bump (or a moved golden hash) has to come with. The tests that need a
//! build are in `core/tests/migrations`.
//!
//! Every check returns the problems it found, as sentences, and an empty
//! list when there is none.

pub mod bump;
pub mod repo;
pub mod seal;
