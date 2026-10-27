//! SeqKit `translate`: codon expansion, frames and ORF output modes.

use crate::io::{CommandError, InputFormat, Output, OwnedRecord, RecordReader};
use crate::transform;
use bioinfo_fastx::writer::Layout;
use std::collections::BTreeSet;
use std::io::Write;

/// The parsed `translate` options.
pub struct TranslateOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-f`: frames; 6 means all six frames.
    pub frames: Vec<i32>,
    /// `-T`: translation table ID (1-6, 9-14, 16, 21-31).
    pub table: i32,
    /// `-l`: list a table, or all table names when zero.
    pub list_table: i32,
    /// `-L`: list a table including unambiguous IUPAC codons.
    pub list_ambiguous: i32,
    /// `-x`: translate invalid codons to X.
    pub allow_unknown: bool,
    /// `-M`: translate the initial codon as M.
    pub init_m: bool,
    /// `-F`: append frame to the ID.
    pub append_frame: bool,
    /// `-s`: output individual ORFs separated by STOP codons.
    pub out_subseqs: bool,
    /// `--clean`: replace STOP with X.
    pub clean: bool,
    /// `-m`: minimum amino-acid length for `-s` ORFs.
    pub min_len: usize,
    /// `-e`: output a blank sequence on translation errors.
    pub skip_errors: bool,
    /// `--trim`: remove trailing X and STOP characters.
    pub trim: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

struct TableInfo {
    id: i32,
    name: &'static str,
    start: &'static str,
}

// NCBI genetic-code names and initiation codons, in SeqKit's listing order.
// Codon mappings themselves remain in table_codon below and are shared with
// normal translation, so a listing cannot drift to a second table definition.
const TABLE_INFO: &[TableInfo] = &[
    TableInfo { id: 1, name: "The Standard Code", start: "ATG, CTG, TTG" },
    TableInfo { id: 2, name: "The Vertebrate Mitochondrial Code", start: "ATA, ATC, ATG, ATT, GTG" },
    TableInfo { id: 3, name: "The Yeast Mitochondrial Code", start: "ATA, ATG" },
    TableInfo { id: 4, name: "The Mold, Protozoan, and Coelenterate Mitochondrial Code and the Mycoplasma/Spiroplasma Code", start: "ATA, ATC, ATG, ATT, CTG, GTG, TTA, TTG" },
    TableInfo { id: 5, name: "The Invertebrate Mitochondrial Code ", start: "ATA, ATC, ATG, ATT, GTG, TTG" },
    TableInfo { id: 6, name: "The Ciliate, Dasycladacean and Hexamita Nuclear Code", start: "ATG" },
    TableInfo { id: 9, name: "The Echinoderm and Flatworm Mitochondrial Code", start: "ATG, GTG" },
    TableInfo { id: 10, name: "The Euplotid Nuclear Code", start: "ATG" },
    TableInfo { id: 11, name: "The Bacterial, Archaeal and Plant Plastid Code", start: "ATA, ATC, ATG, ATT, CTG, GTG, TTG" },
    TableInfo { id: 12, name: "The Alternative Yeast Nuclear Code", start: "ATG, CTG" },
    TableInfo { id: 13, name: "The Ascidian Mitochondrial Code", start: "ATA, ATG, GTG, TTG" },
    TableInfo { id: 14, name: "The Alternative Flatworm Mitochondrial Code", start: "ATG" },
    TableInfo { id: 16, name: "Chlorophycean Mitochondrial Code", start: "ATG" },
    TableInfo { id: 21, name: "Trematode Mitochondrial Code", start: "ATG, GTG" },
    TableInfo { id: 22, name: "Scenedesmus obliquus Mitochondrial Code", start: "ATG" },
    TableInfo { id: 23, name: "Thraustochytrium Mitochondrial Code", start: "ATG, ATT, GTG" },
    TableInfo { id: 24, name: "Pterobranchia Mitochondrial Code", start: "ATG, CTG, GTG, TTG" },
    TableInfo { id: 25, name: "Candidate Division SR1 and Gracilibacteria Code", start: "ATG, GTG, TTG" },
    TableInfo { id: 26, name: "Pachysolen tannophilus Nuclear Code", start: "ATG, CTG" },
    TableInfo { id: 27, name: "Karyorelict Nuclear", start: "ATG" },
    TableInfo { id: 28, name: "Condylostoma Nuclear", start: "ATG" },
    TableInfo { id: 29, name: "Mesodinium Nuclear", start: "ATG" },
    TableInfo { id: 30, name: "Peritrich Nuclear", start: "ATG" },
    TableInfo { id: 31, name: "Blastocrithidia Nuclear", start: "ATG" },
];

const DNA_ORDER: &[u8] = b"ACGT";
// SeqKit's IUPAC listing order, including the placement of M before G and
// ambiguous-only groups such as MGA/TRA/YTA.
const AMBIGUOUS_ORDER: &[u8] = b"ACMGRSVTWYHKDBN";

fn table_info(id: i32) -> Option<&'static TableInfo> {
    TABLE_INFO.iter().find(|info| info.id == id)
}

fn stop_codons(id: i32) -> &'static str {
    // These are the heading values printed by SeqKit. They are intentionally
    // separate from table_codon: several special tables reassign a codon in
    // the mapping while retaining the NCBI stop-codon heading.
    match id {
        1 => "TAA, TAG, TGA",
        2 => "AGA, AGG, TAA, TAG",
        3 | 4 | 5 | 9 | 10 | 13 | 21 | 24 | 25 => "TAA, TAG",
        6 => "TGA",
        11 | 12 | 26 | 28 => "TAA, TAG, TGA",
        14 => "TAG",
        16 => "TAA, TGA",
        22 => "TAA, TCA, TGA",
        23 => "TAA, TAG, TGA, TTA",
        27 | 29 | 30 => "TGA",
        31 => "TAA, TAG",
        _ => "",
    }
}

fn write_listing(options: &TranslateOptions, out: &mut Output) -> Result<bool, CommandError> {
    let directory = options.list_table == 0 || options.list_ambiguous == 0;
    if directory {
        let mut text = String::new();
        for info in TABLE_INFO {
            text.push_str(&format!("{}\t{}\n", info.id, info.name));
        }
        out.write_all(text.as_bytes())?;
        return Ok(true);
    }
    let (id, ambiguous) = if options.list_ambiguous > 0 {
        (options.list_ambiguous, true)
    } else if options.list_table > 0 {
        (options.list_table, false)
    } else {
        return Ok(false);
    };
    let Some(info) = table_info(id) else {
        // SeqKit treats an unknown positive listing ID as an empty successful
        // listing rather than as a translation-table error.
        return Ok(true);
    };
    let mut text = format!(
        "{} (transl_table={})\nSource: https://www.ncbi.nlm.nih.gov/Taxonomy/taxonomyhome.html/index.cgi?chapter=tgencodes#SG{}\n\nInitiation Codons:\n  {}\n\nStop Codons:\n  {}\n\nStranslate Table:\n",
        info.name,
        info.id,
        info.id,
        info.start,
        stop_codons(id),
    );
    let alphabet = if ambiguous {
        AMBIGUOUS_ORDER
    } else {
        DNA_ORDER
    };
    for &first in alphabet {
        let mut first_group = false;
        for &second in alphabet {
            let mut entries = Vec::new();
            for &third in alphabet {
                let codon_bytes = [first, second, third];
                let amino = if ambiguous {
                    codon(id, &codon_bytes).filter(|amino| *amino != 'X')
                } else {
                    table_codon(id, &codon_bytes)
                };
                if let Some(amino) = amino {
                    entries.push(format!(
                        "{}: {}",
                        String::from_utf8_lossy(&codon_bytes),
                        amino
                    ));
                }
            }
            if !entries.is_empty() {
                text.push_str(&format!("  {}\n", entries.join(", ")));
                first_group = true;
            }
        }
        if first_group {
            text.push('\n');
        }
    }
    out.write_all(text.as_bytes())?;
    Ok(true)
}

fn standard_codon(codon: &[u8]) -> Option<char> {
    let amino = match codon {
        b"TTT" | b"TTC" => 'F',
        b"TTA" | b"TTG" | b"CTT" | b"CTC" | b"CTA" | b"CTG" => 'L',
        b"ATT" | b"ATC" | b"ATA" => 'I',
        b"ATG" => 'M',
        b"GTT" | b"GTC" | b"GTA" | b"GTG" => 'V',
        b"TCT" | b"TCC" | b"TCA" | b"TCG" | b"AGT" | b"AGC" => 'S',
        b"CCT" | b"CCC" | b"CCA" | b"CCG" => 'P',
        b"ACT" | b"ACC" | b"ACA" | b"ACG" => 'T',
        b"GCT" | b"GCC" | b"GCA" | b"GCG" => 'A',
        b"TAT" | b"TAC" => 'Y',
        b"TAA" | b"TAG" | b"TGA" => '*',
        b"CAT" | b"CAC" => 'H',
        b"CAA" | b"CAG" => 'Q',
        b"AAT" | b"AAC" => 'N',
        b"AAA" | b"AAG" => 'K',
        b"GAT" | b"GAC" => 'D',
        b"GAA" | b"GAG" => 'E',
        b"TGT" | b"TGC" => 'C',
        b"TGG" => 'W',
        b"CGT" | b"CGC" | b"CGA" | b"CGG" | b"AGA" | b"AGG" => 'R',
        b"GGT" | b"GGC" | b"GGA" | b"GGG" => 'G',
        _ => return None,
    };
    Some(amino)
}

fn table_codon(table: i32, codon: &[u8]) -> Option<char> {
    match table {
        2 => match codon {
            b"AGA" | b"AGG" => Some('*'),
            b"ATA" => Some('M'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        3 => match codon {
            b"ATA" => Some('M'),
            b"CTA" => Some('T'),
            b"CTC" => Some('T'),
            b"CTG" => Some('T'),
            b"CTT" => Some('T'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        4 => match codon {
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        5 => match codon {
            b"AGA" => Some('S'),
            b"AGG" => Some('S'),
            b"ATA" => Some('M'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        6 => match codon {
            b"TAA" => Some('Q'),
            b"TAG" => Some('Q'),
            _ => standard_codon(codon),
        },
        9 => match codon {
            b"AAA" => Some('N'),
            b"AGA" => Some('S'),
            b"AGG" => Some('S'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        10 => match codon {
            b"TGA" => Some('C'),
            _ => standard_codon(codon),
        },
        12 => match codon {
            b"CTG" => Some('S'),
            _ => standard_codon(codon),
        },
        13 => match codon {
            b"AGA" => Some('G'),
            b"AGG" => Some('G'),
            b"ATA" => Some('M'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        14 => match codon {
            b"AAA" => Some('N'),
            b"AGA" => Some('S'),
            b"AGG" => Some('S'),
            b"TAA" => Some('Y'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        16 => match codon {
            b"TAG" => Some('L'),
            _ => standard_codon(codon),
        },
        21 => match codon {
            b"AAA" => Some('N'),
            b"AGA" => Some('S'),
            b"AGG" => Some('S'),
            b"ATA" => Some('M'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        22 => match codon {
            b"TAG" => Some('L'),
            b"TCA" => Some('*'),
            _ => standard_codon(codon),
        },
        23 => match codon {
            b"TTA" => Some('*'),
            _ => standard_codon(codon),
        },
        24 => match codon {
            b"AGA" => Some('S'),
            b"AGG" => Some('K'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        25 => match codon {
            b"TGA" => Some('G'),
            _ => standard_codon(codon),
        },
        26 => match codon {
            b"CTG" => Some('A'),
            _ => standard_codon(codon),
        },
        27 => match codon {
            b"TAA" => Some('Q'),
            b"TAG" => Some('Q'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        28 => match codon {
            b"TAA" => Some('Q'),
            b"TAG" => Some('Q'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        29 => match codon {
            b"TAA" => Some('Y'),
            b"TAG" => Some('Y'),
            _ => standard_codon(codon),
        },
        30 => match codon {
            b"TAA" => Some('E'),
            b"TAG" => Some('E'),
            _ => standard_codon(codon),
        },
        31 => match codon {
            b"TAA" => Some('E'),
            b"TAG" => Some('E'),
            b"TGA" => Some('W'),
            _ => standard_codon(codon),
        },
        _ => standard_codon(codon),
    }
}

fn bases(code: u8) -> &'static [u8] {
    match code {
        b'A' => b"A",
        b'C' => b"C",
        b'G' => b"G",
        b'T' | b'U' => b"T",
        b'R' => b"AG",
        b'Y' => b"CT",
        b'S' => b"GC",
        b'W' => b"AT",
        b'K' => b"GT",
        b'M' => b"AC",
        b'B' => b"CGT",
        b'D' => b"AGT",
        b'H' => b"ACT",
        b'V' => b"ACG",
        b'N' | b'X' => b"ACGT",
        _ => b"",
    }
}

fn codon(table: i32, codon: &[u8]) -> Option<char> {
    let mut candidates = BTreeSet::new();
    for first in bases(codon[0].to_ascii_uppercase()) {
        for second in bases(codon[1].to_ascii_uppercase()) {
            for third in bases(codon[2].to_ascii_uppercase()) {
                if let Some(amino) = table_codon(table, &[*first, *second, *third]) {
                    candidates.insert(amino);
                }
            }
        }
    }
    match candidates.len() {
        0 => None,
        1 => candidates.into_iter().next(),
        _ => Some('X'),
    }
}

fn translate_sequence(
    sequence: &[u8],
    frame: i32,
    options: &TranslateOptions,
) -> Result<(String, bool), CommandError> {
    let (source, offset) = if frame > 0 {
        (sequence.to_vec(), (frame - 1) as usize)
    } else {
        (
            transform::reverse_complement(sequence),
            (-frame - 1) as usize,
        )
    };
    if offset >= source.len() {
        return Ok((String::new(), false));
    }
    let mut protein = String::new();
    let mut first = true;
    for chunk in source[offset..].as_chunks::<3>().0 {
        let amino = match codon(options.table, chunk) {
            Some(amino) => amino,
            None if options.allow_unknown => 'X',
            None if options.skip_errors => return Ok((String::new(), true)),
            None => {
                return Err(CommandError::input(
                    "unknown codon; use -x/--allow-unknown-codon",
                ))
            }
        };
        let amino = if first && options.init_m { 'M' } else { amino };
        first = false;
        protein.push(amino);
    }
    if options.clean {
        protein = protein.replace('*', "X");
    }
    if options.trim {
        while protein.ends_with('*') || protein.ends_with('X') {
            protein.pop();
        }
    }
    Ok((protein, false))
}

fn write_orfs(
    record: &OwnedRecord,
    protein: &str,
    frame: i32,
    options: &TranslateOptions,
    out: &mut Output,
) -> Result<(), CommandError> {
    let mut translated = record.clone();
    let mut segment_start = 0usize;
    let codons: Vec<char> = protein.chars().collect();
    for index in 0..=codons.len() {
        let stop = index == codons.len() || codons[index] == '*';
        if !stop {
            continue;
        }
        let segment = &codons[segment_start..index];
        if !segment.is_empty() && segment.len() >= options.min_len {
            let begin = segment_start * 3 + 1;
            let end = index * 3;
            let name = format!(
                "{}_frame={}_begin={}_end={} ",
                String::from_utf8_lossy(&record.name),
                frame,
                begin,
                end
            );
            translated.name = name.clone().into_bytes();
            let marker = record.raw_header.first().copied().unwrap_or(b'>');
            translated.raw_header = format!("{}{}", char::from(marker), name).into_bytes();
            translated.sequence = segment.iter().collect::<String>().into_bytes();
            translated.qualities.clear();
            translated.plus.clear();
            translated.has_quality_line = false;
            translated.write_to(out, Layout::Fasta, options.line_width)?;
        }
        segment_start = index + 1;
    }
    Ok(())
}

/// Run `translate`.
pub fn run(options: TranslateOptions) -> Result<(), CommandError> {
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    if write_listing(&options, &mut out)? {
        return out.finish();
    }
    if !matches!(
        options.table,
        1 | 2
            | 3
            | 4
            | 5
            | 6
            | 9
            | 10
            | 11
            | 12
            | 13
            | 14
            | 16
            | 21
            | 22
            | 23
            | 24
            | 25
            | 26
            | 27
            | 28
            | 29
            | 30
            | 31
    ) {
        return Err(CommandError::usage(
            "unsupported translation table; supported IDs are 1-6, 9-14, 16 and 21-31",
        ));
    }
    let frames = if options.frames.contains(&6) {
        vec![1, 2, 3, -1, -2, -3]
    } else {
        options.frames.clone()
    };
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut successful_frames = 0usize;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fasta,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            for frame in &frames {
                let (protein, had_error) = translate_sequence(&record.sequence, *frame, &options)?;
                if had_error {
                    let mut translated = record.clone();
                    translated.name.extend_from_slice(b" ");
                    translated.raw_header.extend_from_slice(b" ");
                    translated.sequence.clear();
                    translated.qualities.clear();
                    translated.plus.clear();
                    translated.has_quality_line = false;
                    translated.write_to(&mut out, layout, options.line_width)?;
                    continue;
                }
                successful_frames += 1;
                if options.out_subseqs {
                    write_orfs(&record, &protein, *frame, &options, &mut out)?;
                } else {
                    let mut translated = record.clone();
                    translated.sequence = protein.as_bytes().to_vec();
                    translated.qualities.clear();
                    translated.plus.clear();
                    translated.has_quality_line = false;
                    if options.append_frame {
                        translated
                            .name
                            .extend_from_slice(format!("_frame={frame} ").as_bytes());
                    }
                    let marker = record.raw_header.first().copied().unwrap_or(b'>');
                    translated.raw_header = format!(
                        "{}{}",
                        char::from(marker),
                        String::from_utf8_lossy(&translated.name)
                    )
                    .into_bytes();
                    translated.write_to(&mut out, layout, options.line_width)?;
                }
            }
        }
    }
    if options.skip_errors && successful_frames == 0 {
        return Err(CommandError::runtime("no sequences could be translated").with_code(255));
    }
    out.finish()
}
