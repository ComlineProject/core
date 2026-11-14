// The format gate's tests that need a build (core#268, step 1.2): the fixture
// packages the migrations are proved on, what a fresh build of each hashes
// to, and the live types held to the frozen copy of the current format. The
// checks that look at the repository instead (the seal, the bump check) are
// `cargo xtask`'s.

mod corpus;
mod golden;
mod live_frozen;
