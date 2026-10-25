//! Alignment helpers: position semantics, CIGAR walking and tag access.
//!
//! These mirror `umi_tools.sam_methods.get_read_position` / `find_splice` and the
//! `pysam` accessors used by `sam_methods.update_dicts`.
//!
//! Both hot functions iterate the CIGAR directly: the per-read allocations of an
//! earlier version showed up as a significant part of the parse cost.

use crate::error::{Error, Result};
use crate::io::bam::Record;
use crate::io::bam::{Aux, Cigar};

/// Port of `find_splice`. Returns 0 when there is no splice, matching the
/// upstream `False` because the value only ever lands in a bundle key.
fn find_splice<I>(ops: I) -> i64
where
    I: Iterator<Item = Cigar>,
{
    let mut iter = ops.peekable();
    let mut offset: i64 = 0;
    // A soft clip at the start is not taken as the splice itself.
    if let Some(Cigar::SoftClip(length)) = iter.peek() {
        offset = i64::from(*length);
        iter.next();
    }
    for op in iter {
        match op {
            Cigar::RefSkip(_) | Cigar::SoftClip(_) => return offset,
            Cigar::Match(length)
            | Cigar::Del(length)
            | Cigar::Equal(length)
            | Cigar::Diff(length) => offset += i64::from(length),
            Cigar::Ins(_) | Cigar::HardClip(_) | Cigar::Pad(_) => {}
        }
    }
    0
}

/// `(start, pos)` for a record plus the spliced signature used in the bundle key.
///
/// `start` is UMI-tools' `self.start` (buffer advancement), `pos` is the bundle
/// position key. Forward reads use `POS - leading_softclip`, reverse reads use
/// `aend + trailing_softclip`.
pub fn read_position(record: &Record, soft_clip_threshold: i64, spliced: bool) -> (i64, i64, i64) {
    let cigar = record.cigar();
    let mut reference_length: i64 = 0;
    let mut has_skip = false;
    for op in cigar.iter() {
        match op {
            Cigar::Match(length)
            | Cigar::Del(length)
            | Cigar::RefSkip(length)
            | Cigar::Equal(length)
            | Cigar::Diff(length) => reference_length += i64::from(length),
            Cigar::Ins(_) | Cigar::SoftClip(_) | Cigar::HardClip(_) | Cigar::Pad(_) => {}
        }
        if matches!(op, Cigar::RefSkip(_)) {
            has_skip = true;
        }
    }

    let first = cigar.iter().next();
    let last = cigar.iter().last();

    if record.is_reverse() {
        let mut pos = record.pos() + reference_length;
        if let Some(Cigar::SoftClip(length)) = last {
            pos += i64::from(length);
        }
        let trigger = matches!(first, Some(Cigar::SoftClip(length)) if i64::from(length) > soft_clip_threshold);
        let spliced_value = if spliced && (has_skip || trigger) {
            find_splice(cigar.iter().rev())
        } else {
            0
        };
        (record.pos(), pos, spliced_value)
    } else {
        let mut pos = record.pos();
        if let Some(Cigar::SoftClip(length)) = first {
            pos -= i64::from(length);
        }
        let trigger = matches!(last, Some(Cigar::SoftClip(length)) if i64::from(length) > soft_clip_threshold);
        let spliced_value = if spliced && (has_skip || trigger) {
            find_splice(cigar.iter())
        } else {
            0
        };
        (pos, pos, spliced_value)
    }
}

fn tag_error(tag: &[u8; 2]) -> Error {
    Error::Input(crate::error::InputError::MissingUmi {
        detail: format!("read is missing the '{}' tag", String::from_utf8_lossy(tag)),
    })
}

/// Integer tag lookup, mirroring `read.opt(tag)` for numeric tags.
pub fn aux_int(record: &Record, tag: &[u8; 2]) -> Result<i64> {
    match record.aux(tag) {
        Ok(Aux::I8(v)) => Ok(i64::from(v)),
        Ok(Aux::U8(v)) => Ok(i64::from(v)),
        Ok(Aux::I16(v)) => Ok(i64::from(v)),
        Ok(Aux::U16(v)) => Ok(i64::from(v)),
        Ok(Aux::I32(v)) => Ok(i64::from(v)),
        Ok(Aux::U32(v)) => Ok(i64::from(v)),
        Ok(Aux::Float(v)) => Ok(f64::from(v) as i64),
        Ok(Aux::Double(v)) => Ok(v as i64),
        Ok(_) => Err(Error::Format(format!(
            "tag '{}' is not numeric",
            String::from_utf8_lossy(tag)
        ))),
        Err(_) => Err(tag_error(tag)),
    }
}

/// String/char tag lookup, mirroring `read.opt(tag)` for the XT tag.
pub fn aux_str(record: &Record, tag: &[u8; 2]) -> Result<String> {
    match record.aux(tag) {
        Ok(Aux::String(v)) => Ok(v.to_string()),
        Ok(Aux::Char(v)) => Ok(v.to_string()),
        Ok(_) => Err(Error::Format(format!(
            "tag '{}' is not a string",
            String::from_utf8_lossy(tag)
        ))),
        Err(_) => Err(tag_error(tag)),
    }
}
