#!/usr/bin/env python3
"""Declarative performance coverage, reconciled to the compatibility matrix.

This is a *coverage plan*, not a measured manifest. A case with actual command
specifications still needs fresh inputs, identities, calibration and raw validation.
Every supported compatibility surface must map to a declared family or an explicit
non-workload exclusion; unsupported/unachievable rows are never timed as successes.
"""
from __future__ import annotations

import csv
import json
import re
from pathlib import Path
from types import MappingProxyType

from superiority_batch import digest, validate_spec
from superiority_plan import BUDGETS, fingerprint

ROOT = Path(__file__).resolve().parents[1]
FAMILIES = ('cutadapt-matching', 'cutadapt-modification', 'fx-statistics',
            'fx-selection', 'fx-matching', 'fx-editing', 'fx-partition', 'fx-validation',
            'bam-query', 'bam-sorting', 'bam-coverage', 'bam-short',
            'umi-fastq', 'umi-alignment', 'umi-counting', 'fastqc')
ID = re.compile(r'[A-Za-z0-9][A-Za-z0-9_.-]*\Z')


def historical(root=ROOT):
    """Retain historical conditions as inputs to a fresh experiment, not results."""
    result = []
    definitions = [
        ('real-cutadapt', 'cutadapt-modification', 'real', 'plain', 'short', 'single', 'small-large'),
        ('synthetic-cutadapt', 'cutadapt-modification', 'synthetic', 'plain', 'short', 'single', 'small-large'),
        ('real-cutadapt-gzip', 'cutadapt-modification', 'real', 'gzip', 'short', 'single', 'large'),
        ('real-stats', 'fx-statistics', 'real', 'plain', 'short', 'single', 'large'),
        ('synthetic-stats', 'fx-statistics', 'synthetic', 'plain', 'short', 'single', 'large'),
        ('synthetic-fq2fa', 'fx-statistics', 'synthetic', 'plain', 'short', 'single', 'large'),
        ('synthetic-lint', 'fx-validation', 'synthetic', 'plain', 'short', 'single', 'large'),
        ('bam-flagstat', 'bam-query', 'synthetic', 'bgzf', 'short', 'paired', 'large'),
        ('bam-sort', 'bam-sorting', 'synthetic', 'bgzf', 'short', 'paired', 'large'),
        ('umi-dedup', 'umi-alignment', 'synthetic', 'bgzf', 'short', 'paired', 'large'),
        ('real-fastqc', 'fastqc', 'real', 'gzip', 'short', 'single', 'large'),
    ]
    for name, family, source, codec, reads, pairing, scale in definitions:
        result.append({'id': name, 'family': family, 'origin': 'historical-condition',
                       'evidence': 'bench/results/{scale-7a7ba28,bottleneck-21bc596}/formal-summary/',
                       'comparison': None,
                       'equal_work': name != 'real-fastqc',
                       'scope': 'fresh run required under expanded resource/evidence protocol; historical ratio retained separately',
                       'profile': {'source': source, 'codec': codec, 'reads': reads,
                                   'pairing': pairing, 'scale': scale},
                       'workloads': ['small', 'large'], 'modes': ['historical-default'],
                       'commands': {}})
    return result


