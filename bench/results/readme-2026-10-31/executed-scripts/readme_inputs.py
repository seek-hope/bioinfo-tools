#!/usr/bin/env python3
"""Prepare the declared README workloads; no generation is included in timing."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import random
import shutil
import subprocess
import urllib.request

SEED = 20261031
ACCESSION = "SRR2584863"
URL = "https://ftp.sra.ebi.ac.uk/vol1/fastq/SRR258/003/SRR2584863/SRR2584863_1.fastq.gz"
MD5 = "4a1529a4165970302d6d8cb50b6e7a43"
SOURCE_BYTES = 183292913
SOURCE_READS = 1553259
BASES = bytes(b"ACGT"[i % 4] for i in range(256))
ADAPTER = b"AGATCGGAAGAGC"


def digest(path: Path, algorithm: str = "sha256") -> str:
    h = hashlib.new(algorithm)
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def scan_fastq(path: Path) -> dict:
    n = bases = gc = 0
    minimum = None
    maximum = 0
    with path.open("rb") as f:
        while header := f.readline():
            seq, plus, qual = f.readline().rstrip(b"\r\n"), f.readline(), f.readline().rstrip(b"\r\n")
            if not header.startswith(b"@") or not plus.startswith(b"+") or not seq or len(seq) != len(qual):
                raise ValueError(f"invalid record {n + 1} in {path}")
            if set(seq.upper()) - set(b"ACGTN") or any(q < 33 or q > 126 for q in qual):
                raise ValueError(f"invalid sequence/quality {n + 1} in {path}")
            n += 1
            bases += len(seq)
            gc += seq.upper().count(b"G") + seq.upper().count(b"C")
            minimum = len(seq) if minimum is None else min(minimum, len(seq))
            maximum = max(maximum, len(seq))
    return {"records": n, "bases": bases, "gc_bases": gc, "min_length": minimum, "max_length": maximum,
            "sha256": digest(path), "bytes": path.stat().st_size, "independent_fastq_validation": True}


def synthetic(path: Path, count: int) -> None:
    rng = random.Random(SEED)
    previous = b""
    with path.open("wb") as f:
        for i in range(count):
            seq = rng.randbytes(150).translate(BASES)
            if i % 10 == 0:
                seq = seq[:100] + ADAPTER + seq[100 + len(ADAPTER):]
            if i and i % 20 == 1:
                seq = previous
            previous = seq
            quality = (b"?" * 100 + b"5" * 50) if i % 1000 else b"#" * 150
            f.write(f"@synthetic{i:09d}\n".encode() + seq + b"\n+\n" + quality + b"\n")


def subset_real(source: Path, paths: dict[int, Path]) -> None:
    # Each size has a seeded simple random sample; sorted indices retain source order.
    selected = {n: set(random.Random(SEED).sample(range(SOURCE_READS), n)) for n in paths}
    handles = {n: path.open("wb") for n, path in paths.items()}
    seen = 0
    try:
        with gzip.open(source, "rb") as f:
            while first := f.readline():
                record = first + f.readline() + f.readline() + f.readline()
                for n, chosen in selected.items():
                    if seen in chosen:
                        handles[n].write(record)
                seen += 1
    finally:
        for f in handles.values():
            f.close()
    if seen != SOURCE_READS:
        raise ValueError(f"source has {seen} records, expected {SOURCE_READS}")


def alignments(directory: Path, count: int, samtools: Path) -> dict:
    rng = random.Random(SEED)
    groups = []
    left = count
    while left:
        if left < 10 or left % 10:
            raise ValueError("alignment count must be a multiple of 10")
        g = len(groups)
        scale = min(100 if g % 100 == 0 else (10 if g % 10 == 0 else 1), left // 10)
        groups.append((g, scale, rng.randbytes(50).translate(BASES), rng.randbytes(10).translate(BASES)))
        left -= 10 * scale
    sam = directory / "unsorted.sam"
    expected = directory / "dedup-qnames.txt"
    complement = bytes.maketrans(b"ACGT", b"TGCA")
    selected = []
    with sam.open("wb") as f:
        f.write(f"@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:chr1\tLN:{len(groups) * 200 + 1000}\n".encode())
        for g, scale, seq, umi in reversed(groups):
            neighbor = umi[:9] + umi[9:].translate(complement)
            distance_two = neighbor[:8] + neighbor[8:9].translate(complement) + neighbor[9:]
            far = umi.translate(complement)
            serial = 0
            for k, (tag, abundance) in enumerate(((umi, 6 * scale), (neighbor, 2 * scale), (distance_two, scale), (far, scale))):
                for j in range(abundance):
                    name = f"g{g:08d}_r{serial:05d}"
                    mapq = 60 if j == 0 and k in (0, 3) else 30
                    if mapq == 60:
                        selected.append(name)
                    f.write(f"{name}\t0\tchr1\t{g * 200 + 1}\t{mapq}\t50M\t*\t0\t0\t".encode()
                            + seq + b"\t" + b"I" * 50 + b"\tUB:Z:" + tag + b"\n")
                    serial += 1
    expected.write_text("\n".join(sorted(selected)) + "\n")
    bam = directory / "unsorted.bam"
    ordered = directory / "sorted.bam"
    for cmd in ([str(samtools), "view", "-b", "-o", str(bam), str(sam)],
                [str(samtools), "sort", "-@", "0", "-m", "256M", "-l", "6", "-o", str(ordered), str(bam)],
                [str(samtools), "index", str(ordered)]):
        subprocess.run(cmd, check=True)
    sam.unlink()
    return {"id": f"synthetic-bam-{count}", "kind": "bam", "records": count,
            "bundles": len(groups), "expected_dedup_records": len(selected),
            "unsorted": str(bam), "sorted": str(ordered), "expected_qnames": str(expected),
            "sha256": {p.name: digest(p) for p in (bam, ordered, Path(str(ordered) + '.bai'), expected)},
            "generator": {"seed": SEED, "bundle_scales": [1, 10, 100], "umi_counts_per_scale": [6, 2, 1, 1],
                          "umi_distance_from_major": [0, 1, 2, 10], "independent_representative": "unique highest MAPQ"}}


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--workdir", type=Path, required=True)
    p.add_argument("--samtools", type=Path, required=True)
    p.add_argument("--sizes", default="100000,1000000")
    p.add_argument("--large", action="store_true", help="also prepare 10M synthetic FASTQ")
    args = p.parse_args()
    root = args.workdir.resolve()
    root.mkdir(parents=True, exist_ok=True)
    source = root / f"{ACCESSION}_1.fastq.gz"
    if not source.exists():
        partial = source.with_suffix(source.suffix + ".part")
        if not partial.exists():
            with urllib.request.urlopen(URL, timeout=60) as response, partial.open("wb") as out:
                shutil.copyfileobj(response, out)
        if partial.stat().st_size != SOURCE_BYTES or digest(partial, "md5") != MD5:
            raise ValueError("ENA source byte count / MD5 mismatch")
        partial.rename(source)
    if source.stat().st_size != SOURCE_BYTES or digest(source, "md5") != MD5:
        raise ValueError("cached ENA source byte count / MD5 mismatch")
    sizes = [int(x) for x in args.sizes.split(",")]
    paths = {n: root / f"real-{n}.fastq" for n in sizes}
    subset_real(source, paths)
    data = []
    for n in sizes + ([10000000] if args.large else []):
        path = root / f"synthetic-{n}.fastq"
        synthetic(path, n)
        data.append({"id": f"synthetic-{n}", "kind": "fastq", "path": str(path),
                     "source": "synthetic", "seed": SEED, **scan_fastq(path)})
    for n, path in paths.items():
        data.append({"id": f"real-{n}", "kind": "fastq", "path": str(path), "source": ACCESSION,
                     "sampling": "seeded uniform record indices without replacement; retained source order",
                     "seed": SEED, **scan_fastq(path)})
    for n in sizes:
        directory = root / f"bam-{n}"
        directory.mkdir(exist_ok=True)
        data.append(alignments(directory, n, args.samtools.resolve()))
    manifest = {"schema_version": 1, "seed": SEED,
                "real_source": {"run": ACCESSION, "organism": "Escherichia coli B str. REL606",
                                "instrument": "Illumina HiSeq 2500", "mate": 1, "url": URL,
                                "md5": MD5, "bytes": SOURCE_BYTES, "records": SOURCE_READS,
                                "sha256": digest(source)},
                "generator_sha256": digest(Path(__file__)), "datasets": data}
    (root / "inputs.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"manifest": str(root / "inputs.json"), "datasets": [(x["id"], x["records"]) for x in data]}), flush=True)


if __name__ == "__main__":
    main()
