#!/usr/bin/env python3
"""Negative controls for benchmark evidence, not implementation-mirroring tests."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import readme_benchmark as bench
import summarize_readme as summary
from readme_inputs import digest

ROOT = Path(__file__).resolve().parents[1]
SAMTOOLS = ROOT / 'tests/ci/env/bin/samtools'


class EvidenceControls(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)

    def tearDown(self):
        self.scratch.cleanup()

    def test_changed_fastq_cannot_use_stale_record_count(self):
        path = self.root / 'input.fastq'
        path.write_bytes(b'@r\nAC\n+\nII\n')
        manifest = {'datasets':[{'kind':'fastq','path':str(path),'bytes':path.stat().st_size,'sha256':digest(path)}]}
        bench.verify_inputs(manifest)
        path.write_bytes(b'@r\nAG\n+\nII\n')
        with self.assertRaisesRegex(ValueError,'input changed'):
            bench.verify_inputs(manifest)

    def test_changed_index_cannot_reuse_bam_manifest(self):
        bam = self.root / 'sorted.bam'; index = self.root / 'sorted.bam.bai'
        bam.write_bytes(b'input'); index.write_bytes(b'original index')
        manifest = {'datasets':[{'kind':'bam','unsorted':str(bam),'sha256':{p.name:digest(p) for p in (bam,index)}}]}
        bench.verify_inputs(manifest)
        index.write_bytes(b'changed index')
        with self.assertRaisesRegex(ValueError,'input changed'):
            bench.verify_inputs(manifest)

    def test_refused_rerun_preserves_previous_evidence(self):
        output = self.root / 'evidence.json'
        original = b'{"precious_previous_measurements":true}\n'
        output.write_bytes(original)
        result = subprocess.run([sys.executable,str(ROOT/'bench/readme_benchmark.py'),'--inputs',str(self.root/'inputs.json'),'--out',str(output),'--cutadapt','unused'],capture_output=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn(b'refusing to overwrite',result.stderr)
        self.assertEqual(output.read_bytes(),original)

    def test_unknown_case_cannot_silently_shrink_the_matrix(self):
        result = subprocess.run([sys.executable,str(ROOT/'bench/readme_benchmark.py'),
                                 '--inputs',str(self.root/'inputs.json'),'--out',str(self.root/'out.json'),
                                 '--cutadapt','unused','--cases','fx_fq2fa,bam_flagstats'],capture_output=True)
        self.assertEqual(result.returncode,2)
        self.assertIn(b'unknown cases',result.stderr)
        self.assertFalse((self.root/'out.json').exists())
        manifest = {'datasets':[{'id':'fq','kind':'fastq','records':1000000}]}
        with self.assertRaisesRegex(ValueError,'no applicable dataset'):
            bench.planned_conditions(manifest, set(), {'fx_fq2fa','bam_flagstat'})
        with self.assertRaisesRegex(ValueError,'duplicate dataset'):
            bench.planned_conditions({'datasets':manifest['datasets']*2}, set(), set())

    def test_pilot_cannot_be_presented_as_five_repetitions(self):
        report = {'completed_utc':'recorded','kind':'readme-pilot',
                  'parameters':{'repeats':1,'warmups':0},'results':[]}
        with self.assertRaisesRegex(ValueError,'formal tables'):
            summary.validate_report(report)
        report.update(kind='readme-benchmark',parameters={'repeats':5,'warmups':1})
        report['results']=[{'passed':True,'samples':{'rust':[{}],'upstream':[{}]},'warmups':[{}]}]
        with self.assertRaisesRegex(ValueError,'incomplete samples'):
            summary.validate_report(report)

    def test_failed_qc_warmup_still_renders_visible_failure(self):
        row = {'dataset':'failed','case':'fastqc','passed':False,
               'checks':[{'block':-1,'passed':False,'reason':'nonzero execution'}]}
        self.assertIn('no measured module comparison',summary.qc_table([row]))
        self.assertIn('incomplete',summary.table([row]))

    def test_changed_output_fails_equivalence(self):
        a = self.root/'a.tsv'; b = self.root/'b.tsv'
        a.write_text('reads\t100\n'); b.write_text('reads\t99\n')
        self.assertFalse(bench.verify('fx_stats',{}, {'rust':a,'upstream':b},{},None)['passed'])

    def test_fastqc_raw_difference_remains_visible(self):
        a = self.root/'a.txt'; b = self.root/'b.txt'
        basic = '>>Basic Statistics\tpass\n#Measure\tValue\nTotal Sequences\t2\nTotal Bases\t8 bp\nSequence length\t4\n>>END_MODULE\n'
        a.write_text(basic+'>>Adapter Content\tpass\n1\t0.0\n>>END_MODULE\n')
        b.write_text(basic+'>>Adapter Content\tfail\n1\t100.0\n>>END_MODULE\n')
        result = bench.verify('fastqc',{'records':2,'bases':8,'min_length':4,'max_length':4},{'rust':a,'upstream':b},{},None)
        self.assertTrue(result['passed'])  # explicitly scoped independent totals
        self.assertFalse(result['full_module_equality'])
        self.assertFalse(result['speed_ratio_eligible'])
        self.assertIn('Adapter Content',result['differences'])
        b.write_text(basic.replace('8 bp','7 bp'))
        self.assertFalse(bench.verify('fastqc',{'records':2,'bases':8,'min_length':4,'max_length':4},{'rust':a,'upstream':b},{},None)['passed'])

    def make_bam(self, name, records):
        sam = self.root/(name+'.sam'); bam = self.root/(name+'.bam')
        sam.write_text('@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:100\n'+records)
        subprocess.run([str(SAMTOOLS),'view','--no-PG','-b','-o',str(bam),str(sam)],check=True,capture_output=True)
        return bam

    @unittest.skipUnless(SAMTOOLS.is_file(),'pinned samtools not installed')
    def test_missing_bgzf_eof_is_rejected_even_if_records_decode(self):
        bam = self.make_bam('complete','a\t0\tchr1\t1\t60\t4M\t*\t0\t0\tACGT\tIIII\n')
        self.assertEqual(bench.bam_summary(bam,SAMTOOLS,None)['records'],1)
        bam.write_bytes(bam.read_bytes()[:-28])
        decoded = subprocess.run([str(SAMTOOLS),'view',str(bam)],capture_output=True)
        self.assertEqual(decoded.returncode,0)
        with self.assertRaises(subprocess.CalledProcessError):
            bench.bam_summary(bam,SAMTOOLS,None)

    @unittest.skipUnless(SAMTOOLS.is_file(),'pinned samtools not installed')
    def test_only_coordinate_ties_are_canonicalized(self):
        a='a\t0\tchr1\t1\t60\t4M\t*\t0\t0\tACGT\tIIII\n'
        b='b\t0\tchr1\t1\t60\t4M\t*\t0\t0\tACGT\tIIII\n'
        first=bench.bam_summary(self.make_bam('first',a+b),SAMTOOLS,None)
        second=bench.bam_summary(self.make_bam('second',b+a),SAMTOOLS,None)
        self.assertEqual(first['normalized_sha256'],second['normalized_sha256'])
        changed=bench.bam_summary(self.make_bam('changed',a+b.replace('IIII','IIIH')),SAMTOOLS,None)
        self.assertNotEqual(first['normalized_sha256'],changed['normalized_sha256'])
        with self.assertRaisesRegex(ValueError,'not coordinate sorted'):
            bench.bam_summary(self.make_bam('unsorted',b.replace('\t1\t','\t2\t')+a),SAMTOOLS,None)


if __name__=='__main__':
    unittest.main()
