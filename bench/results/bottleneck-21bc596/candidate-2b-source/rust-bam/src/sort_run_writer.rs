//! Temporary sort runs use level-1 BGZF, with ordered block compression.

use std::fs::File;
use std::io::Write;
use std::num::NonZero;

use noodles::sam::alignment::io::Write as _;
use noodles::{bam, bgzf, sam};

use crate::error::{Error, Result};
use crate::io_util::Record;

pub(crate) enum RunWriter {
    Serial(bam::io::Writer<bgzf::io::Writer<File>>),
    Parallel(bam::io::Writer<bgzf::io::MultithreadedWriter<File>>),
}

impl RunWriter {
    pub(crate) fn new(file: File, workers: usize) -> Self {
        if workers > 1 {
            let writer = bgzf::io::multithreaded_writer::Builder::default()
                .set_compression_level(bgzf::io::writer::CompressionLevel::FAST)
                .set_worker_count(NonZero::new(workers).expect("workers > 1"))
                .build_from_writer(file);
            Self::Parallel(bam::io::Writer::from(writer))
        } else {
            let writer = bgzf::io::writer::Builder::default()
                .set_compression_level(bgzf::io::writer::CompressionLevel::FAST)
                .build_from_writer(file);
            Self::Serial(bam::io::Writer::from(writer))
        }
    }

    pub(crate) fn write_header(&mut self, header: &sam::Header) -> Result<()> {
        match self {
            Self::Serial(writer) => writer.write_header(header),
            Self::Parallel(writer) => writer.write_header(header),
        }
        .map_err(|error| Error::io(format!("cannot write BAM header: {error}")))
    }

    pub(crate) fn write_record(&mut self, header: &sam::Header, record: &Record) -> Result<()> {
        match self {
            Self::Serial(writer) => writer.write_alignment_record(header, record),
            Self::Parallel(writer) => writer.write_alignment_record(header, record),
        }
        .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))
    }

    pub(crate) fn write_encoded_record(&mut self, frame: &[u8]) -> Result<()> {
        match self {
            Self::Serial(writer) => writer.get_mut().write_all(frame),
            Self::Parallel(writer) => writer.get_mut().write_all(frame),
        }
        .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))
    }

    pub(crate) fn finish(self) -> Result<File> {
        match self {
            Self::Serial(writer) => writer.into_inner().finish(),
            Self::Parallel(writer) => writer.into_inner().finish(),
        }
        .map_err(|error| Error::io(format!("cannot finish BGZF stream: {error}")))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn finishing_a_temporary_run_reports_output_failure_with_any_worker_count() {
        for workers in [0, 1, 4] {
            let file = File::options().write(true).open("/dev/full").unwrap();
            let mut writer = RunWriter::new(file, workers);
            writer.write_header(&sam::Header::default()).unwrap();
            let error = writer.finish().unwrap_err();
            assert!(
                error.to_string().starts_with("cannot finish BGZF stream:"),
                "{error}"
            );
        }
    }
}
