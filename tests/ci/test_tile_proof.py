#!/usr/bin/env python3
"""Ensure independent tile validation rejects plausible but incorrect reports."""
import importlib.util
from pathlib import Path
import unittest
from copy import deepcopy

spec = importlib.util.spec_from_file_location("tile_proof", Path(__file__).resolve().parents[1] / "oracle" / "fastqc_tile_diff.py")
proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proof)


class TileProofControls(unittest.TestCase):
    def setUp(self):
        self.expected = proof.independent([proof.record(1, 1), proof.record(2, 2)], ())
        self.correct = ("pass", ["#Tile\tBase\tMean", "1\t1\t0.0", "1\t2\tNaN", "2\t1\t0.0", "2\t2\t0.0"])

    def test_observed_only_reference_mean(self):
        proof.check_model("control", self.correct, self.expected)

    def test_missing_quality_cannot_become_zero(self):
        observed = deepcopy(self.correct)
        observed[1][2] = "1\t2\t0.0"
        with self.assertRaises(AssertionError):
            proof.check_model("imputed", observed, self.expected)

    def test_observed_value_cannot_become_missing_or_infinite(self):
        for value in ("NaN", "Infinity", "0.1"):
            observed = deepcopy(self.correct)
            observed[1][1] = "1\t1\t" + value
            with self.assertRaises(AssertionError):
                proof.check_model("wrong value", observed, self.expected)

    def test_noncanonical_missing_and_finite_values_fail(self):
        for index, value in ((2, "nan"), (2, "NAN"), (1, "0"), (1, "0e0")):
            observed = deepcopy(self.correct)
            fields = observed[1][index].split("\t")
            fields[2] = value
            observed[1][index] = "\t".join(fields)
            with self.assertRaises(AssertionError):
                proof.check_model("noncanonical", observed, self.expected)

    def test_status_is_computed_independently(self):
        with self.assertRaises(AssertionError):
            proof.check_model("wrong status", ("warn", self.correct[1]), self.expected)

    def test_missing_and_duplicate_rows_fail(self):
        for rows in (self.correct[1][:-1], self.correct[1][:-1] + [self.correct[1][1]]):
            with self.assertRaises(AssertionError):
                proof.check_model("wrong rows", ("pass", rows), self.expected)

    def test_report_must_have_complete_envelopes(self):
        report = "##FastQC\t0.13.0\n>>Per tile sequence quality\tpass\n#Tile\tBase\tMean\n>>END_MODULE\n"
        self.assertIn(proof.NAME, proof.blocks(report))
        for bad in (report.replace(">>END_MODULE\n", ""), report + report.split("\n", 1)[1], report.replace("0.13.0", "0.12.1")):
            with self.assertRaises(AssertionError):
                proof.blocks(bad)


if __name__ == "__main__":
    unittest.main()
