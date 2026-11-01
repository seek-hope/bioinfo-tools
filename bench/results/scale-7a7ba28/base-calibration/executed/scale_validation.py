#!/usr/bin/env python3
"""Full streaming BAM validation for enlarged benchmarks; no record fields discarded."""
from __future__ import annotations
import hashlib
from pathlib import Path
import subprocess
import tempfile
import threading


def bam_summary(path: Path, samtools: Path, canonicalizer: Path, timeout: float = 1800) -> dict:
    check = subprocess.run([str(samtools), 'quickcheck', str(path)], capture_output=True, timeout=timeout)
    if check.returncode:
        raise ValueError(f'BAM quickcheck failed: {check.stderr!r}')
    with path.open('rb') as handle:
        if path.stat().st_size < 28:
            raise ValueError('BAM lacks BGZF EOF marker')
        handle.seek(-28, 2)
        if handle.read() != bytes.fromhex('1f8b08040000000000ff0600424302001b0003000000000000000000'):
            raise ValueError('BAM lacks canonical BGZF EOF marker')
    checksum = hashlib.sha256()
    with tempfile.TemporaryFile() as errors:
        with subprocess.Popen([str(samtools), 'view', '-h', str(path)], stdout=subprocess.PIPE, stderr=errors) as source:
            with subprocess.Popen([str(canonicalizer)], stdin=source.stdout, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as canonical:
                assert source.stdout is not None and canonical.stdout is not None and canonical.stderr is not None
                source.stdout.close()
                timed_out = threading.Event()
                def stop():
                    timed_out.set()
                    for process in (source, canonical):
                        try:
                            process.kill()
                        except ProcessLookupError:
                            pass
                watchdog = threading.Timer(timeout, stop)
                watchdog.daemon = True
                watchdog.start()
                try:
                    with canonical.stdout as output:
                        for chunk in iter(lambda: output.read(1024 * 1024), b''):
                            checksum.update(chunk)
                    with canonical.stderr as diagnostics:
                        detail = diagnostics.read()
                    canonical_code = canonical.wait()
                    source_code = source.wait()
                    errors.seek(0)
                    source_errors = errors.read()
                finally:
                    watchdog.cancel()
                if timed_out.is_set() or source_code or canonical_code:
                    raise ValueError(f'BAM validation failed: source={source_code}, canonical={canonical_code}, timeout={timed_out.is_set()}, {source_errors!r}, {detail!r}')
                if not detail.startswith(b'records=') or detail.count(b'\n') != 1:
                    raise ValueError(f'invalid canonicalizer count: {detail!r}')
                return {'records': int(detail[8:]), 'normalized_sha256': checksum.hexdigest(),
                        'rule': 'full SAM field tuples; only @PG omitted, coordinate ties sorted',
                        'bgzf_eof': True, 'samtools_stderr': source_errors.decode(errors='replace')}
