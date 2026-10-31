#!/usr/bin/env python3
"""Negative controls: approval must not hide unsupported or divergent behavior."""

from copy import deepcopy
import unittest

import check_compat


class ExceptionPolicyTests(unittest.TestCase):
    def setUp(self):
        self.rows = deepcopy(check_compat.read_tsv())

    def test_registered_policy_is_well_formed(self):
        self.assertEqual(check_compat.check_exceptions(self.rows), [])

    def test_unimplemented_command_cannot_be_relabelled_without_proof(self):
        row = next(row for row in self.rows if row["status"] == "unsupported" and row["kind"] == "command")
        row["status"] = "exception"
        row["deviation"] = "unimplemented"
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("lacks explicit approval" in error for error in errors), errors)

    def test_approved_difference_cannot_be_counted_as_equivalence(self):
        row = next(row for row in self.rows if row["status"] == "exception")
        row["status"] = "verified"
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("must remain visible as an exception" in error for error in errors), errors)

    def test_exception_must_state_its_difference(self):
        row = next(row for row in self.rows if row["status"] == "exception")
        row["deviation"] = ""
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("needs an explicit deviation" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
