//! FastQC contaminant matching.
//!
//! The input format is `name<TAB>sequence`, not FASTA. Matching follows the
//! shipped FastQC 0.13.0 `Contaminant` class: both strands, exact containment for
//! 8–19 bp queries, and the longest ungapped match with at most one mismatch.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Strand on which a contaminant matched.
pub enum Direction {
    /// The query matched the contaminant sequence directly.
    Forward,
    /// The query matched the reverse complement.
    Reverse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// The best ungapped match to one contaminant.
pub struct ContaminantHit {
    /// Contaminant name from the input list.
    pub name: String,
    /// Strand of the match.
    pub direction: Direction,
    /// Length of the matching interval in bases.
    pub length: usize,
    /// Integer percent identity used by FastQC's report.
    pub percent_id: i32,
}

impl ContaminantHit {
    /// Render the `Possible Source` cell used by FastQC.
    pub fn render(&self) -> String {
        format!(
            "{} ({}% over {}bp)",
            self.name, self.percent_id, self.length
        )
    }
}

#[derive(Clone, Debug)]
/// A named contaminant with a precomputed reverse complement.
pub struct Contaminant {
    /// Contaminant name from the input list.
    pub name: String,
    forward: Vec<u8>,
    reverse: Vec<u8>,
}

impl Contaminant {
    /// Construct a contaminant, rejecting bases FastQC cannot reverse-complement.
    pub fn new(name: &str, sequence: &str) -> Result<Self, String> {
        let forward = sequence.to_ascii_uppercase().into_bytes();
        let mut reverse = vec![0; forward.len()];
        for (index, base) in forward.iter().enumerate() {
            let reverse_index = forward.len() - 1 - index;
            reverse[reverse_index] = match base {
                b'A' => b'T',
                b'C' => b'G',
                b'G' => b'C',
                b'T' => b'A',
                other => {
                    return Err(format!(
                        "contaminant '{name}' contains illegal base '{}'; expected A/C/G/T",
                        char::from(*other)
                    ))
                }
            };
        }
        Ok(Self {
            name: name.to_string(),
            forward,
            reverse,
        })
    }

    /// Find the best forward or reverse-complement match to a read sequence.
    pub fn find_match(&self, query: &[u8]) -> Option<ContaminantHit> {
        let query = query
            .iter()
            .map(|base| base.to_ascii_uppercase())
            .collect::<Vec<_>>();
        if (8..20).contains(&query.len()) {
            if self
                .forward
                .windows(query.len())
                .any(|window| window == query)
            {
                return Some(self.hit(Direction::Forward, query.len(), 100));
            }
            if self
                .reverse
                .windows(query.len())
                .any(|window| window == query)
            {
                return Some(self.hit(Direction::Reverse, query.len(), 100));
            }
        }

        let mut best = None;
        let min_offset = -(self.forward.len() as i64 - 20);
        let max_offset = query.len() as i64 - 20;
        for offset in min_offset..max_offset {
            if let Some(hit) =
                self.find_match_inner(&self.forward, &query, offset, Direction::Forward)
            {
                replace_longer(&mut best, hit);
            }
        }
        for offset in min_offset..max_offset {
            if let Some(hit) =
                self.find_match_inner(&self.reverse, &query, offset, Direction::Reverse)
            {
                replace_longer(&mut best, hit);
            }
        }
        best
    }

    fn hit(&self, direction: Direction, length: usize, percent_id: i32) -> ContaminantHit {
        ContaminantHit {
            name: self.name.clone(),
            direction,
            length,
            percent_id,
        }
    }

