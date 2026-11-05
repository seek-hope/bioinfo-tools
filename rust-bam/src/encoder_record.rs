//! Borrowed RecordBuf adapter for the validated slice path in the BAM encoder.

use std::io;

use bstr::BStr;
use noodles::{
    core::Position,
    sam::{
        self,
        alignment::{
            record::{
                Cigar, CigarRef, Data, DataRef, Flags, MappingQuality, QualityScores,
                QualityScoresRef, Sequence, SequenceRef,
            },
            Record, RecordBuf,
        },
    },
};

pub(crate) struct EncoderRecord<'a>(pub(crate) &'a RecordBuf);

impl Record for EncoderRecord<'_> {
    fn name(&self) -> Option<&BStr> {
        Record::name(self.0)
    }

    fn flags(&self) -> io::Result<Flags> {
        Record::flags(self.0)
    }

    fn reference_sequence_id<'r, 'h: 'r>(
        &'r self,
        header: &'h sam::Header,
    ) -> Option<io::Result<usize>> {
        Record::reference_sequence_id(self.0, header)
    }

    fn alignment_start(&self) -> Option<io::Result<Position>> {
        Record::alignment_start(self.0)
    }

    fn mapping_quality(&self) -> Option<io::Result<MappingQuality>> {
        Record::mapping_quality(self.0)
    }

    fn cigar(&self) -> Box<dyn Cigar + '_> {
        Record::cigar(self.0)
    }

    fn mate_reference_sequence_id<'r, 'h: 'r>(
        &'r self,
        header: &'h sam::Header,
    ) -> Option<io::Result<usize>> {
        Record::mate_reference_sequence_id(self.0, header)
    }

    fn mate_alignment_start(&self) -> Option<io::Result<Position>> {
        Record::mate_alignment_start(self.0)
    }

    fn template_length(&self) -> io::Result<i32> {
        Record::template_length(self.0)
    }

    fn sequence(&self) -> Box<dyn Sequence + '_> {
        Record::sequence(self.0)
    }

    fn quality_scores(&self) -> Box<dyn QualityScores + '_> {
        Record::quality_scores(self.0)
    }

    fn data(&self) -> Box<dyn Data<'_> + '_> {
        Record::data(self.0)
    }

    fn cigar_ref(&self) -> CigarRef<'_> {
        Record::cigar_ref(self.0)
    }

    fn sequence_ref(&self) -> SequenceRef<'_> {
        Record::sequence_ref(self.0)
    }

    fn quality_scores_ref(&self) -> QualityScoresRef<'_> {
        QualityScoresRef::Raw(self.0.quality_scores().as_ref())
    }

    fn data_ref(&self) -> DataRef<'_> {
        Record::data_ref(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::{bam, sam::alignment::io::Write as _};

    fn check(record: RecordBuf) {
        let header = sam::Header::default();
        let mut original = bam::io::Writer::from(Vec::new());
        let mut adapted = bam::io::Writer::from(Vec::new());
        original.write_header(&header).unwrap();
        adapted.write_header(&header).unwrap();
        // A later valid record must not expose the differing private-buffer
        // contents left by a failing quality check.
        for current in [&record, &RecordBuf::default()] {
            let expected = original
                .write_alignment_record(&header, current)
                .map_err(|error| (error.kind(), error.to_string()));
            let actual = adapted
                .write_alignment_record(&header, &EncoderRecord(current))
                .map_err(|error| (error.kind(), error.to_string()));
            assert_eq!(actual, expected);
            assert_eq!(adapted.get_ref(), original.get_ref());
        }
    }

    fn record(bases: usize, scores: Vec<u8>) -> RecordBuf {
        RecordBuf::builder()
            .set_name(b"read".to_vec())
            .set_sequence(vec![b'A'; bases].into())
            .set_quality_scores(scores.into())
            .build()
    }

    #[test]
    fn quality_slices_match_generic_encoder_for_all_bytes_and_lengths() {
        for score in 0..=u8::MAX {
            check(record(3, vec![score; 3]));
            check(record(3, vec![0, score, 93]));
            check(record(2, vec![score; 3]));
            check(record(4, vec![score; 3]));
        }
        for length in [0, 1, 150, 65535, 65536, 70001] {
            check(record(length, Vec::new()));
            check(record(
                length,
                (0..length).map(|i| (i % 94) as u8).collect(),
            ));
        }
    }

    #[test]
    fn earlier_field_errors_still_precede_quality_errors() {
        use sam::alignment::record::cigar::{op::Kind, Op};

        let mut bad_name = record(3, vec![94; 3]);
        *bad_name.name_mut() = Some(vec![b'x'; 256].into());
        check(bad_name);

        let mut bad_reference = record(3, vec![255; 3]);
        *bad_reference.reference_sequence_id_mut() = Some(0);
        check(bad_reference);

        let mut bad_mate = record(3, vec![94; 3]);
        *bad_mate.mate_reference_sequence_id_mut() = Some(0);
        check(bad_mate);

        let mut bad_cigar = record(3, vec![94; 3]);
        *bad_cigar.cigar_mut() = [Op::new(Kind::Match, 1 << 28)].into_iter().collect();
        check(bad_cigar);

        let mut mismatched_cigar = record(3, vec![94; 3]);
        *mismatched_cigar.cigar_mut() = [Op::new(Kind::Match, 2)].into_iter().collect();
        check(mismatched_cigar);
    }
}