REQUIRED_MODES = {
    'cutadapt-matching': {'3prime', '5prime', 'anchored', 'anywhere', 'linked',
                          'wildcards', 'indels', 'no-indels', 'reverse-complement',
                          'repeated', 'long-adapter', 'adapter-rich', 'adapter-poor',
                          'one-adapter', '32-adapters'},
    'cutadapt-modification': {'quality', 'nextseq', 'poly-a', 'fixed', 'n-trim',
                              'filters', 'actions', 'rename', 'auxiliary', 'demux',
                              'se', 'two-file-pe', 'interleaved-pe', 'fasta', 'fastq',
                              'plain', 'gzip', 'bzip2', 'xz', 'small', 'large'},
    'fx-statistics': {'stats', 'seq', 'fq2fa', 'fx2tab', 'tab2fx', 'fa2fq',
                      'basic', 'extended', 'offsets', 'gaps', 'wrapped', 'four-line',
                      'plain', 'gzip', 'bzip2', 'xz', 'zstd', 'small', 'large',
                      'short', 'long'},
    'fx-selection': {'head', 'range', 'subseq', 'sliding', 'restart', 'translate',
                     'negative-tail', 'bed', 'gtf', 'flank', 'circular', 'six-frames',
                     'codon-tables', 'small', 'large', 'short', 'long'},
    'fx-matching': {'grep', 'common', 'rmdup', 'id', 'name', 'sequence', 'regex',
                    'degenerate', 'mismatch', 'embedded', '1-pattern', '1000-patterns',
                    '0-percent-duplicates', '90-percent-duplicates', '2-files', '8-files'},
    'fx-editing': {'sort', 'duplicate', 'rename', 'replace', 'id', 'natural', 'length',
                   'sequence', 'two-pass', 'regex', 'key-value', 'uuid-semantic',
                   'compressed-two-pass-rejection', 'small', 'large'},
    'fx-partition': {'sample', 'split', 'faidx', 'proportion', 'count', 'records', 'parts',
                     'fresh-index', 'reused-index', 'explicit-region', 'regex-region',
                     'input-order-semantic', 'rng-scope'},
    'fx-validation': {'lint', 'valid', 'malformed-tail', 'se', 'pe', 'paired-name-growth',
                      'stdin', 'plain', 'gzip', 'small', 'large'},
    'bam-query': {'view', 'flagstat', 'idxstats', 'index', 'sam', 'bam', 'filters', 'count',
                  'header', 'regions', 'bai', 'csi', 'tags', 'cigar', 'reference-names',
                  'paired-flags', 'small', 'large'},
    'bam-sorting': {'in-memory', 'one-spill', 'multilevel', 'sam-output', 'bam-output',
                    'memory-budgets', 'fanins', 'compression-levels', 'long-records',
                    'ties', 'small', 'large'},
    'bam-coverage': {'depth', 'coverage', 'zero-positions', 'mate-overlap', 'region',
                     'bed', 'multiple-inputs', 'histogram', 'reference-span', 'small', 'large'},
    'bam-short': {'quickcheck', 'head', 'flags', '1-file', '100-distinct-files',
                  'header-only', 'limited-records', 'invalid', 'numeric', 'named',
                  'sam', 'bam', 'gzip', 'bgzf', 'stdin', 'error-masks'},
    'umi-fastq': {'extract', 'whitelist', 'string', 'regex', '5prime', '3prime', 'se', 'pe',
                  'quality', 'mask', 'read-count', 'umi-count', 'knees', 'corrections',
                  'barcode-diversity', 'small', 'large'},
    'umi-alignment': {'dedup', 'group', 'five-methods', 'name', 'tag', 'umis', 'gene', 'cell',
                      'transcript', 'statistics', 'graphs', 'representatives', 'rng', 'se', 'pe',
                      'paired-whitelist', 'cram-reference', 'small', 'large'},
    'umi-counting': {'count', 'count_tab', 'gene', 'contig', 'transcript', 'sparse', 'wide',
                     'gene-cell', 'partition', 'spill', 'small', 'large'},
    'fastqc': {'12-modules', 'default', 'kmer', 'tile', 'custom', 'long', 'grouped',
               'multiple-files', 'duplication', 'length-options', 'html', 'zip',
               'report-assets', 'small', 'large'},
}


REQUIRED_MODES = MappingProxyType({key: frozenset(value) for key, value in REQUIRED_MODES.items()})


def planned():
    """Explicit cases to expand, including cases that still need new validators.

    `commands` is empty only while the case is not executable. A case being listed
    here is never evidence of execution or compatibility closure.
    """
    result = []
    for family, modes in REQUIRED_MODES.items():
        # A family-level case is an inventory obligation, not a single shortcut
        # covering every mode in one invocation. Expansion must preserve all modes.
        result.append({'id': 'planned-' + family, 'family': family,
                       'origin': 'coverage-inventory', 'evidence': None,
                       'comparison': 'descriptive' if family == 'fastqc' else None,
                       'equal_work': False,
                       'scope': 'not executable until split into concrete CLI/input/validator cases',
                       'profile': None, 'workloads': [], 'modes': sorted(modes), 'commands': {}})
    return result


