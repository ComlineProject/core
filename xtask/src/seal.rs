//! The seal: `formats/MANIFEST` records the blake3 of each superseded
//! format's directory, so that a frozen type, a codec or a document can't
//! change after its format was replaced.
//!
//! One line per sealed format: `v<N> blake3:<hex> # <why it was sealed>`.
//! The current format is not sealed, and every older one is.

use std::fs;
use std::path::Path;

use crate::repo::{Repo, MANIFEST};

/// The header a manifest starts with.
const HEADER: &str = "\
# The seal of each superseded IR format (core#268): the blake3 of its
# directory under src/, over every file's relative path and bytes in path
# order. `cargo xtask check-seal` fails when one differs; a format is sealed
# when the next is introduced, with `cargo xtask seal-format <N> --why <text>`,
# which is also the only way to move a seal.
";

/// A line of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seal {
    pub format: u32,
    pub hash: String,
    pub why: String,
}

/// The hash of the directory `dir`: for each file under it in path order,
/// its path relative to `dir` (with `/`), a zero byte, its length as a
/// little-endian `u64`, and its bytes.
pub fn dir_hash(dir: &Path) -> Result<String, String> {
    fn walk(base: &Path, dir: &Path, files: &mut Vec<String>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(base, &path, files)?;
            } else {
                let relative = path.strip_prefix(base).map_err(|e| e.to_string())?;
                let parts: Vec<_> = relative.iter().map(|p| p.to_string_lossy()).collect();
                files.push(parts.join("/"));
            }
        }
        Ok(())
    }

    let mut files = vec![];
    walk(dir, dir, &mut files)?;
    files.sort();
    let mut hasher = blake3::Hasher::new();
    for file in &files {
        let bytes = fs::read(dir.join(file)).map_err(|e| format!("can't read {file}: {e}"))?;
        hasher.update(file.as_bytes());
        hasher.update(&[0]);
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

/// The seals in `text`.
pub fn parse(text: &str) -> Result<Vec<Seal>, String> {
    let mut seals = vec![];
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (entry, why) = line.split_once('#').unwrap_or((line, ""));
        let mut words = entry.split_whitespace();
        let (Some(format), Some(hash), None) = (words.next(), words.next(), words.next()) else {
            return Err(format!(
                "MANIFEST line {}: expected `v<N> blake3:<hex> # <why>`",
                number + 1
            ));
        };
        let format = format
            .strip_prefix('v')
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| format!("MANIFEST line {}: `{format}` is not a format", number + 1))?;
        let valid = hash.strip_prefix("blake3:").is_some_and(|h| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if !valid {
            return Err(format!(
                "MANIFEST line {}: `{hash}` is not `blake3:` and 64 hex digits",
                number + 1
            ));
        }
        if seals.iter().any(|s: &Seal| s.format == format) {
            return Err(format!(
                "MANIFEST line {}: format {format} is sealed twice",
                number + 1
            ));
        }
        seals.push(Seal {
            format,
            hash: hash.to_string(),
            why: why.trim().to_string(),
        });
    }
    Ok(seals)
}

fn render(seals: &[Seal]) -> String {
    let mut text = HEADER.to_string();
    for seal in seals {
        text.push_str(&format!("v{} {} # {}\n", seal.format, seal.hash, seal.why));
    }
    text
}

/// Everything wrong with the seals: a superseded format that isn't sealed,
/// one that changed since it was, and a seal on the current format or on one
/// that is gone.
pub fn verify(repo: &Repo) -> Result<Vec<String>, String> {
    let current = repo.current_format()?;
    let seals = parse(&repo.read(MANIFEST)?)?;
    let mut problems = vec![];
    for seal in &seals {
        let dir = repo.format_dir(seal.format);
        if seal.format >= current {
            problems.push(format!(
                "format {} is sealed, but the current format is {current}: a format is sealed when the next one is introduced",
                seal.format
            ));
        } else if !dir.is_dir() {
            problems.push(format!(
                "format {} is sealed, but its directory is gone",
                seal.format
            ));
        } else if dir_hash(&dir)? != seal.hash {
            problems.push(format!(
                "sealed format {} changed: its frozen types, codec or document differ from the seal in MANIFEST. \
                 A superseded format is never edited; if this is deliberate, move the seal with \
                 `cargo xtask seal-format {} --why <reason>`",
                seal.format, seal.format
            ));
        }
    }
    for format in repo.formats()? {
        if format < current && !seals.iter().any(|s| s.format == format) {
            problems.push(format!(
                "format {format} is superseded by {current} and not sealed: `cargo xtask seal-format {format} --why <reason>`"
            ));
        }
    }
    Ok(problems)
}

