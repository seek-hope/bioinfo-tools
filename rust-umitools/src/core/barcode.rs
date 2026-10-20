//! UMI and cell barcode extraction.
//!
//! Port of `umi_tools.sam_methods.get_barcode_read_id` / `get_barcode_tag`. No
//! byte is normalised: the extracted UMI is handed to the UMI layer unchanged.

use crate::core::umi::Umi;
use crate::error::{Error, InputError, Result};
use crate::io::alignment::aux_str;
use rust_htslib::bam::Record;

#[derive(Clone, Debug)]
pub enum UmiSource {
    /// Last `sep`-separated field of the read name.
    ReadId { sep: u8 },
    /// `umi_tag`, with optional split/delimiter post-processing.
    Tag {
        umi_tag: [u8; 2],
        umi_tag_split: Option<String>,
        umi_tag_delim: Option<String>,
    },
    /// `UMI_` / `CELL_` prefixed fields in the read name (`umi_tools extract` output).
    Umis,
}

fn apply_split(value: &str, split: &Option<String>, delim: &Option<String>) -> String {
    let mut out = value.to_string();
    if let Some(sep) = split {
        out = out.split(sep.as_str()).next().unwrap_or("").to_string();
    }
    if let Some(sep) = delim {
        out = out.split(sep.as_str()).collect::<Vec<_>>().join("");
    }
    out
}

/// Extract the UMI for one record.
pub fn extract_umi(source: &UmiSource, record: &Record) -> Result<Umi> {
    match source {
        UmiSource::ReadId { sep } => {
            let name = record.qname();
            let position = name.iter().rposition(|b| b == sep);
            let field = match position {
                Some(index) => &name[index + 1..],
                None => name,
            };
            if field.is_empty() {
                return Err(Error::Input(InputError::MissingUmi {
                    detail: format!(
                        "read name '{}' has an empty UMI field",
                        String::from_utf8_lossy(name)
                    ),
                }));
            }
            Ok(Umi::from_bytes(field))
        }
        UmiSource::Tag {
            umi_tag,
            umi_tag_split,
            umi_tag_delim,
        } => {
            let raw = aux_str(record, umi_tag)?;
            let value = apply_split(&raw, umi_tag_split, umi_tag_delim);
            if value.is_empty() {
                return Err(Error::Input(InputError::MissingUmi {
                    detail: format!("tag '{}' is empty", String::from_utf8_lossy(umi_tag)),
                }));
            }
            Ok(Umi::from_bytes(value.as_bytes()))
        }
        UmiSource::Umis => {
            let name = record.qname();
            let mut umi: Option<&[u8]> = None;
            for field in name.split(|b| *b == b':') {
                if let Some(rest) = field.strip_prefix(b"UMI_") {
                    umi = Some(rest);
                }
            }
            match umi {
                Some(bytes) if !bytes.is_empty() => Ok(Umi::from_bytes(bytes)),
                _ => Err(Error::Input(InputError::MissingUmi {
                    detail: "no UMI_ field in the read name".to_string(),
                })),
            }
        }
    }
}

/// Cell barcode extraction, used by `--per-cell`.
pub fn extract_cell(
    source: &UmiSource,
    record: &Record,
    cell_tag: &[u8; 2],
    cell_tag_split: &Option<String>,
    cell_tag_delim: &Option<String>,
) -> Result<Box<[u8]>> {
    match source {
        UmiSource::ReadId { sep } => {
            let name = record.qname();
            let fields: Vec<&[u8]> = name.split(|b| *b == *sep).collect();
            if fields.len() < 2 {
                return Err(Error::Input(InputError::MissingUmi {
                    detail: format!(
                        "read name '{}' has no cell barcode field",
                        String::from_utf8_lossy(name)
                    ),
                }));
            }
            Ok(fields[fields.len() - 2].into())
        }
        UmiSource::Tag { .. } | UmiSource::Umis => {
            let raw = aux_str(record, cell_tag)?;
            let value = apply_split(&raw, cell_tag_split, cell_tag_delim);
            Ok(value.into_bytes().into())
        }
    }
}
