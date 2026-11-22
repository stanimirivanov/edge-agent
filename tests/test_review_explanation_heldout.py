"""Offline held-out review tests."""
import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from review_explanation_heldout import CASES, make_plan, score
from qualify_explanation_model import HarnessError


def fixture(target_layout="reader_first"):
    rows, attempts = [], []
    for i, case in enumerate(CASES):
        target = i < 4
        standard = f"research-explanation-runtime.v1\nVerified body {i}\n"
        opening = f"research-explanation-reader-first.v1\n\nVerified opening {i}\n"
        reader_first = opening + "\nFull validated presentation\n" + standard
        eligible = ["standard", "reader_first"] if target else ["standard"]
        row = dict(id=case, task=f"Task {i}", choice_binding=f"{i + 1:064x}",
                   standard=standard, reader_first=reader_first, eligible_layouts=eligible)
        layout = target_layout if target else "standard"
        rows.append(row)
        attempts.append(dict(kind="research.explanation-development-model-attempt.v1", case=case,
                             input=row["choice_binding"], manifest="a" * 64, profile="b" * 64,
                             status="selected", layout=layout,
                             presentation_sha256=hashlib.sha256(row[layout].encode()).hexdigest()))
    return rows, attempts


def review(plan, name):
    return {"reviewer": name, "ratings": [
        dict(id=view["id"], view=key, rationale="The opening places evidence next to the answer.",
             regression=False, preference=view["candidate"] if view["case"] in CASES[:4] else "tie")
        for key, view in plan["views"].items()
    ]}


class HeldoutReviewTest(unittest.TestCase):
    def test_packets_reverse_exact_reader_text_and_hide_instrumentation(self):
        plan, packets = make_plan(*fixture(), "a" * 64, "b" * 64)
        self.assertEqual(12, len(packets))
        self.assertEqual(12, len({packet["id"] for packet in packets}))
        for case in CASES:
            views = [p for p in packets if plan["views"][p["view"]]["case"] == case]
            self.assertEqual(len(views), 2)
            self.assertEqual(views[0]["first"], views[1]["second"])
            self.assertEqual(views[0]["second"], views[1]["first"])
            for packet in views:
                self.assertEqual(set(packet), {"id", "view", "task", "first", "second"})
                self.assertNotIn("research-explanation-reader-first.v1", packet["first"])
                self.assertNotIn("research-explanation-runtime.v1", packet["second"])

    def test_refuses_foreign_or_changed_attempts(self):
        for field, value in (("case", "U01"), ("status", "model_failed"), ("input", "c" * 64),
                             ("profile", "c" * 64), ("presentation_sha256", "c" * 64)):
            with self.subTest(field=field), self.assertRaises(HarnessError):
                rows, attempts = fixture()
                attempts[0][field] = value
                make_plan(rows, attempts, "a" * 64, "b" * 64)

    def test_scores_two_judges_separately_and_blocks_regressions(self):
        plan, _ = make_plan(*fixture(), "a" * 64, "b" * 64)
        first, second = review(plan, "one"), review(plan, "two")
        self.assertEqual(score(plan, [first, second])["status"], "advance")
        first["ratings"][0]["regression"] = True
        self.assertEqual(score(plan, [first, second])["status"], "hold")
        self.assertEqual(score(plan, [first])["status"], "incomplete")

    def test_order_disagreement_never_creates_a_preference(self):
        plan, _ = make_plan(*fixture(), "a" * 64, "b" * 64)
        first, second = review(plan, "one"), review(plan, "two")
        for case in CASES[:2]:
            rating = next(x for x in second["ratings"] if plan["views"][x["view"]]["case"] == case)
            rating["preference"] = "tie"
        result = score(plan, [first, second])
        self.assertEqual([4, 2], [row["target_preferences"] for row in result["reviews"]])
        self.assertEqual(result["status"], "hold")

    def test_equal_text_cannot_earn_target_credit_or_hide_control_bias(self):
        plan, _ = make_plan(*fixture("standard"), "a" * 64, "b" * 64)
        first, second = review(plan, "one"), review(plan, "two")
        control = next(x for x in first["ratings"] if plan["views"][x["view"]]["case"] == CASES[-1])
        control["preference"] = "first"
        result = score(plan, [first, second])
        self.assertEqual(0, result["max_target_preferences"])
        self.assertEqual([0, 0], [row["target_preferences"] for row in result["reviews"]])
        self.assertEqual(result["reviews"][0]["control_ties"], 1)
        self.assertEqual(result["status"], "hold")

    def test_rejects_duplicate_reviewer_ids_or_misbound_ratings(self):
        plan, _ = make_plan(*fixture(), "a" * 64, "b" * 64)
        first, second = review(plan, "one"), review(plan, "two")
        with self.assertRaises(HarnessError):
            score(plan, [first, first])
        second["ratings"][0]["id"] = "foreign"
        with self.assertRaises(HarnessError):
            score(plan, [first, second])