def bam_short(inputs, executables):
    """Expand the already-exercised byte family using caller-frozen actual files.

    This does not reuse old times. Specifications are validated structurally;
    their identities, inputs, calibration and all fresh output bytes still must
    be checked by the execution/evidence layers.
    """
    if not isinstance(inputs, dict) or not isinstance(executables, dict) or set(executables) != {'current', 'upstream'}:
        raise ValueError('named input files and both executable paths required')
    required_roles = {'bam', 'no-eof', 'empty', 'header', 'sam', 'gzip', 'bgzf', 'distinct'}
    if not required_roles <= inputs.keys():
        raise ValueError('all semantic input roles required')
    for key, value in inputs.items():
        if key == 'distinct' and not isinstance(value, list):
            raise ValueError('distinct role must be a path list')
        if key != 'distinct' and not isinstance(value, Path):
            raise ValueError('non-distinct roles must each name one path')
        paths = value if isinstance(value, list) else [value]
        if not paths or not all(isinstance(p, Path) and p.is_absolute() for p in paths):
            raise ValueError('absolute input paths required')
        for path in paths:
            if path.is_symlink() or not path.is_file():
                raise ValueError('input roles must be regular non-symlink files')
    role_paths = [inputs[key] for key in required_roles-{'distinct'}] + list(inputs['distinct'])
    if len({(p.stat().st_dev, p.stat().st_ino) for p in role_paths}) != len(role_paths):
        raise ValueError('semantic input roles and distinct files must not alias')
    if not all(isinstance(p, Path) and p.is_absolute() for p in executables.values()):
        raise ValueError('absolute executable paths required')
    cases = [('flags-numeric', ['flags', '99'], None, 0),
             ('flags-named', ['flags', 'PAIRED,PROPER_PAIR,MREVERSE,READ1'], None, 0),
             ('head-sam', ['head', '-n', '1', 'sam'], None, 0),
             ('head-bam', ['head', '-n', '1', 'bam'], None, 0),
             ('quickcheck-header-only', ['quickcheck', 'header'], None, 0),
             ('quickcheck-1', ['quickcheck', 'bam'], None, 0),
             ('quickcheck-100-repeated', ['quickcheck', *['bam']*100], None, 0),
             ('quickcheck-no-eof', ['quickcheck', 'no-eof'], None, 16),
             ('quickcheck-header-eof', ['quickcheck', 'empty'], None, 24),
             ('quickcheck-mixed-repeated', ['quickcheck', *['bam']*99, 'empty'], None, 24),
             ('quickcheck-100-distinct', ['quickcheck', *['distinct']*100], None, 0),
             ('quickcheck-mixed-distinct', ['quickcheck', *['distinct']*99, 'empty'], None, 24),
             ('quickcheck-stdin-bam', ['quickcheck', '-'], 'bam', 0),
             ('quickcheck-stdin-no-eof', ['quickcheck', '-'], 'no-eof', 16),
             ('quickcheck-stdin-sam', ['quickcheck', '-'], 'sam', 0),
             ('quickcheck-sam', ['quickcheck', 'sam'], None, 0),
             ('quickcheck-gzip-sam', ['quickcheck', 'gzip'], None, 0),
             ('quickcheck-bgzf-sam', ['quickcheck', 'bgzf'], None, 0)]
    if 'distinct' in inputs:
        distinct = inputs['distinct']
        if not isinstance(distinct, list) or len(distinct) != 100:
            raise ValueError('100 distinct input paths required')
        if len({str(p) for p in distinct}) != 100:
            raise ValueError('distinct input path names must be unique')
        for path in distinct:
            info = path.lstat()
            if not path.is_file() or path.is_symlink():
                raise ValueError('distinct inputs must be regular non-symlink files')
        if len({(p.stat().st_dev, p.stat().st_ino) for p in distinct}) != 100:
            raise ValueError('100 distinct input inodes required')
    result = []
    for name, args, stdin, code in cases:
        expanded = []
        distinct_index = 0
        for arg in args:
            if arg in ('bam', 'no-eof', 'empty', 'header', 'sam', 'gzip', 'bgzf'):
                if arg not in inputs:
                    raise ValueError(f'missing input role: {arg}')
                expanded.append(str(inputs[arg]))
            elif arg == 'distinct':
                if not isinstance(inputs.get('distinct'), list) or len(inputs['distinct']) != 100:
                    raise ValueError('100 distinct input paths required')
                expanded.append(str(inputs['distinct'][distinct_index]))
                distinct_index += 1
            else:
                expanded.append(arg)
        commands = {}
        for arm, executable in executables.items():
            commands[arm] = {}
            for cores in BUDGETS:
                spec = {'schema_version': 1, 'count': 2, 'cores': cores,
                        'stdin': str(inputs[stdin]) if stdin else None,
                        'expected_exit': code, 'argv': [str(executable), *expanded]}
                validate_spec(spec)
                commands[arm][str(cores)] = spec
        modes = ['flags' if name.startswith('flags') else 'head' if name.startswith('head') else 'quickcheck']
        input_mode = 'stdin' if stdin else '1-file' if set(args) & required_roles-{'distinct'} else None
        if input_mode:
            modes.append(input_mode)
        if '100-distinct' in name or 'mixed-distinct' in name:
            if '1-file' in modes:
                modes.remove('1-file')
            modes.append('100-distinct-files')
        if code:
            modes.extend(['invalid', 'error-masks'])
        if any(token in name for token in ('sam', 'gzip', 'bgzf')):
            modes.extend(x for x in ('sam', 'gzip', 'bgzf') if x in name)
        if 'bam' in name or 'head-bam' in name or set(args) & {'bam', 'no-eof', 'empty', 'header', 'distinct'} or stdin in ('bam', 'no-eof'):
            modes.append('bam')
        if name.startswith('flags'):
            modes.append('numeric' if name.endswith('numeric') else 'named')
        if name.startswith('head'):
            modes.append('limited-records')
        if name == 'quickcheck-header-only':
            modes.append('header-only')
        result.append({'id': name, 'family': 'bam-short', 'origin': 'coverage-inventory',
                       'evidence': None, 'comparison': 'exact', 'equal_work': True,
                       'scope': 'frozen deterministic complete CLI bytes; no live-pipe or timing eligibility implied',
                       'profile': {'source': 'existing quickcheck controls', 'reads': 'short',
                                   'input_identities': {key: (digest(path) if isinstance(path, Path) else [digest(p) for p in path])
                                                        for key, path in inputs.items() if key in set(args) or key == stdin}},
                       'workloads': ['small'], 'modes': modes, 'commands': commands})
    return result


