#!/usr/bin/env python3
"""Prepare the protocol's 10M streaming extension without changing earlier inputs."""
import argparse
import json
from pathlib import Path

from readme_inputs import SEED, alignments, digest, scan_fastq, synthetic


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--inputs', type=Path, required=True)
    p.add_argument('--samtools', type=Path, required=True)
    args = p.parse_args()
    root = args.inputs.resolve().parent
    out = root/'inputs-large.json'
    fq = root/'synthetic-10000000.fastq'
    bam_dir = root/'bam-10000000'
    if out.exists() or fq.exists() or bam_dir.exists():
        p.error('extension already exists; refusing to overwrite')
    original = json.loads(args.inputs.read_text())
    synthetic(fq, 10000000)
    fastq = {'id':'synthetic-10000000','kind':'fastq','path':str(fq),
             'source':'synthetic','seed':SEED, **scan_fastq(fq)}
    bam_dir.mkdir()
    bam = alignments(bam_dir, 10000000, args.samtools.resolve())
    result = {'schema_version':1,'seed':SEED,'real_source':original['real_source'],
              'parent_manifest_sha256':digest(args.inputs),
              'generator_sha256':digest(Path(__file__)),
              'shared_generator_sha256':digest(Path(__file__).with_name('readme_inputs.py')),
              'datasets':[fastq,bam]}
    out.write_text(json.dumps(result,indent=2)+'\n')
    print(f'prepared {out}',flush=True)


if __name__=='__main__':
    main()
