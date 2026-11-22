"""Exercise the offline judge runner with deterministic local substitutes."""
import contextlib
from unittest.mock import patch
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from review_explanation_offline import JUDGES, calibration, execute, preflight, prepare
from review_explanation_heldout import digest, make_plan
from review_explanation_offline_runtime import Judge, command, verify_libraries
from qualify_explanation_model import HarnessError
from test_review_explanation_heldout import fixture


def artifacts():
    def file(name, sha="a" * 64):
        return {"path": f"/local/{name}", "sha256": sha}
    return {
        "runtime": file("llama-server"),
        "libraries": [file("libllama.so")],
        "judges": {name: {"weights": file(name + ".gguf", sha), "license": file(name + ".license"),
                          "template": file(name + ".jinja")} for name, sha in JUDGES.items()},
        "threads": 6, "timeout_seconds": 600, "memory_headroom_mib": 2048,
    }


def trial():
    heldout, packets = make_plan(*fixture(), "a" * 64, "b" * 64)
    heldout["packets"] = {packet["view"]: digest(packet) for packet in packets}
    return heldout, packets


class OfflineTest(unittest.TestCase):
    def test_only_exact_packages_and_both_blinded_orders_are_admitted(self):
        heldout, packets = trial()
        prepare(heldout, packets, artifacts())
        changed = dict(packets[0], first="altered")
        with self.assertRaises(HarnessError):
            prepare(heldout, [changed] + packets[1:], artifacts())
        config = artifacts()
        config["judges"]["phi4"]["weights"]["sha256"] = JUDGES["ministral"]
        with self.assertRaises(HarnessError):
            prepare(heldout, packets, config)

    def test_calibration_preserves_controls_answer_and_injection_in_both_orders(self):
        rows = calibration()
        self.assertEqual([p[1] for p in rows], ["tie", "tie", "first", "second", "first", "second"])
        for first, second in zip(rows[::2], rows[1::2], strict=True):
            self.assertEqual(first[0]["first"], second[0]["second"])
            self.assertNotIn("expected", first[0])
            self.assertNotEqual(first[0]["view"], second[0]["view"])

    def test_namespace_uses_fixed_mounts_without_host_runtime_sockets(self):
        args = command(artifacts(), Path("/local/plan.json"), "a" * 64, Path("/local/output"))
        self.assertIn("--unshare-net", args)
        self.assertIn("--clearenv", args)
        self.assertIn("/input/spent.json", args)
        self.assertNotIn("/run", args)
        self.assertNotIn("/local/output/run", args)
        self.assertIn("/models/phi4.weights", args)

    def test_rejects_a_runtime_loaded_with_an_unpinned_library(self):
        config = artifacts()
        config["libraries"][0]["path"] = "/local/libllama.so"
        maps = "1-2 r-xp 00000000 00:00 123 /runtime/libllama.so\n"
        with patch("review_explanation_offline_runtime.Path.read_text", return_value=maps):
            with patch("review_explanation_offline_runtime.check_file") as check:
                verify_libraries(config, 42)
                check.assert_called_once_with({"path": "/runtime/libllama.so", "sha256": "a" * 64}, 256 * 1024**2)
        with patch("review_explanation_offline_runtime.Path.read_text",
                   return_value=maps.replace("libllama.so", "libunknown.so")):
            with self.assertRaises(HarnessError):
                verify_libraries(config, 42)

    def test_preflight_only_measures_each_judges_largest_blinded_view(self):
        heldout, packets = trial()
        plan = prepare(heldout, packets, artifacts())
        ordered = plan["packets"]
        measured = []

        @contextlib.contextmanager
        def session(config, name):
            class Judge:
                def tokenize(self, packet):
                    count = ordered.index(packet) + 1
                    return [count] * (count if name == "ministral" else 13 - count)

                def warm(self, ids):
                    measured.append((name, ids[0]))
                    return {"fits": True, "largest_input_tokens": len(ids)}
            yield Judge()

        with tempfile.TemporaryDirectory() as directory:
            audit = Path(directory) / "fit"
            preflight(plan, audit, session)
            self.assertEqual(measured, [("ministral", 12), ("phi4", 1)])
            self.assertEqual(json.loads((audit / "outcome.json").read_text())["status"], "fit")
            self.assertEqual(json.loads((audit / "phi4.preflight.json").read_text())["largest_view"],
                             ordered[0]["view"])
            self.assertEqual({f.name for f in audit.iterdir()},
                             {"ministral.preflight.json", "phi4.preflight.json", "outcome.json"})
            with self.assertRaises(FileExistsError):
                preflight(plan, audit, session)

    def test_preflight_launch_never_mounts_or_spends_a_trial_reservation(self):
        args = command(artifacts(), Path("/local/plan.json"), "a" * 64,
                       Path("/local/output"), preflight_only=True)
        self.assertIn("_fit", args)
        self.assertNotIn("/input/spent.json", args)
        self.assertNotIn("_isolated", args)

    def test_deadline_retains_bounded_memory_measurements_as_a_refusal(self):
        child = type("Child", (), {"pid": 42})()
        judge = Judge(child, 1, artifacts())
        sample = dict(available_kib=3 * 1024**2, host_swapout_pages=12,
                      peak_rss_kib=6 * 1024**2, process_swap_kib=0)
        with patch.object(judge, "complete", side_effect=TimeoutError):
            with patch("review_explanation_offline_runtime.resource_status", return_value=sample):
                result = judge.warm([1, 2, 3])
        self.assertFalse(result["fits"])
        self.assertEqual(result["probe"], "deadline")
        self.assertEqual(result["largest_input_tokens"], 3)
        self.assertEqual(result["before"], sample)
        self.assertEqual(result["after"], sample)

    def test_swapped_process_refuses_before_prompt_evaluation(self):
        child = type("Child", (), {"pid": 42})()
        judge = Judge(child, 1, artifacts())
        sample = dict(available_kib=3 * 1024**2, host_swapout_pages=12,
                      peak_rss_kib=6 * 1024**2, process_swap_kib=16)
        with patch.object(judge, "complete") as complete:
            with patch("review_explanation_offline_runtime.resource_status", return_value=sample):
                result = judge.warm([1, 2, 3])
            complete.assert_not_called()
        self.assertFalse(result["fits"])
        self.assertEqual(result["probe"], "memory_admission")

    def exercise(self, fault=None):
        heldout, packets = trial()
        plan = prepare(heldout, packets, artifacts())
        expected = {packet["view"]: preference for packet, preference in calibration()}
        events = []
        outer = self

        @contextlib.contextmanager
        def session(config, name):
            class Judge:
                def tokenize(self, packet):
                    return [1] * (12001 if fault == ("capacity", name) else 20)

                def warm(self, tokens):
                    events.append(("warm", name))
                    return {"largest_input_tokens": len(tokens), "fits": fault != ("memory", name)}

                def judge(self, packet, tokens):
                    active = output / f'{name}-{packet["id"]}.reserved.json'
                    outer.assertTrue(active.is_file())
                    view = packet["view"]
                    events.append(("calibration" if view in expected else "graded", name))
                    choice = expected[view] if view in expected else heldout["views"][view]["candidate"]
                    if view in heldout["views"] and heldout["views"][view]["case"] in heldout["identical"]:
                        choice = "tie"
                    if fault == ("calibration", name):
                        choice = "second" if choice != "second" else "first"
                    return json.dumps(dict(id=packet["id"], view="foreign" if fault == ("graded", name)
                                           and view not in expected else view, preference=choice,
                                           rationale="The first relevant evidence is easy to find.", regression=False))
            yield Judge()

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "trial"
            if fault:
                with self.assertRaises(HarnessError):
                    execute(plan, output, session)
            else:
                execute(plan, output, session)
            outcome = json.loads((output / "outcome.json").read_text())
            reservations = list(output.glob("*.reserved.json"))
            with self.assertRaises(FileExistsError):
                execute(plan, output, session)
            return events, outcome, len(reservations)

    def test_preflight_both_then_calibrate_both_then_score(self):
        events, outcome, count = self.exercise()
        self.assertEqual([phase for phase, _ in events], ["warm"] * 2 + ["calibration"] * 12 + ["graded"] * 24)
        self.assertEqual(outcome["status"], "advance")
        self.assertEqual(count, 36)

    def test_oversize_or_calibration_failure_spends_no_graded_slots(self):
        for fault in (("capacity", "phi4"), ("memory", "phi4"), ("calibration", "phi4")):
            events, outcome, count = self.exercise(fault)
            self.assertNotIn("graded", [phase for phase, _ in events])
            self.assertEqual(outcome["status"], "incomplete")
            self.assertIn(count, (0, 7))

    def test_ambiguous_judgment_retains_spent_slot_and_incomplete_result(self):
        events, outcome, count = self.exercise(("graded", "ministral"))
        self.assertEqual(events[-1], ("graded", "ministral"))
        self.assertEqual(outcome["status"], "incomplete")
        self.assertEqual(outcome["stage"], "graded")
        self.assertEqual(count, 13)
