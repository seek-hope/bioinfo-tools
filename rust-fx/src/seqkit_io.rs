//! Opt-in SeqKit I/O semantics for commands with fatal-input differentials.
//!
//! xopen 0.4 uses a 64 KiB output buffer. SeqKit's fatal exit discards its
//! pending bytes instead of running deferred Close. Keep that boundary without
//! buffering whole inputs or changing the other commands' output contracts.

use crate::io::{compression_levels, CommandError};
use bioinfo_fastx::{format_from_extension, reader::Writer, Format};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const CAPACITY: usize = 65_536;

struct GatedSink {
    sink: Box<dyn Write + Send>,
    active: Arc<AtomicBool>,
}

impl Write for GatedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.active.load(Ordering::Relaxed) {
            self.sink.write(bytes)
        } else {
            // Encoder destructors must not finalize an aborted output stream.
            Ok(bytes.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.active.load(Ordering::Relaxed) {
            self.sink.flush()
        } else {
            Ok(())
        }
    }
}

pub(crate) struct SeqkitOutput {
    writer: Option<Writer>,
    active: Arc<AtomicBool>,
    pending: Vec<u8>,
    plain: bool,
}

impl SeqkitOutput {
    pub(crate) fn create(path: Option<&str>, level: Option<i32>) -> Result<Self, CommandError> {
        let path = path.unwrap_or("-");
        let format = if path == "-" {
            Format::Plain
        } else {
            format_from_extension(Path::new(path))
        };
        let levels = compression_levels(format, level)?;
        let sink: Box<dyn Write + Send> = if path == "-" {
            Box::new(io::stdout())
        } else {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            Box::new(std::fs::File::create(path)?)
        };
        let active = Arc::new(AtomicBool::new(true));
        let writer = Writer::from_sink_with_levels(
            Box::new(GatedSink {
                sink,
                active: active.clone(),
            }),
            format,
            levels,
        )?;
        Ok(Self {
            writer: Some(writer),
            active,
            pending: Vec::with_capacity(CAPACITY),
            plain: format == Format::Plain,
        })
    }

    fn commit(&mut self) -> io::Result<()> {
        let writer = self.writer.as_mut().expect("unfinished output");
        writer.write_all(&self.pending)?;
        self.pending.clear();
        if self.plain {
            // The shared writer has its own buffer; plain xopen output does not
            // have a second buffer after the one whose boundary we reproduce.
            writer.flush()?;
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), CommandError> {
        self.commit()?;
        self.writer.take().expect("unfinished output").finish()?;
        Ok(())
    }
}

impl Write for SeqkitOutput {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len();
        // Go's bufio.Writer flushes only when a write exceeds available space.
        // An exactly full buffer remains pending until the next nonempty write.
        while bytes.len() > CAPACITY - self.pending.len() {
            if self.pending.is_empty() && self.plain {
                let writer = self.writer.as_mut().expect("unfinished output");
                writer.write_all(bytes)?;
                writer.flush()?;
                return Ok(count);
            }
            let take = CAPACITY - self.pending.len();
            self.pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            self.commit()?;
        }
        self.pending.extend_from_slice(bytes);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.commit()?;
        self.writer.as_mut().expect("unfinished output").flush()
    }
}

impl Drop for SeqkitOutput {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
    }
}

/// Check all named inputs before creating/truncating the output.
pub(crate) fn check_paths(inputs: &[String], output: Option<&str>) -> Result<(), CommandError> {
    for input in inputs.iter().filter(|path| *path != "-") {
        let metadata = std::fs::metadata(input)
            .map_err(|error| CommandError::io(format!("stat {input}: {error}")).with_code(255))?;
        if let Some(output) = output.filter(|path| *path != "-") {
            if let Ok(other) = std::fs::metadata(output) {
                #[cfg(unix)]
                let same = {
                    use std::os::unix::fs::MetadataExt;
                    metadata.dev() == other.dev() && metadata.ino() == other.ino()
                };
                #[cfg(not(unix))]
                let same = {
                    let _ = (&metadata, &other);
                    std::fs::canonicalize(input)? == std::fs::canonicalize(output)?
                };
                if same {
                    return Err(
                        CommandError::usage("input and output files cannot be the same")
                            .with_code(255),
                    );
                }
            }
        }
    }
    Ok(())
}
