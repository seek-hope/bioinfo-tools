//! Temporary artifacts with atomic commit.
//!
//! Every file output is produced next to its destination and only renamed on
//! success, so a failed or interrupted run cannot leave a half-written result
//! that looks complete.

use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct TempOutput {
    path: PathBuf,
    committed: bool,
}

impl TempOutput {
    /// Create a uniquely named file in `dir`.
    pub fn create(dir: &Path, prefix: &str) -> Result<TempOutput> {
        let pid = std::process::id();
        for attempt in 0..1000u32 {
            let candidate = dir.join(format!(".{prefix}.{pid}.{attempt}.tmp"));
            if candidate.exists() {
                continue;
            }
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    return Ok(TempOutput {
                        path: candidate,
                        committed: false,
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Error::io(&candidate, e)),
            }
        }
        Err(Error::Resource(format!(
            "could not create a temporary file in {}",
            dir.display()
        )))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically move the finished artifact onto `destination`.
    pub fn commit(mut self, destination: &Path) -> Result<()> {
        fs::rename(&self.path, destination).map_err(|e| Error::io(destination, e))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Canonical identity of a path. A destination that does not exist yet is
/// resolved through its parent, so two names for the same future file match.
fn identity(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(_) => {
            // `parent()` is empty for a bare file name, which must mean ".".
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let parent = fs::canonicalize(parent).map_err(|e| Error::io(parent, e))?;
            Ok(parent.join(path.file_name().unwrap_or_default()))
        }
    }
}

fn same_inode(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

fn same_file_error(first: &Path, second: &Path) -> Error {
    Error::usage_with_hint(
        format!(
            "{} and {} refer to the same file",
            first.display(),
            second.display()
        ),
        "give each input and output a distinct path; the input is read while writing",
    )
}

/// Reject an output path that aliases the input, including via symlinks or hard
/// links, before anything is written.
pub fn ensure_distinct(input: &Path, output: &Path) -> Result<()> {
    if identity(input)? == identity(output)? || same_inode(input, output) {
        return Err(same_file_error(input, output));
    }
    Ok(())
}

/// Reject an output set that aliases the input or that names the same file
/// twice, so a run cannot truncate its own input or silently overwrite one of
/// its own outputs.
pub fn ensure_distinct_outputs(input: &Path, outputs: &[&Path]) -> Result<()> {
    let stdin_input = input.as_os_str() == "-";
    let input_id = if stdin_input {
        PathBuf::new()
    } else {
        identity(input)?
    };
    let mut seen: Vec<(PathBuf, &Path)> = Vec::with_capacity(outputs.len());
    for &output in outputs {
        // `-` is a stream, not a path, so it cannot alias a file.
        if output.as_os_str() == "-" {
            continue;
        }
        let id = identity(output)?;
        if !stdin_input && (id == input_id || same_inode(input, output)) {
            return Err(same_file_error(input, output));
        }
        for (previous_id, previous) in &seen {
            if previous_id == &id || same_inode(previous, output) {
                return Err(same_file_error(previous, output));
            }
        }
        seen.push((id, output));
    }
    Ok(())
}
