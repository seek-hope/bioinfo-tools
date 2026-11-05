//! Initial sort chunks retaining small, ordinarily encoded BAM frames.
//!
//! Input still passes through the full RecordBuf decoder. Chunk accounting and
//! spill boundaries use the existing record_memory estimate. Serialization errors
//! are deferred by retaining the original record, so sorted emission determines
//! their visibility. SAM output uses owned records throughout.

use std::io::{self, Write};

use noodles::{bam, sam, sam::alignment::io::Write as _};

use crate::{
    encoder_record::EncoderRecord,
    io_util::{BamWriter, Record},
    sort_order::SortKey,
    Result,
};

const FRAME_LIMIT: usize = 64 * 1024;

pub(crate) trait Chunk {
    fn len(&self) -> usize;
    fn order(&self) -> Vec<usize>;
    fn push_record(
        &mut self,
        header: &sam::Header,
        key: SortKey,
        record: &mut Record,
        estimate: u64,
    );
    fn emit<W: Write>(
        &self,
        position: usize,
        header: &sam::Header,
        writer: &mut BamWriter<W>,
    ) -> Result<usize>;
    fn take(&mut self, position: usize, record: &mut Record, frame: &mut Vec<u8>);
    fn clear(&mut self);
}

impl Chunk for Vec<(SortKey, Record)> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
    fn order(&self) -> Vec<usize> {
        crate::radix_sort::indices(self)
    }
    fn push_record(&mut self, _: &sam::Header, key: SortKey, record: &mut Record, _: u64) {
        self.push((key, std::mem::take(record)));
    }
    fn emit<W: Write>(
        &self,
        position: usize,
        header: &sam::Header,
        writer: &mut BamWriter<W>,
    ) -> Result<usize> {
        let (key, record) = &self[position];
        writer.write_record(header, record)?;
        Ok(key.input_index)
    }
    fn take(&mut self, position: usize, record: &mut Record, frame: &mut Vec<u8>) {
        frame.clear();
        *record = std::mem::take(&mut self[position].1);
    }
    fn clear(&mut self) {
        Vec::clear(self);
    }
}

enum Stored {
    Frame { start: usize, end: usize },
    Original(Box<Record>),
}

#[derive(Default)]
struct FrameSink {
    frame: Vec<u8>,
    oversized: bool,
}

impl FrameSink {
    fn clear(&mut self) {
        self.frame.clear();
        self.oversized = false;
    }
}

impl Write for FrameSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.oversized {
            // The ordinary BAM writer publishes the four-byte length before the
            // body. Check it before copying a possibly large auxiliary payload.
            let prefix = (4usize.saturating_sub(self.frame.len())).min(bytes.len());
            self.frame.extend_from_slice(&bytes[..prefix]);
            if self.frame.len() >= 4 {
                let length = u32::from_le_bytes(self.frame[..4].try_into().unwrap()) as u64 + 4;
                if length > FRAME_LIMIT as u64
                    || self.frame.len() + bytes.len() - prefix > FRAME_LIMIT
                {
                    self.oversized = true;
                    self.frame.clear();
                    return Ok(bytes.len());
                }
            }
            self.frame.extend_from_slice(&bytes[prefix..]);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Compact {
    entries: Vec<(SortKey, Stored)>,
    arena: Vec<u8>,
    encoder: bam::io::Writer<FrameSink>,
}

impl Default for Compact {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            arena: Vec::new(),
            encoder: bam::io::Writer::from(FrameSink::default()),
        }
    }
}

