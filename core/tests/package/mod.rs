#[cfg(feature = "deps")]
mod compiler_cases;
#[cfg(feature = "deps")]
mod dependency_pin;
mod encodings;
mod fail_closed_imports;
mod from_sources;
mod handshake;
mod package_wire;
mod provider;
mod unverified;
#[cfg(feature = "deps")]
mod unverified_oracle;
mod wire;
mod schema_loading;
