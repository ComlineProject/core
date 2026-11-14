//! Where things are in the repository, and reading them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The file that holds `CURRENT_IR_FORMAT_VERSION`.
pub const STAMP_FILE: &str = "core/src/package/build/cas/object_store.rs";
/// The frozen formats' crate.
pub const FORMATS: &str = "core/formats";
/// The seals.
pub const MANIFEST: &str = "core/formats/MANIFEST";
/// The golden fresh-build hashes of the fixture packages.
pub const EXPECTED: &str = "core/tests/migrations/expected";

/// The files every format's directory has.
pub const FORMAT_FILES: [&str; 4] = ["SCHEMA.md", "types.rs", "codec.rs", "ledger.toml"];

/// A checkout of the repository.
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The checkout the running `xtask` belongs to.
    pub fn here() -> Self {
        Self::new(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn format_dir(&self, format: u32) -> PathBuf {
        self.path(&format!("{FORMATS}/src/v{format}"))
    }

    pub fn read(&self, relative: &str) -> Result<String, String> {
        fs::read_to_string(self.path(relative)).map_err(|e| format!("can't read {relative}: {e}"))
    }

    /// The formats that have a directory in the crate, ascending.
    pub fn formats(&self) -> Result<Vec<u32>, String> {
        let src = self.path(&format!("{FORMATS}/src"));
        let mut formats: Vec<u32> = fs::read_dir(&src)
            .map_err(|e| format!("can't read {}: {e}", src.display()))?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('v')?.parse().ok())
            .collect();
        formats.sort_unstable();
        Ok(formats)
    }

    /// `CURRENT_IR_FORMAT_VERSION` in the working tree.
    pub fn current_format(&self) -> Result<u32, String> {
        current_format_in(&self.read(STAMP_FILE)?)
    }

    /// Run `git` in the checkout.
    pub fn git(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| format!("can't run git: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// A file as `revision` had it, `None` if it had none.
    pub fn file_at(&self, revision: &str, relative: &str) -> Option<String> {
        self.git(&["show", &format!("{revision}:{relative}")]).ok()
    }
}

/// The number in `pub const CURRENT_IR_FORMAT_VERSION: u32 = <n>;`.
pub fn current_format_in(source: &str) -> Result<u32, String> {
    let line = source
        .lines()
        .find(|l| {
            l.trim_start()
                .starts_with("pub const CURRENT_IR_FORMAT_VERSION")
        })
        .ok_or("no `CURRENT_IR_FORMAT_VERSION` in the stamp file")?;
    line.split('=')
        .nth(1)
        .and_then(|v| v.trim().trim_end_matches(';').trim().parse().ok())
        .ok_or_else(|| format!("can't read a number from `{}`", line.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stamp_is_read_from_its_constant() {
        let source = "/// docs\npub const CURRENT_IR_FORMAT_VERSION: u32 = 3;\nfn x() {}\n";
        assert_eq!(current_format_in(source), Ok(3));
        assert!(current_format_in("const X: u32 = 3;").is_err());
        assert!(current_format_in("pub const CURRENT_IR_FORMAT_VERSION: u32 = three;").is_err());
    }
}