def _surface_family(tool, surface, kind):
    """Explicit groups; unmatched supported rows are a hard coverage failure."""
    if tool == 'rust-cutadapt':
        matching = {'-a --adapter', '-g --front', '-b --anywhere', '-e --error-rate --errors',
                    '--no-indels', '-n --times', '-O --overlap', '--match-read-wildcards',
                    '-N --no-match-adapter-wildcards', '--rc --revcomp', '-A', '-G', '-B', '--pair-adapters'}
        modification = {'--debug', '-j --cores', '--action', '-u --cut', '--nextseq-trim',
                        '-q --quality-cutoff', '--quality-base', '--poly-a', '--length -l',
                        '--trim-n', '--length-tag', '--strip-suffix', '-x --prefix', '-y --suffix',
                        '--rename', '--zero-cap -z', '-m --minimum-length', '-M --maximum-length',
                        '--max-n', '--max-expected-errors --max-ee', '--max-average-error-rate --max-aer',
                        '--discard-trimmed --discard', '--discard-untrimmed --trimmed-only', '--discard-casava',
                        '--quiet', '--report', '--json', '-o --output', '--fasta', '-Z', '--info-file',
                        '-r --rest-file', '--wildcard-file', '--too-short-output', '--too-long-output',
                        '--untrimmed-output', '-U', '-Q', '-p --paired-output', '--pair-filter',
                        '--interleaved', '--untrimmed-paired-output', '--too-short-paired-output',
                        '--too-long-paired-output', 'report:text', 'report:json',
                        'compression:gzip/bzip2/xz', 'paired/interleaved'}
        if surface in matching:
            return 'cutadapt-matching'
        return 'cutadapt-modification' if surface in modification else None
    if tool == 'rust-bam':
        if surface == 'sort' or surface.startswith('sort:'):
            return 'bam-sorting'
        if surface in ('quickcheck', 'head', 'flags'):
            return 'bam-short'
        if surface == 'coverage' or surface == 'depth' or surface.startswith(('coverage:', 'depth:')):
            return 'bam-coverage'
        if surface in {'view', 'flagstat', 'idxstats', 'index', 'index:--csi'} or surface.startswith('view:'):
            return 'bam-query'
        return None
    if tool == 'rust-fx':
        command = surface.split(':', 1)[1] if surface.startswith(('seqkit:', 'fq:')) else surface.split(':', 1)[0]
        groups = {'fx-statistics': {'stats', 'seq', 'fq2fa', 'fx2tab', 'tab2fx', 'fa2fq', 'global'},
                  'fx-selection': {'head', 'range', 'subseq', 'sliding', 'restart', 'translate'},
                  'fx-matching': {'grep', 'common', 'rmdup'},
                  'fx-editing': {'sort', 'duplicate', 'rename', 'replace'},
                  'fx-partition': {'sample', 'split', 'faidx'},
                  'fx-validation': {'lint'}}
        return next((family for family, names in groups.items() if command in names), None)
    if tool == 'rust-fastqc':
        supported = {'--outdir', '-c --contaminants', '-a --adapters', '-l --limits',
                     '--min_length', '--max_length', '--dup_length', '-q --quiet', '--phred64',
                     '--nogroup', '--extract', '--delete', 'report:fastqc_data.txt', 'report:HTML',
                     'report:ZIP', 'module:adapter-content', 'module:kmer-content',
                     'module:per-tile-quality', 'module:contaminants', 'module:custom-contaminants'}
        return 'fastqc' if surface in supported else None
    if tool == 'rust-umitools':
        if surface.startswith(('extract:', 'whitelist:')) or surface == 'whitelist' or surface == 'extract':
            return 'umi-fastq'
        if surface.startswith(('dedup:', 'group:')) or surface in ('dedup', 'group'):
            return 'umi-alignment'
        if surface.startswith(('count:', 'count_tab:')) or surface in ('count', 'count_tab'):
            return 'umi-counting'
        # Shared global flags are exercised in each concrete family; they do not
        # constitute an independent workload by themselves.
        return 'umi-alignment' if surface.startswith('global:') else None
    return None