/// Record the seal of `format`, replacing the one it has. `why` is written
/// beside it.
pub fn seal(repo: &Repo, format: u32, why: &str) -> Result<(), String> {
    if why.trim().is_empty() {
        return Err("a seal is moved with a reason: `--why <text>`".into());
    }
    let current = repo.current_format()?;
    if format >= current {
        return Err(format!(
            "format {format} is the current format (or newer) and can't be sealed yet"
        ));
    }
    let dir = repo.format_dir(format);
    if !dir.is_dir() {
        return Err(format!("format {format} has no directory"));
    }
    let mut seals = parse(&repo.read(MANIFEST)?)?;
    seals.retain(|s| s.format != format);
    seals.push(Seal {
        format,
        hash: dir_hash(&dir)?,
        why: why.trim().to_string(),
    });
    seals.sort_by_key(|s| s.format);
    fs::write(repo.path(MANIFEST), render(&seals)).map_err(|e| format!("can't write MANIFEST: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bump::tests::scratch;

    #[test]
    fn a_manifest_parses_and_renders() {
        let text = format!(
            "{HEADER}v3 blake3:{} # format 4 is introduced\n",
            "ab".repeat(32)
        );
        let seals = parse(&text).unwrap();
        assert_eq!(seals.len(), 1);
        assert_eq!(
            (seals[0].format, seals[0].why.as_str()),
            (3, "format 4 is introduced")
        );
        assert_eq!(render(&seals), text);
        assert!(parse("v3 sha:abc").is_err());
        assert!(parse("three blake3:00").is_err());
        let twice = format!("v3 blake3:{0}\nv3 blake3:{0}\n", "ab".repeat(32));
        assert!(parse(&twice).is_err());
    }

    #[test]
    fn the_hash_follows_every_file_and_its_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "one").unwrap();
        let first = dir_hash(dir.path()).unwrap();
        assert_eq!(dir_hash(dir.path()).unwrap(), first);
        fs::write(dir.path().join("a"), "two").unwrap();
        let changed = dir_hash(dir.path()).unwrap();
        assert_ne!(changed, first);
        fs::rename(dir.path().join("a"), dir.path().join("b")).unwrap();
        assert_ne!(dir_hash(dir.path()).unwrap(), changed);
    }

    #[test]
    fn the_current_format_is_not_sealed_and_an_older_one_has_to_be() {
        let repo = scratch(4);
        let problems = verify(&repo).unwrap();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("format 3 is superseded by 4 and not sealed")),
            "{problems:?}"
        );
        assert!(
            seal(&repo, 4, "why").is_err(),
            "the current format can't be sealed"
        );
        assert!(seal(&repo, 3, " ").is_err(), "a seal needs a reason");

        seal(&repo, 3, "format 4 is introduced").unwrap();
        assert_eq!(verify(&repo).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_sealed_format_that_changes_fails_until_the_seal_is_moved_with_a_reason() {
        let repo = scratch(4);
        seal(&repo, 3, "format 4 is introduced").unwrap();

        fs::write(repo.format_dir(3).join("types.rs"), "// edited\n").unwrap();
        let problems = verify(&repo).unwrap();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("sealed format 3 changed")),
            "{problems:?}"
        );

        seal(&repo, 3, "a typo in a document").unwrap();
        assert_eq!(verify(&repo).unwrap(), Vec::<String>::new());
        assert!(fs::read_to_string(repo.path(MANIFEST))
            .unwrap()
            .contains("# a typo in a document"));
    }

    #[test]
    fn a_seal_on_the_current_format_fails() {
        let repo = scratch(3);
        let manifest = format!("{HEADER}v3 blake3:{} # early\n", "00".repeat(32));
        fs::write(repo.path(MANIFEST), manifest).unwrap();
        let problems = verify(&repo).unwrap();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("is sealed, but the current format is 3")),
            "{problems:?}"
        );
    }
}
