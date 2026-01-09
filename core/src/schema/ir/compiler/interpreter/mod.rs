// Relative Modules
pub mod meta_stage;
pub mod object_stage;
pub mod semi_frozen;
pub mod incremental;

// Re-export for tests
pub use incremental::IncrementalInterpreter;
