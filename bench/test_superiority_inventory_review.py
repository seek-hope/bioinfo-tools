#!/usr/bin/env python3
"""Adversarial inventory controls prompted by read-only review."""
import copy
from pathlib import Path
import tempfile
import unittest

import superiority_inventory as inventory
from superiority_inventory import audit, bam_short, historical, planned


def row():
    return {'tool':'rust-bam','upstream':'test','surface':'sort','kind':'command',
            'status':'verified','unachievable':'','evidence':'','deviation':'','next_action':''}


def fixture(root):
    inputs={name:root/name for name in ['bam','no-eof','empty','sam','gzip','bgzf','header']}
    for path in inputs.values():path.write_bytes(path.name.encode())
    inputs['distinct']=[]
    for n in range(100):
        path=root/f'distinct-{n}';path.write_bytes(b'x');inputs['distinct'].append(path)
    exe={'current':root/'current','upstream':root/'upstream'}
    for path in exe.values():path.write_bytes(b'x')
    return inputs,exe


class InventoryReviewTests(unittest.TestCase):
    def test_custom_population_must_retain_every_historical_condition(self):
        with self.assertRaises(ValueError):audit([row()],planned())
        with self.assertRaises(ValueError):audit([row()],historical()[1:]+planned())
        cases=historical()+planned();cases[0]['modes']=['historical-default','invented']
        with self.assertRaises(ValueError):audit([row()],cases)

    def test_budget_label_must_match_internal_cpu_contract(self):
        with tempfile.TemporaryDirectory() as d:
            inputs,exe=fixture(Path(d));cases=historical()+planned()+bam_short(inputs,exe)
            case=next(c for c in cases if c['id']=='flags-numeric')
            case['commands']['current']['1']['cores']=4
            with self.assertRaises(ValueError):audit([row()],cases)

    def test_exact_equal_work_cannot_have_different_cli_work_or_count(self):
        with tempfile.TemporaryDirectory() as d:
            inputs,exe=fixture(Path(d));original=historical()+planned()+bam_short(inputs,exe)
            for change in ['arguments','count','stdin','exit']:
                cases=copy.deepcopy(original)
                case=next(c for c in cases if c['id']=='head-sam')
                a=case['commands']['current']['1'];b=case['commands']['upstream']['1']
                if change=='arguments':b['argv'][b['argv'].index('1')]='2'
                elif change=='count':b['count']+=1
                elif change=='stdin':b['stdin']=str(inputs['sam'])
                else:b['expected_exit']=1
                with self.subTest(change=change),self.assertRaises(ValueError):audit([row()],cases)

    def test_only_distinct_role_accepts_path_lists(self):
        with tempfile.TemporaryDirectory() as d:
            inputs,exe=fixture(Path(d));inputs['header']=[inputs['header']]
            with self.assertRaises(ValueError):bam_short(inputs,exe)

    def test_semantic_input_roles_cannot_alias(self):
        with tempfile.TemporaryDirectory() as d:
            original,exe=fixture(Path(d))
            for role,other in [('header','bam'),('empty','bam'),('no-eof','bam'),('sam','bam'),('gzip','bam'),('bgzf','bam')]:
                inputs=dict(original);inputs[role]=inputs[other]
                with self.subTest(role=role,other=other),self.assertRaises(ValueError):bam_short(inputs,exe)
            inputs=dict(original);inputs['empty']=inputs['distinct'][0]
            with self.assertRaises(ValueError):bam_short(inputs,exe)

    def test_planned_cases_cannot_carry_fake_evidence(self):
        cases=historical()+planned()
        target=next(c for c in cases if c['origin']=='coverage-inventory')
        for value in ({}, [], 'unexecuted claim', 0):
            target['evidence']=value
            with self.subTest(value=value),self.assertRaises(ValueError):audit([row()],cases)

    def test_required_mode_baseline_cannot_be_edited(self):
        with self.assertRaises(TypeError):inventory.REQUIRED_MODES['bam-short']=set()
        with self.assertRaises(AttributeError):inventory.REQUIRED_MODES['bam-short'].remove('flags')

    def test_flags_do_not_claim_file_input(self):
        with tempfile.TemporaryDirectory() as d:
            inputs,exe=fixture(Path(d));cases=bam_short(inputs,exe)
            for case in cases[:2]:self.assertNotIn('1-file',case['modes'])


if __name__=='__main__':unittest.main()
