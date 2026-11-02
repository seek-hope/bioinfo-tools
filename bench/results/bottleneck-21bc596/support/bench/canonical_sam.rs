//! Streaming SAM canonicalization for benchmark validation only.
//! Mirrors readme_benchmark.bam_summary's @PG removal, coordinate check and
//! lexicographic field-tuple ordering within coordinate ties. Memory is O(the
//! largest coordinate tie group), not constant. No alignment fields are dropped.
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn number(bytes: &[u8]) -> io::Result<usize> {
    if bytes.is_empty() {
        return Err(invalid("empty coordinate"));
    }
    bytes.iter().try_fold(0usize, |n, b| {
        if !b.is_ascii_digit() {
            return Err(invalid("invalid coordinate"));
        }
        n.checked_mul(10)
            .and_then(|n| n.checked_add((b - b'0') as usize))
            .ok_or_else(|| invalid("coordinate overflow"))
    })
}

fn emit<W: Write>(group: &mut Vec<Vec<u8>>, out: &mut W) -> io::Result<()> {
    group.sort_unstable_by(|a, b| a.split(|c| *c == b'\t').cmp(b.split(|c| *c == b'\t')));
    for record in group.drain(..) {
        out.write_all(&record)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn canonicalize<R: BufRead, W: Write>(mut input: R, mut out: W) -> io::Result<u64> {
    let mut references = HashMap::<Vec<u8>, usize>::new();
    let mut previous = None;
    let mut group = Vec::new();
    let mut line = Vec::new();
    let mut count = 0u64;
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.starts_with(b"@") {
            if count != 0 {
                return Err(invalid("header after records"));
            }
            if line.starts_with(b"@PG\t") {
                continue;
            }
            if line.starts_with(b"@SQ\t") {
                let name = line
                    .strip_suffix(b"\n")
                    .unwrap_or(&line)
                    .split(|c| *c == b'\t')
                    .find_map(|field| field.strip_prefix(b"SN:"))
                    .ok_or_else(|| invalid("reference header without SN"))?;
                let index = references.len();
                if references.insert(name.to_vec(), index).is_some() {
                    return Err(invalid("duplicate reference name"));
                }
            }
            out.write_all(&line)?;
            continue;
        }
        while line.last() == Some(&b'\n') {
            line.pop();
        }
        let mut fields = line.split(|c| *c == b'\t');
        let _name = fields.next().ok_or_else(|| invalid("missing name"))?;
        let _flags = fields.next().ok_or_else(|| invalid("missing flags"))?;
        let reference = fields.next().ok_or_else(|| invalid("missing reference"))?;
        let position = fields.next().ok_or_else(|| invalid("missing position"))?;
        if fields.count() < 7 {
            return Err(invalid("fewer than 11 SAM fields"));
        }
        let rank = if reference == b"*" {
            references.len()
        } else {
            *references
                .get(reference)
                .ok_or_else(|| invalid("unknown reference"))?
        };
        let key = (rank, number(position)?);
        if previous.is_some_and(|p| key < p) {
            return Err(invalid("coordinates out of order"));
        }
        if previous.is_some_and(|p| key != p) {
            emit(&mut group, &mut out)?;
        }
        previous = Some(key);
        group.push(std::mem::take(&mut line));
        count = count
            .checked_add(1)
            .ok_or_else(|| invalid("record count overflow"))?;
    }
    emit(&mut group, &mut out)?;
    out.flush()?;
    Ok(count)
}

fn main() -> io::Result<()> {
    let count = canonicalize(io::stdin().lock(), io::BufWriter::new(io::stdout().lock()))?;
    eprintln!("records={count}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_fields_are_retained_and_ties_are_sorted() {
        let input = b"@HD\tVN:1.6\n@SQ\tSN:c\tLN:9\n@PG\tID:x\n\
            b\t0\tc\t1\t60\t1M\t*\t0\t0\tA\tI\tXX:Z:tail\n\
            a\t0\tc\t1\t60\t1M\t*\t0\t0\tC\tI\n";
        let mut out = Vec::new();
        assert_eq!(canonicalize(&input[..], &mut out).unwrap(), 2);
        assert_eq!(
            out,
            b"@HD\tVN:1.6\n@SQ\tSN:c\tLN:9\n\
            a\t0\tc\t1\t60\t1M\t*\t0\t0\tC\tI\n\
            b\t0\tc\t1\t60\t1M\t*\t0\t0\tA\tI\tXX:Z:tail\n"
        );
    }
    #[test]
    fn rejects_out_of_order_and_missing_fields() {
        for input in [
            b"@SQ\tSN:c\tLN:9\nr\t0\tc\t2\t60\t1M\t*\t0\t0\tA\tI\nr\t0\tc\t1\t60\t1M\t*\t0\t0\tA\tI\n".as_slice(),
            b"r\t0\t*\t0\n",
        ] {
            assert!(canonicalize(input, Vec::new()).is_err());
        }
    }
}
