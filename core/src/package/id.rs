//! The package id: `id = "..."` in `config.idp`.
//!
//! A random 128-bit id, written as 32 lowercase hex digits. Tooling writes
//! it: `comline new` for a new package, and `comline build` for one that
//! has none ([`ensure_in_config`]). It is the package's identity for the
//! compatibility check (`comline@2`): a rename keeps it, a copy must change
//! it, and changing it is a major change.

// Standard Uses
use std::path::Path;

// External Uses
use eyre::Result;

/// How many hex digits an id has.
pub const LENGTH: usize = 32;

/// A new random id.
///
/// Drawn from std's randomly seeded hasher, the clock and the process id,
/// mixed through blake3, so it needs no extra dependency. Not meant to be
/// secret, only unique.
pub fn generate() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = blake3::Hasher::new();
    for _ in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(0);
        hasher.update(&h.finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    hasher.update(&now.to_le_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    let hash = hasher.finalize();
    hash.to_hex()[..LENGTH].to_string()
}

/// Check an id's shape: 32 lowercase hex digits.
pub fn check(id: &str) -> std::result::Result<(), String> {
    if id.len() == LENGTH && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err(format!(
            "'id' should be {LENGTH} lowercase hex digits, like \"{}\"; `comline build` writes \
             one when it's missing",
            "0123456789abcdef".repeat(2)
        ))
    }
}

/// The `id = "..."` line for `id`.
pub fn config_line(id: &str) -> String {
    format!("id = \"{id}\"")
}

/// `source` with an `id` line added after the `congregation` line (or at
/// the top, without one), and the id added. `None` when `source` already
/// has an `id` key. Nothing else in the file changes.
pub fn add_to_config_source(source: &str) -> Option<(String, String)> {
    let has_id = source.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix("id").is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    if has_id {
        return None;
    }
    let id = generate();
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let line = config_line(&id);
    let mut out = String::with_capacity(source.len() + line.len() + 2);
    let mut inserted = false;
    for chunk in source.split_inclusive('\n') {
        out.push_str(chunk);
        if !inserted && chunk.trim_start().starts_with("congregation") {
            if !chunk.ends_with('\n') {
                out.push_str(newline);
            }
            out.push_str(&line);
            out.push_str(newline);
            inserted = true;
        }
    }
    if !inserted {
        out = format!("{line}{newline}{source}");
    }
    Some((out, id))
}

/// Add an `id` to the package's `config.idp` if it has none, editing the
/// file minimally. Returns the id added, or `None` when it had one.
pub fn ensure_in_config(package_path: &Path) -> Result<Option<String>> {
    let path = package_path.join("config.idp");
    let source = std::fs::read_to_string(&path)?;
    match add_to_config_source(&source) {
        Some((updated, id)) => {
            std::fs::write(&path, updated)?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_check_and_differ() {
        let (a, b) = (generate(), generate());
        assert!(check(&a).is_ok() && check(&b).is_ok());
        assert_ne!(a, b);
        assert!(check("ABC").is_err());
        assert!(check(&"g".repeat(LENGTH)).is_err());
    }

    #[test]
    fn the_id_goes_after_the_congregation_line_and_nothing_else_moves() {
        let source = "// a package\ncongregation test\nspecification_version = 1\n";
        let (out, id) = add_to_config_source(source).unwrap();
        assert_eq!(
            out,
            format!("// a package\ncongregation test\nid = \"{id}\"\nspecification_version = 1\n")
        );
        assert!(add_to_config_source(&out).is_none());
    }
}
