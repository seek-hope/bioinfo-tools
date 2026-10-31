#!/usr/bin/env python3
"""Independent controls for the kmer model and its report acceptance gate."""
from copy import deepcopy
from decimal import Decimal
from pathlib import Path
import sys
import random
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "oracle"))
import fastqc_kmer_limits_diff as proof


class KmerProofControls(unittest.TestCase):
    def setUp(self):
        self.reads = ["A"*7+"C"*99]*200
        self.expected = proof.model(self.reads)
        self.actual = ("warn", [proof.HEADER] + [
            f"{'A'*(7-i)+'C'*i}\t200\t1.6384E-4\t100.0\t{i+1}" for i in range(7)])

    def test_direct_binomial_and_full_input_count(self):
        proof.check_model(self.actual, self.expected)
        self.assertEqual(proof.binomial(4, 4, Decimal("0.01")), Decimal("1e-8"))
        self.assertEqual(proof.binomial(4, 4, Decimal("0.01"), exclusive=True), 0)
        self.assertEqual(sum(s.count("AAAAAAA") for s in self.reads), 200)

    def test_one_or_two_samples_do_not_have_zero_p_values(self):
        for count in (50,100):
            self.assertEqual(proof.model(self.reads[:count]), ("pass", []))
            self.assertEqual(proof.model(self.reads[:count], buggy=True)[0], "fail")

    def test_count_scaling_tail_position_and_enrichment_mutations_fail(self):
        for field, value in ((1,"20"), (2,"0.0"), (3,"101.0"), (4,"2")):
            actual = deepcopy(self.actual)
            row = actual[1][1].split("\t")
            row[field] = value
            actual[1][1] = "\t".join(row)
            with self.assertRaises(AssertionError):
                proof.check_model(actual, self.expected)

    def test_missing_duplicate_or_reordered_rows_fail(self):
        for rows in (self.actual[1][:-1], self.actual[1]+[self.actual[1][1]],
                     self.actual[1][:1]+self.actual[1][1:][::-1]):
            with self.assertRaises(AssertionError):
                proof.check_model(("warn", rows), self.expected)

    def test_wrong_status_and_nonfinite_values_fail(self):
        with self.assertRaises(AssertionError):
            proof.check_model(("fail",self.actual[1]),self.expected)
        for value in ("NaN", "Infinity", "-Infinity"):
            rows = list(self.actual[1])
            rows[1] = rows[1].replace("1.6384E-4", value)
            with self.assertRaises(AssertionError):
                proof.check_model(("warn",rows),self.expected)

    def test_opportunity_model_is_invariant_under_sample_block_permutation(self):
        short = "C"*9+"A"*7
        long = short+"G"*90
        forward, reverse = [short]*200+[long]*200, [long]*200+[short]*200
        self.assertEqual(proof.model(forward), proof.model(reverse))
        self.assertNotEqual(proof.model(forward,buggy=True), proof.model(reverse,buggy=True))

    def test_exact_fivefold_cutoff_is_exclusive(self):
        reads = ["AAAAAAACCCC"]*900+["TTTTTTTCCCC"]*100
        self.assertEqual(proof.model(reads), ("pass", []))
        with self.assertRaises(AssertionError):
            proof.check_model(("fail", [proof.HEADER, "AAAAAAA\t900\t8.4282E-8\t5.0\t1"]), proof.model(reads))

    def test_displayed_rows_cannot_replace_full_significance(self):
        rng = random.Random(0)
        common = ("A"*7+"T"*28)*2
        rare = "".join("A"*7+"T"*6+"".join(rng.choice("ACG") for _ in range(16))+"T"*6 for _ in range(2))
        status, rows = proof.model([common]*2300+[rare]*200)
        self.assertEqual(status, "fail")
        self.assertEqual(len(rows), 20)
        self.assertTrue(all(row[2] > Decimal("0.00001") for row in rows))
        lines = [proof.HEADER] + [f"{word}\t{count}\t{float(p):.10f}\t{float(ratio):.1f}\t{position}" for word,count,p,ratio,position in rows]
        proof.check_model(("fail", lines), (status, rows))
        with self.assertRaises(AssertionError):
            proof.check_model(("warn", lines), (status, rows))

    def test_underflow_does_not_change_the_verdict(self):
        expected = proof.model([self.reads[0]]*8150, warn=320,error=323)
        self.assertEqual(expected[0], "warn")
        self.assertAlmostEqual(float(-expected[1][0][2].log10()),321.78558006070425,places=10)


if __name__ == "__main__":
    unittest.main()