    fn find_match_inner(
        &self,
        contaminant: &[u8],
        query: &[u8],
        offset: i64,
        direction: Direction,
    ) -> Option<ContaminantHit> {
        let mut best = None;
        let mut mismatches = 0i64;
        let mut start = 0i64;
        let mut end = 0i64;

        for (index, base) in contaminant.iter().enumerate() {
            let index = index as i64;
            let query_index = index + offset;
            if query_index < 0 {
                start = index + 1;
                continue;
            }
            if query_index >= query.len() as i64 {
                break;
            }
            if *base == query[query_index as usize] {
                end = index;
            } else {
                mismatches += 1;
                if mismatches > 1 {
                    let length = 1 + end - start;
                    if length > 20 {
                        let percent = ((length - (mismatches - 1)) * 100 / length) as i32;
                        let candidate = self.hit(direction, length as usize, percent);
                        replace_longer(&mut best, candidate);
                    }
                    start = index + 1;
                    end = index + 1;
                    mismatches = 0;
                }
            }
        }

        let length = 1 + end - start;
        if length > 20 {
            let percent = ((length - mismatches) * 100 / length) as i32;
            let candidate = self.hit(direction, length as usize, percent);
            replace_longer(&mut best, candidate);
        }
        best
    }
}

fn replace_longer(best: &mut Option<ContaminantHit>, candidate: ContaminantHit) {
    let replace = match best {
        None => true,
        Some(current) => {
            candidate.length > current.length
                || (candidate.length == current.length && candidate.percent_id > current.percent_id)
        }
    };
    if replace {
        *best = Some(candidate);
    }
}

/// Return the longest match across a contaminant list, breaking ties by percent identity.
pub fn find_best_hit(contaminants: &[Contaminant], query: &[u8]) -> Option<ContaminantHit> {
    let mut best = None;
    for contaminant in contaminants {
        if let Some(hit) = contaminant.find_match(query) {
            replace_longer(&mut best, hit);
        }
    }
    best
}

/// Parse FastQC's `name<TAB>sequence` contaminant-list format.
pub fn parse(text: &str) -> Result<Vec<Contaminant>, String> {
    let mut contaminants = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, sequence) = line
            .split_once('\t')
            .ok_or_else(|| format!("line {}: expected name<TAB>sequence", line_number + 1))?;
        let name = name.trim();
        let sequence = sequence.trim();
        if name.is_empty() || sequence.is_empty() {
            return Err(format!(
                "line {}: contaminant name and sequence must not be empty",
                line_number + 1
            ));
        }
        contaminants.push(
            Contaminant::new(name, sequence)
                .map_err(|error| format!("line {}: {error}", line_number + 1))?,
        );
    }
    Ok(contaminants)
}

/// The contaminant list shipped by FastQC 0.13.0.
pub const DEFAULT_CONTAMINANTS: &str = include_str!("contaminant_list.txt");

/// Parse the built-in FastQC 0.13.0 contaminant list.
pub fn default_contaminants() -> Result<Vec<Contaminant>, String> {
    parse(DEFAULT_CONTAMINANTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence() -> String {
        "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTAC".to_string()
    }

    #[test]
    fn exact_longer_and_one_mismatch_matches_follow_fastqc() {
        let query = sequence();
        let exact = Contaminant::new("exact", &query).unwrap();
        assert_eq!(
            exact.find_match(query.as_bytes()).unwrap().render(),
            "exact (100% over 50bp)"
        );
        let longer = Contaminant::new("longer", &(query.clone() + "TTTT")).unwrap();
        assert_eq!(
            longer.find_match(query.as_bytes()).unwrap().render(),
            "longer (100% over 50bp)"
        );
        let mismatch =
            Contaminant::new("mismatch", &(query[..query.len() - 1].to_string() + "A")).unwrap();
        assert_eq!(
            mismatch.find_match(query.as_bytes()).unwrap().render(),
            "mismatch (97% over 49bp)"
        );
    }

    #[test]
    fn short_queries_match_both_strands_but_short_contaminants_do_not_invent_a_hit() {
        let query = sequence();
        let short = &query[..12];
        let contaminant = Contaminant::new("long", &query).unwrap();
        assert_eq!(
            contaminant.find_match(short.as_bytes()).unwrap().percent_id,
            100
        );
        let reverse = crate::contaminants::Contaminant::new("reverse", &query).unwrap();
        let reverse_query = reverse.reverse.clone();
        assert_eq!(
            reverse.find_match(&reverse_query).unwrap().direction,
            Direction::Reverse
        );
        let prefix = Contaminant::new("prefix", short).unwrap();
        assert!(prefix.find_match(query.as_bytes()).is_none());
    }

    #[test]
    fn parser_requires_tab_separated_names_and_acgt() {
        let parsed = parse("name\tACGT\n# comment\tAAAA\n").unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(parse("not-fast-a-record").is_err());
        assert!(parse("bad\tACGN").is_err());
    }
}