impl Chunk for Compact {
    fn len(&self) -> usize {
        self.entries.len()
    }
    fn order(&self) -> Vec<usize> {
        crate::radix_sort::indices(&self.entries)
    }
    fn push_record(
        &mut self,
        header: &sam::Header,
        key: SortKey,
        record: &mut Record,
        estimate: u64,
    ) {
        self.encoder.get_mut().clear();
        let stored = if estimate <= FRAME_LIMIT as u64
            && self
                .encoder
                .write_alignment_record(header, &EncoderRecord(record))
                .is_ok()
            && !self.encoder.get_ref().oversized
        {
            let start = self.arena.len();
            self.arena.extend_from_slice(&self.encoder.get_ref().frame);
            Stored::Frame {
                start,
                end: self.arena.len(),
            }
        } else {
            // The private encoder may have allocated a large record before the
            // capped sink saw its length, or partially encoded an invalid record.
            // Release it, and let the original writer report errors in sort order.
            self.encoder = bam::io::Writer::from(FrameSink::default());
            Stored::Original(Box::new(std::mem::take(record)))
        };
        self.entries.push((key, stored));
    }
    fn emit<W: Write>(
        &self,
        position: usize,
        header: &sam::Header,
        writer: &mut BamWriter<W>,
    ) -> Result<usize> {
        let (key, stored) = &self.entries[position];
        match stored {
            Stored::Frame { start, end } => {
                writer.write_encoded_record(&self.arena[*start..*end])?
            }
            Stored::Original(record) => writer.write_record(header, record)?,
        }
        Ok(key.input_index)
    }
    fn take(&mut self, position: usize, record: &mut Record, frame: &mut Vec<u8>) {
        frame.clear();
        match &mut self.entries[position].1 {
            Stored::Frame { start, end } => frame.extend_from_slice(&self.arena[*start..*end]),
            Stored::Original(original) => *record = std::mem::take(original.as_mut()),
        }
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.arena.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::sam::alignment::record::{
        cigar::{op::Kind, Op},
        data::field::Tag,
    };
    use noodles::sam::alignment::record_buf::{data::field::Value, Cigar, Sequence};

    fn key(record: &Record, index: usize) -> SortKey {
        SortKey::new(
            record.flags(),
            None,
            None,
            record.name().map(|n| n.as_ref()).unwrap_or(&[]),
            index,
        )
    }

    #[test]
    fn compact_frames_fallbacks_and_deferred_errors_match_owned_emission() {
        let header = sam::Header::default();
        let small = Record::builder()
            .set_name("small")
            .set_sequence(Sequence::from(b"ACGT".to_vec()))
            .set_quality_scores(vec![20; 4].into())
            .build();
        let large = Record::builder()
            .set_name("large")
            .set_sequence(Sequence::from(vec![b'A'; 70000]))
            .build();
        let invalid = Record::builder().set_name(vec![b'x'; 256]).build();
        let mut tags = small.clone();
        tags.data_mut()
            .insert(Tag::COMMENT, Value::String(vec![b'z'; 70000].into()));
        let invalid_cigar = Record::builder()
            .set_cigar(Cigar::from(vec![Op::new(Kind::Match, 1 << 28)]))
            .build();
        let records = [small.clone(), large, small, tags, invalid, invalid_cigar];
        let mut compact = Compact::default();
        let mut owned = Vec::new();
        for (i, original) in records.iter().enumerate() {
            let mut record = original.clone();
            compact.push_record(
                &header,
                key(original, i),
                &mut record,
                if i == 1 { 70000 } else { 1024 },
            );
            owned.push((key(original, i), original.clone()));
            assert_eq!(
                matches!(compact.entries[i].1, Stored::Frame { .. }),
                i == 0 || i == 2
            );
        }
        assert_eq!(compact.order(), owned.order());
        let mut a = BamWriter::new(Vec::new());
        let mut b = BamWriter::new(Vec::new());
        a.write_header(&header).unwrap();
        b.write_header(&header).unwrap();
        for i in [0, 1, 2, 3, 4, 2, 5, 0] {
            let left = compact
                .emit(i, &header, &mut a)
                .map_err(|e| (e.exit_code(), e.to_string()));
            let right = owned
                .emit(i, &header, &mut b)
                .map_err(|e| (e.exit_code(), e.to_string()));
            assert_eq!(left, right);
        }
        assert_eq!(a.finish().unwrap(), b.finish().unwrap());
    }

    #[test]
    fn capped_sink_handles_split_prefixes_and_discards_oversized_bodies() {
        for size in [0, 31, FRAME_LIMIT - 4, FRAME_LIMIT - 3, 100000] {
            let mut expected = (size as u32).to_le_bytes().to_vec();
            expected.extend(vec![7; size]);
            for split in [1, 3, 4, 17, usize::MAX] {
                let mut sink = FrameSink::default();
                for bytes in expected.chunks(split) {
                    sink.write_all(bytes).unwrap();
                }
                assert_eq!(sink.oversized, size + 4 > FRAME_LIMIT);
                if sink.oversized {
                    assert!(sink.frame.is_empty());
                } else {
                    assert_eq!(sink.frame, expected);
                }
                sink.clear();
                assert!(!sink.oversized);
                assert!(sink.frame.is_empty());
            }
        }
    }
}
