#!/usr/bin/env bash
# End-to-end UMI workflow consumer smoke.
#
# MODE=prebuilt uses the synthetic pre-aligned SAM from make_fixture.py. It is
# explicitly not an aligner result. MODE=align requires ALIGNER=bwa|minimap2 and a
# reference; a missing aligner is a hard error, not a simulated alignment.
set -euo pipefail

DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$DIR/../.." && pwd)
BIN=${BIN:-"$ROOT/target/release"}
OUT=${OUT:-"$DIR/out"}
MODE=${MODE:-prebuilt}
REF=${REF:-"$DIR/fixtures/reference.fa"}

for tool in rust-cutadapt rust-fastqc rust-bam rust-umitools; do
    if [ ! -x "$BIN/$tool" ]; then
        printf 'ERROR: missing %s at %s (build with cargo build --release --bins)\n' "$tool" "$BIN/$tool" >&2
        exit 3
    fi
done

python3 "$DIR/make_fixture.py" "$DIR/fixtures"
mkdir -p "$OUT" "$OUT/qc"

"$BIN/rust-cutadapt" --quiet -a AGATCGGAAGAGC -m 30 \
    -o "$OUT/trimmed.fastq.gz" "$DIR/fixtures/reads.fastq"
"$BIN/rust-fastqc" -o "$OUT/qc" "$DIR/fixtures/reads.fastq"

if [ "$MODE" = "prebuilt" ]; then
    cp "$DIR/fixtures/aligned.sam" "$OUT/aligned.sam"
    printf 'using synthetic pre-aligned SAM (not an aligner result)\n'
elif [ "$MODE" = "align" ]; then
    : "${ALIGNER:?set ALIGNER=bwa|minimap2 for MODE=align}"
    command -v "$ALIGNER" >/dev/null 2>&1 || {
        printf 'ERROR: aligner %s is not installed; use MODE=prebuilt for the synthetic smoke\n' "$ALIGNER" >&2
        exit 3
    }
    [ -f "$REF" ] || {
        printf 'ERROR: reference not found: %s\n' "$REF" >&2
        exit 3
    }
    case "$ALIGNER" in
        bwa) bwa mem -t 4 "$REF" "$DIR/fixtures/reads.fastq" > "$OUT/aligned.sam" ;;
        minimap2) minimap2 -a -t 4 "$REF" "$DIR/fixtures/reads.fastq" > "$OUT/aligned.sam" ;;
        *) printf 'ERROR: unsupported ALIGNER=%s (use bwa or minimap2)\n' "$ALIGNER" >&2; exit 3 ;;
    esac
else
    printf 'ERROR: MODE must be prebuilt or align, got %s\n' "$MODE" >&2
    exit 3
fi

"$BIN/rust-bam" view -b -o "$OUT/aligned.bam" "$OUT/aligned.sam"
"$BIN/rust-bam" sort -o "$OUT/aligned.sorted.bam" "$OUT/aligned.bam"
"$BIN/rust-bam" index "$OUT/aligned.sorted.bam"

# The fixture carries UB (UMI) and GX (gene) tags. A real protocol may need a
# different extraction order; this is the documented order for this fixture.
"$BIN/rust-umitools" dedup -I "$OUT/aligned.sorted.bam" -S "$OUT/dedup.bam" \
    --extract-umi-method=tag --umi-tag=UB
"$BIN/rust-bam" sort -o "$OUT/dedup.sorted.bam" "$OUT/dedup.bam"
"$BIN/rust-bam" index "$OUT/dedup.sorted.bam"
"$BIN/rust-umitools" count -I "$OUT/dedup.sorted.bam" -S "$OUT/counts.tsv" \
    --per-gene --gene-tag GX

"$BIN/rust-bam" quickcheck "$OUT/aligned.sorted.bam" "$OUT/dedup.sorted.bam"
test -s "$OUT/counts.tsv"
grep -q 'Total Sequences' "$OUT/qc/reads_fastqc/fastqc_data.txt"

if command -v multiqc >/dev/null 2>&1; then
    multiqc "$OUT/qc" -o "$OUT/multiqc" --force >/dev/null
    test -s "$OUT/multiqc/multiqc_report.html"
    printf 'MultiQC report: %s\n' "$OUT/multiqc/multiqc_report.html"
else
    printf 'MultiQC not installed; skipped consumer report (not counted as pass)\n'
fi

printf 'workflow PASS: %s\n' "$OUT"
printf '  trimmed: %s\n  aligned sorted/indexed: %s\n  dedup sorted/indexed: %s\n  counts: %s\n' \
    "$OUT/trimmed.fastq.gz" "$OUT/aligned.sorted.bam" "$OUT/dedup.sorted.bam" "$OUT/counts.tsv"
