#!/usr/bin/env python3
"""Build and freeze exactly the artifacts used by the scale benchmark."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from readme_inputs import digest

ROOT = Path(__file__).resolve().parents[1]
NAMES = ['rust-cutadapt', 'rust-fx', 'rust-bam', 'rust-fastqc', 'rust-umitools']


def source_hashes():
    paths = [ROOT/'Cargo.toml', ROOT/'Cargo.lock', *sorted(ROOT.glob('*/Cargo.toml')), *sorted(ROOT.glob('*/src/**/*.rs'))]
    return {str(p.relative_to(ROOT)): digest(p) for p in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--outdir', type=Path, required=True)
    args = parser.parse_args()
    root = args.outdir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    recipe = [['cargo', 'build', '--release', '--workspace', '--bins', '--locked'],
              ['cargo', 'build', '--release', '--locked', '-p', 'rust-fx'],
              ['rustc', '--edition', '2021', '-O', str(ROOT/'bench/canonical_sam.rs'), '-o', str(root/'canonical-sam')]]
    metadata = {'passed': False, 'source_sha256': source_hashes(), 'recipe': recipe,
                'base_commit': subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip(),
                'rustc': subprocess.check_output(['rustc','-Vv'], cwd=ROOT, text=True),
                'cargo': subprocess.check_output(['cargo','--version'], cwd=ROOT, text=True).strip(),
                'executables': {}, 'seal_script_sha256': digest(Path(__file__))}
    try:
        with (root/'build.log').open('w') as log:
            for cmd in recipe:
                log.write(json.dumps(cmd) + '\n'); log.flush()
                subprocess.run(cmd, cwd=ROOT, env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'target')),
                               stdout=log, stderr=subprocess.STDOUT, check=True)
        if metadata['source_sha256'] != source_hashes():
            raise ValueError('source changed during build')
        for name in NAMES:
            destination = root/name
            shutil.copy2(ROOT/'target/release'/name, destination)
            metadata['executables'][name] = {'path': str(destination), 'sha256': digest(destination)}
        metadata['canonicalizer'] = {'path': str(root/'canonical-sam'), 'sha256': digest(root/'canonical-sam'),
                                     'source_sha256': digest(ROOT/'bench/canonical_sam.rs')}
        metadata['passed'] = True
    except BaseException as error:
        metadata['error'] = repr(error)
        raise
    finally:
        (root/'build-provenance.json').write_text(json.dumps(metadata, indent=2) + '\n')


if __name__ == '__main__':
    main()
