#!/usr/bin/env python3
"""Create the deterministic synthetic UMI workflow fixture.

This is a pre-aligned fixture for a consumer smoke. It is NOT the output of an
aligner and must not be presented as a biological alignment result.
"""

from pathlib import Path
import random
import sys

out = Path(sys.argv[1] if len(sys.argv) > 1 else "fixtures")
out.mkdir(parents=True, exist_ok=True)
rng = random.Random(20261023)
bases = "ACGT"
adapter = "AGATCGGAAGAGC"

with (out / "reference.fa").open("w", encoding="ascii") as reference:
    for name in ("chr1", "chr2"):
        sequence = "".join(rng.choice(bases) for _ in range(2000))
        reference.write(f">{name}\n{sequence}\n")

with (out / "reads.fastq").open("w", encoding="ascii") as fastq, (
    out / "aligned.sam"
).open("w", encoding="ascii") as sam:
    sam.write("@HD\tVN:1.6\tSO:unsorted\n")
    sam.write("@SQ\tSN:chr1\tLN:2000\n@SQ\tSN:chr2\tLN:2000\n")
    for index in range(240):
        sequence = "".join(rng.choice(bases) for _ in range(100))
        if index % 10 == 0:
            sequence = sequence[:50] + adapter + sequence[50 + len(adapter):]
        fastq.write(f"@read{index:04d}\n{sequence}\n+\n{'I' * len(sequence)}\n")

        ref = "chr1" if index % 2 else "chr2"
        flag = 16 if index % 5 == 0 else 0
        position = index // 3 + 1
        umi = f"UMI{index % 3:05d}"
        gene = "geneA" if index % 2 else "geneB"
        aligned_sequence = sequence[:50]
        sam.write(
            f"read{index:04d}_{umi}\t{flag}\t{ref}\t{position}\t60\t50M\t*\t0\t0\t"
            f"{aligned_sequence}\t{'I' * 50}\tUB:Z:{umi}\tGX:Z:{gene}\n"
        )

print(f"wrote synthetic fixture to {out} (pre-aligned SAM, not an aligner result)")
