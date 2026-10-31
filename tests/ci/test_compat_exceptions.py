#!/usr/bin/env python3
"""Negative controls: approval must not hide unsupported or divergent behavior."""

from copy import deepcopy
import unittest
from unittest.mock import patch
import json

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

    def test_empty_exception_rules_are_rejected(self):
        policy = json.loads((check_compat.COMPAT / "approved-exceptions.json").read_text())
        entry = deepcopy(policy["exceptions"][0])
        entry["id"] = "EMPTY-SURFACES-CONTROL"
        entry["surfaces"] = []
        policy["exceptions"].append(entry)
        with patch.object(check_compat.json, "loads", return_value=policy):
            errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("at least one surface" in error for error in errors), errors)

    def test_unfinished_work_is_not_unachievable(self):
        row = next(row for row in self.rows if row["status"] == "unsupported")
        row["unachievable"] = "not implemented yet"
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("unachievable must match" in error for error in errors), errors)

    def test_proven_bug_is_correctable_not_unachievable(self):
        row = next(row for row in self.rows if row["status"] == "exception" and row["tool"] == "rust-fx")
        row["unachievable"] = "upstream is wrong"
        self.assertTrue(check_compat.check_exceptions(self.rows))

    def test_native_constraint_cannot_be_hidden_or_replaced(self):
        for replacement in ("", "too difficult"):
            rows = deepcopy(self.rows)
            row = next(row for row in rows if row["unachievable"])
            row["unachievable"] = replacement
            self.assertTrue(check_compat.check_exceptions(rows))

    def test_native_constraint_is_not_equivalence(self):
        row = next(row for row in self.rows if row["unachievable"])
        row["status"] = "verified"
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("must remain an explicit exception" in error for error in errors), errors)

    def test_exception_must_state_its_difference(self):
        row = next(row for row in self.rows if row["status"] == "exception")
        row["deviation"] = ""
        errors = check_compat.check_exceptions(self.rows)
        self.assertTrue(any("needs an explicit deviation" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