def audit(rows, cases=None):
    """Return a coverage report; never infer coverage from case IDs alone."""
    cases = historical() + planned() if cases is None else cases
    if not isinstance(rows, list) or not rows:
        raise ValueError('compatibility matrix population required')
    ids = set()
    fields = {'id', 'family', 'origin', 'evidence', 'comparison', 'equal_work',
              'scope', 'profile', 'workloads', 'modes', 'commands'}
    for case in cases:
        if not isinstance(case, dict) or set(case) != fields:
            raise ValueError('exact inventory case schema required')
        if type(case['id']) is not str or not ID.fullmatch(case['id']) or case['id'] in ids:
            raise ValueError('unique safe case IDs required')
        ids.add(case['id'])
        if case['origin'] not in ('historical-condition', 'coverage-inventory'):
            raise ValueError('unknown inventory provenance')
        if case['origin'] == 'historical-condition' and case['modes'] != ['historical-default']:
            raise ValueError('legacy labels cannot claim expanded mode coverage')
        if case['origin'] == 'historical-condition':
            if not isinstance(case['evidence'], str) or not case['evidence']:
                raise ValueError('historical evidence provenance required')
        elif case['evidence'] is not None:
            raise ValueError('planned cases cannot claim evidence')
        if not isinstance(case['scope'], str) or not case['scope'].strip():
            raise ValueError('case scope required')
        if case['profile'] is not None and not isinstance(case['profile'], dict):
            raise ValueError('invalid input profile')
        if not isinstance(case['workloads'], list) or not all(type(w) is str and w for w in case['workloads']):
            raise ValueError('invalid workload declaration')
        if not isinstance(case['commands'], dict):
            raise ValueError('command declaration must be an object')
        if case['family'] not in FAMILIES or not isinstance(case['modes'], list) or not case['modes']:
            raise ValueError('declared family and nonempty modes required')
        if not all(isinstance(mode, str) and mode for mode in case['modes']) or len(set(case['modes'])) != len(case['modes']):
            raise ValueError('case modes must be unique nonempty strings')
        if type(case['equal_work']) is not bool or case['comparison'] not in (None, 'exact', 'proved-correction', 'descriptive'):
            raise ValueError('explicit comparison status required')
        if case['comparison'] == 'descriptive' and case['equal_work']:
            raise ValueError('descriptive case cannot be equal-work')
        if case.get('commands'):
            if set(case['commands']) != {'current', 'upstream'}:
                raise ValueError('both command arms required when executable')
            for arm, by_budget in case['commands'].items():
                if not isinstance(by_budget, dict) or set(by_budget) != {str(c) for c in BUDGETS}:
                    raise ValueError('all command budgets required')
                for budget, spec in by_budget.items():
                    validate_spec(spec)
                    if spec['cores'] != int(budget):
                        raise ValueError('command budget key differs from CPU contract')
            if case['comparison'] == 'exact':
                for budget in map(str, BUDGETS):
                    current = case['commands']['current'][budget]
                    upstream = case['commands']['upstream'][budget]
                    for field in ('count', 'cores', 'stdin', 'expected_exit'):
                        if fingerprint(current[field]) != fingerprint(upstream[field]):
                            raise ValueError('exact arms differ in invocation/input/exit work')
                    if case['family'] == 'bam-short' and current['argv'][1:] != upstream['argv'][1:]:
                        raise ValueError('BAM short-command arms have different CLI work')
    by_id = {case['id']: case for case in cases}
    for expected in historical():
        actual = by_id.get(expected['id'])
        if actual is None:
            raise ValueError('historical performance condition omitted')
        for field, value in expected.items():
            if field != 'commands' and fingerprint(actual[field]) != fingerprint(value):
                raise ValueError('historical performance condition changed')
    mapped = []
    for row in rows:
        if set(row) != {'tool', 'upstream', 'surface', 'kind', 'status', 'unachievable', 'evidence', 'deviation', 'next_action'}:
            raise ValueError('unexpected compatibility matrix schema')
        if row['status'] == 'unsupported':
            disposition, family = 'excluded-not-implemented', None
        elif row['unachievable']:
            disposition, family = 'excluded-unachievable', None
        elif row['surface'] in ('-h --help', '-v --version', '--version', 'global:--help/--version'):
            disposition, family = 'excluded-metadata', None
        elif row['kind'] == 'feature' and row['surface'].startswith(('performance:', 'determinism:')):
            disposition, family = 'excluded-validation-assertion', None
        else:
            family = _surface_family(row['tool'], row['surface'], row['kind'])
            if family is None:
                raise ValueError(f"unmapped supported surface: {row['tool']} {row['surface']}")
            disposition = 'performance-family'
        mapped.append({'tool': row['tool'], 'surface': row['surface'], 'kind': row['kind'],
                       'status': row['status'], 'disposition': disposition, 'family': family})
    coverage = {family: set() for family in FAMILIES}
    for case in cases:
        if case['origin'] == 'historical-condition':
            # Preserve the old population as a run obligation. Its legacy label
            # does not satisfy expanded mode coverage by itself.
            continue
        coverage[case['family']].update(case['modes'])
    missing = {family: sorted(REQUIRED_MODES[family]-coverage[family])
               for family in FAMILIES if REQUIRED_MODES[family]-coverage[family]}
    unknown = {family: sorted(coverage[family]-REQUIRED_MODES[family])
               for family in FAMILIES if coverage[family]-REQUIRED_MODES[family]}
    if missing or unknown:
        raise ValueError(f'declared family obligations omitted or unknown: missing={missing}, unknown={unknown}')
    return {'schema_version': 1, 'kind': 'performance-coverage-inventory',
            'matrix_sha256': fingerprint(rows), 'cases': cases, 'matrix_mapping': mapped,
            'declared_modes': {key: sorted(value) for key, value in coverage.items()},
            'complete': False,
            'unresolved_case_ids': [case['id'] for case in cases if not case['commands']],
            'limitations': ['Family/mode declarations are not executable case coverage.',
                            'Concrete commands, inputs, equal-work validators, calibration and formal raw evidence are still required.',
                            'Historical conditions are carried forward for fresh measurement; old results are not imported as new acceptance.']}


def load_matrix(path=ROOT/'compat/matrix.tsv'):
    with path.open(newline='') as stream:
        return list(csv.DictReader(stream, delimiter='\t'))


def main():
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--matrix', type=Path, default=ROOT/'compat/matrix.tsv')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = audit(load_matrix(args.matrix))
    if args.out.exists():
        raise FileExistsError(args.out)
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(f"coverage inventory: {len(result['cases'])} case obligations; {len(result['matrix_mapping'])} matrix rows; complete=false")


if __name__ == '__main__':
    main()
