"""Hosted review stays blinded, bounded and incomplete on ambiguous evidence."""
import copy
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import subprocess
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import review_explanation_hosted as review


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.calls = 0
        rows = review.TASKS.read_text().splitlines()
        self.bundle = {"kind": "research.explanation-runtime.collection.v2",
                       "permission_granted": False, "collection": {
                           "manifest": "a" * 64, "helper": "b" * 64,
                           "suite": review.SUITE, "preparation": review.PREPARATION,
                           "profile": {}, "cases": [], "safety": [
                               {"id": str(i), "candidate": {"outcome": "generated", "proposal": "invented"}} for i in range(4)]},
                       "pairs": []}
        for row in rows:
            ident, _, pin, _, _ = row.split("\t")
            self.bundle["collection"]["cases"].append({"id": ident, "input": pin,
                "candidate_first": True, "candidate": {"outcome": "generated", "proposal": "invented"}})
            self.bundle["pairs"].append({"id": ident, "presentation": review.digest(ident),
                                         "first": "Invented first text", "second": "Invented second text"})

    def plan(self):
        return review.prepare(self.bundle, {"accounts": {
            name: {"account": "opaque-" + name, "handling": "reviewed invented-only handling",
                   "pricing": "owner quote", "input_rate": rates[0], "output_rate": rates[1]}
            for name, rates in {"openai": (2500000, 15000000), "anthropic": (4000000, 20000000)}.items()},
            "ceiling_micro_usd": 20000000, "overhead_micro_usd": 0})

    def test_plan_freezes_complete_views_without_sending_private_metadata(self):
        plan = self.plan()
        self.assertEqual(len(plan["views"]), 66)
        self.assertEqual(plan["bound_micro_usd"], 14609760)
        view = plan["views"][6]
        self.assertEqual(set(view["packet"]), {"id", "view", "task", "first", "second"})
        self.assertEqual(view["packet"]["first"], plan["views"][7]["packet"]["second"])
        for provider in review.PROVIDERS:
            body = review.request(provider, view["packet"])
            self.assertEqual(body.get("service_tier"), "default" if provider == "openai" else "standard_only")
            wire = json.dumps(body)
            for forbidden in ("candidate_first", "opaque-", "owner quote", "manifest", "baseline"):
                self.assertNotIn(forbidden, wire)
            self.assertNotEqual(view["packet"]["view"], plan["views"][7]["packet"]["view"])
        changed = copy.deepcopy(self.bundle)
        changed["pairs"][0]["first"] += " changed"
        self.assertNotEqual(review.prepare(changed, plan["config"])["views"][6]["packet"]["view"], view["packet"]["view"])

    def test_missing_duplicate_foreign_or_reordered_census_is_refused(self):
        for mutate in (lambda b: b["pairs"].pop(),
                       lambda b: b["pairs"].__setitem__(1, b["pairs"][0]),
                       lambda b: b["collection"].__setitem__("suite", "c" * 64),
                       lambda b: b["collection"]["cases"].reverse(),
                       lambda b: b["pairs"][0].__setitem__("candidate_first", True)):
            original = copy.deepcopy(self.bundle)
            mutate(self.bundle)
            with self.subTest(mutate=mutate), self.assertRaises(review.HarnessError):
                self.plan()
            self.bundle = original

    def test_failed_probe_or_impossible_preference_total_refuses_before_judging(self):
        original = copy.deepcopy(self.bundle)
        self.bundle["collection"]["safety"][0]["candidate"] = {"outcome": "failed"}
        with self.assertRaises(review.HarnessError):
            self.plan()
        self.bundle = original
        for case in self.bundle["collection"]["cases"][:10]:
            case["candidate"] = {"outcome": "failed"}
        with self.assertRaises(review.HarnessError):
            self.plan()

    def test_reply_identity_framing_enum_and_rationale_are_independently_checked(self):
        packet = self.plan()["views"][6]["packet"]
        good = dict(id=packet["id"], view=packet["view"], preference="first", rationale="Clearer.")
        self.assertEqual(review.rating(json.dumps(good), packet), good)
        invalid = [dict(good, view="c" * 64), dict(good, extra=True), dict(good, preference="pass"),
                   dict(good, rationale="word " * 121), dict(good, rationale=3), dict(good, rationale="   ")]
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(review.HarnessError):
                review.rating(json.dumps(value), packet)
        for raw in (json.dumps(good) + " {}", '{"id":"U01","id":"U02"}', "NaN"):
            with self.subTest(raw=raw), self.assertRaises(review.HarnessError):
                review.rating(raw, packet)

    def test_normalization_needs_both_votes_and_never_selects_favorable_order(self):
        for original, swapped, expected in (("first", "second", "first"),
                ("second", "first", "second"), ("first", "first", "tie"),
                ("tie", "second", "tie"), ("tie", "tie", "tie")):
            self.assertEqual(review.normalize(original, swapped), expected)
        with self.assertRaises(review.HarnessError):
            review.normalize("first", None)

    def response(self, provider, packet, preference):
        text = json.dumps(dict(id=packet["id"], view=packet["view"], preference=preference, rationale="Clearer."))
        usage = {"input_tokens": 500, "output_tokens": 100}
        if provider == "openai":
            return dict(id="resp_test", object="response", service_tier="default", model=review.PROVIDERS[provider][1], status="completed", usage=usage,
                        output=[{"type": "message", "status": "completed", "role": "assistant",
                                 "content": [{"type": "output_text", "text": text}]}])
        usage["service_tier"] = "standard"
        return dict(id="msg_test", type="message", role="assistant", model=review.PROVIDERS[provider][1], stop_reason="end_turn", usage=usage,
                    content=[{"type": "thinking", "thinking": "MUST NOT PERSIST"}, {"type": "text", "text": text}])

    def transport(self, provider, body, counting):
        self.calls += 1
        if counting:
            return {"input_tokens": 500}, f"req_count_{self.calls}"
        packet = json.loads(body["input"] if provider == "openai" else body["messages"][0]["content"])
        preference = "tie" if packet["id"] == "C01" else "first"
        plan = self.plan()
        view = next(v for v in plan["views"] if v["packet"]["view"] == packet["view"])
        if view["swapped"] and preference != "tie":
            preference = "second"
        value = self.response(provider, packet, preference)
        value["id"] = f"response_{self.calls}"
        return value, f"req_{self.calls}"

    def test_complete_trial_retains_bound_reviews_and_discards_hidden_thinking(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "trial"
            review.execute(self.plan(), output, self.transport)
            reviews = json.loads((output / "reviews.json").read_bytes())
            self.assertEqual([len(r["ratings"]) for r in reviews], [30, 30])
            self.assertEqual(set(reviews[0]["ratings"][0]), {"id", "presentation", "preference"})
            self.assertEqual(reviews[0]["ratings"][0]["preference"], "first")
            self.assertEqual(len(list(output.glob("*.reserved.json"))), 132)
            for path in output.iterdir():
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertNotIn(b"MUST NOT PERSIST", path.read_bytes())
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
            with self.assertRaises(OSError):
                review.execute(self.plan(), output, self.transport)

    def test_no_paid_dispatch_on_oversized_input_or_failed_calibration(self):
        for failure in ("input", "calibration", "ambiguous"):
            paid = []
            with tempfile.TemporaryDirectory() as temp:
                output = Path(temp) / "trial"
                def transport(provider, body, counting):
                    if failure == "input" and counting:
                        return {"input_tokens": 12001}, "req_count"
                    if not counting:
                        self.assertEqual(len(list(output.glob("*.reserved.json"))), len(paid) + 1)
                        paid.append(provider)
                        if failure == "ambiguous":
                            raise TimeoutError("PRIVATE RAW RESPONSE AND API KEY")
                        value, ident = self.transport(provider, body, counting)
                        packet = self.plan()["views"][0]["packet"]
                        return self.response(provider, packet, "first"), ident
                    return self.transport(provider, body, counting)
                with self.assertRaises((review.HarnessError, TimeoutError)):
                    review.execute(self.plan(), output, transport)
                self.assertLessEqual(len(paid), 1)
                self.assertFalse((output / "reviews.json").exists())
                self.assertEqual(json.loads((output / "outcome.json").read_bytes())["status"], "incomplete")
                for path in output.iterdir():
                    self.assertNotIn(b"PRIVATE RAW", path.read_bytes())

    def test_provider_model_stop_usage_and_unknown_content_fail_closed(self):
        packet = self.plan()["views"][6]["packet"]
        for provider in review.PROVIDERS:
            good = self.response(provider, packet, "first")
            for key, value in (("model", "moving-alias"), ("id", ""), ("usage", {"input_tokens": 12001, "output_tokens": 100}),
                               ("usage", {"input_tokens": 500, "output_tokens": 4097}),
                               ("object" if provider == "openai" else "role", "foreign"),
                               ("status" if provider == "openai" else "stop_reason", "incomplete")):
                bad = dict(good, **{key: value})
                with self.subTest(provider=provider, key=key), self.assertRaises(review.HarnessError):
                    review.accept(provider, bad, packet)
            bad = copy.deepcopy(good)
            bad["output" if provider == "openai" else "content"].append({"type": "tool_use"})
            with self.assertRaises(review.HarnessError):
                review.accept(provider, bad, packet)
            bad = copy.deepcopy(good)
            (bad if provider == "openai" else bad["usage"])["service_tier"] = "priority"
            with self.assertRaises(review.HarnessError):
                review.accept(provider, bad, packet)

    def test_reused_provider_response_cannot_complete_a_trial(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "trial"
            def duplicated(provider, body, counting):
                value, ident = self.transport(provider, body, counting)
                if not counting:
                    value["id"] = "duplicate"
                return value, ident
            with self.assertRaises(review.HarnessError):
                review.execute(self.plan(), output, duplicated)
            self.assertFalse((output / "reviews.json").exists())

    def test_offline_plan_and_mismatched_execution_never_read_keys_or_dispatch(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            collection, accounts, plan = (root / n for n in ("collection.json", "accounts.json", "plan.json"))
            collection.write_bytes(review.canonical(self.bundle))
            accounts.write_bytes(review.canonical(self.plan()["config"]))
            args = ["--collection", str(collection), "--accounts", str(accounts), "--plan", str(plan)]
            args += ["--verifier", sys.executable, "--verifier-sha256", "a" * 64]
            with patch.object(review, "post", side_effect=AssertionError("unexpected dispatch")), \
                    patch.object(review, "verify_pairs", return_value={"sha256": "a" * 64}), \
                    patch.dict(review.os.environ, {}, clear=True):
                self.assertEqual(review.main(args), 0)
                original = plan.read_bytes()
                self.assertEqual(review.main(args + ["--execute-plan", "f" * 64, "--audit-dir", str(root / "audit")]), 2)
                self.assertEqual(plan.read_bytes(), original)
                self.assertFalse((root / "audit").exists())
                plan.write_bytes(original + b" ")
                self.assertEqual(review.main(args), 2)

    def test_unaffordable_or_unreviewed_account_configuration_is_refused(self):
        for mutate in (lambda c: c.__setitem__("ceiling_micro_usd", 1),
                       lambda c: c["accounts"]["openai"].__setitem__("handling", ""),
                       lambda c: c["accounts"]["anthropic"].__setitem__("input_rate", True)):
            config = self.plan()["config"]
            mutate(config)
            with self.assertRaises(review.HarnessError):
                review.prepare(self.bundle, config)

    def test_provider_transport_refuses_redirects_and_bounds_request_and_response(self):
        class Response:
            status = 200
            def getheader(self, name):
                return "req_http"
            def read(self, size):
                self.limit = size
                return b'{"input_tokens":500}'
        class Connection:
            def __init__(self, host, **kwargs):
                self.host, self.options = host, kwargs
                self.response, self.closed = Response(), False
            def request(self, method, path, body, headers):
                self.sent = method, path, body, headers
            def getresponse(self):
                return self.response
            def close(self):
                self.closed = True
        packet = self.plan()["views"][6]["packet"]
        for provider in review.PROVIDERS:
            for counting in (False, True):
                body = review.request(provider, packet)
                connection = Connection(review.PROVIDERS[provider][0], timeout=120)
                stdout = type("Output", (), {"buffer": io.BytesIO()})()
                stdin = type("Input", (), {"buffer": io.BytesIO(review.canonical(body))})()
                with patch("http.client.HTTPSConnection", return_value=connection) as factory, \
                        patch.object(review.sys, "stdin", stdin), patch.object(review.sys, "stdout", stdout), \
                        patch.dict(review.os.environ, {review.PROVIDERS[provider][2]: "invented-key"}):
                    review.post_child(provider, counting)
                    self.assertEqual(factory.call_args.args, (review.PROVIDERS[provider][0],))
                    self.assertEqual(factory.call_args.kwargs["timeout"], 120)
                    self.assertEqual(connection.sent[0], "POST")
                    self.assertEqual(connection.response.limit, 1024 * 1024 + 1)
                    self.assertTrue(connection.closed)
                    self.assertNotIn(b"invented-key", stdout.buffer.getvalue())
                    self.assertTrue(connection.sent[1].endswith("count_tokens" if provider == "anthropic" and counting
                                    else "input_tokens" if counting else "messages" if provider == "anthropic" else "responses"))
                    connection.response.status = 302
                    stdin.buffer.seek(0)
                    with self.assertRaises(review.HarnessError):
                        review.post_child(provider, counting)
                    connection.response.status = 200
                    connection.response.read = lambda _: b"x" * (1024 * 1024 + 1)
                    stdin.buffer.seek(0)
                    with self.assertRaises(review.HarnessError):
                        review.post_child(provider, counting)

    def test_entire_network_call_has_one_hard_deadline_and_no_retry(self):
        body = review.request("openai", self.plan()["views"][6]["packet"])
        with patch.dict(review.os.environ, {"OPENAI_API_KEY": "invented-key"}), \
                patch.object(review.subprocess, "run", side_effect=subprocess.TimeoutExpired("private", 120)) as child:
            with self.assertRaises(subprocess.TimeoutExpired):
                review.post("openai", body, False)
            self.assertEqual(child.call_count, 1)
            self.assertEqual(child.call_args.kwargs["timeout"], 120)
            self.assertNotIn("invented-key", str(child.call_args.args))

    def test_accepted_plan_is_spent_once_even_with_a_different_audit_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            collection, accounts, plan = (root / n for n in ("collection.json", "accounts.json", "plan.json"))
            collection.write_bytes(review.canonical(self.bundle))
            accounts.write_bytes(review.canonical(self.plan()["config"]))
            args = ["--collection", str(collection), "--accounts", str(accounts), "--plan", str(plan)]
            args += ["--verifier", sys.executable, "--verifier-sha256", "a" * 64]
            verifier = patch.object(review, "verify_pairs", return_value={"sha256": "a" * 64})
            verifier.start()
            self.addCleanup(verifier.stop)
            self.assertEqual(review.main(args), 0)
            accepted = hashlib.sha256(plan.read_bytes()).hexdigest()
            with patch.dict(review.os.environ, {"OPENAI_API_KEY": "invented", "ANTHROPIC_API_KEY": "invented"}), \
                    patch.object(review, "post", side_effect=self.transport):
                self.assertEqual(review.main(args + ["--execute-plan", accepted, "--audit-dir", str(root / "one")]), 0)
                calls = self.calls
                self.assertEqual(review.main(args + ["--execute-plan", accepted, "--audit-dir", str(root / "two")]), 2)
                self.assertEqual(self.calls, calls)
                self.assertFalse((root / "two").exists())

    def test_pair_reconstruction_checks_binary_pin_and_rejects_tampered_text(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "verifier"
            path.write_bytes(b"invented executable")
            pin = hashlib.sha256(path.read_bytes()).hexdigest()
            expected = copy.deepcopy(self.bundle["pairs"])
            def process(argv, **kwargs):
                self.assertEqual(argv, [str(path), "verify-runtime"])
                self.assertEqual(kwargs["env"], {"PATH": "/usr/bin:/bin", "LANG": "C"})
                kwargs["stdout"].write(review.canonical({"pairs": expected}))
                return subprocess.CompletedProcess(argv, 0)
            with patch.object(review.subprocess, "run", side_effect=process) as child:
                self.assertEqual(review.verify_pairs(self.bundle, path, pin)["sha256"], pin)
                self.bundle["pairs"][0]["first"] = "different text under the old presentation"
                with self.assertRaises(review.HarnessError):
                    review.verify_pairs(self.bundle, path, pin)
                calls = child.call_count
                with self.assertRaises(review.HarnessError):
                    review.verify_pairs(self.bundle, path, "f" * 64)
                self.assertEqual(child.call_count, calls)

    def test_interrupted_counting_retains_each_completed_preflight_response(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "trial"
            calls = 0
            def counting(provider, body, is_count):
                nonlocal calls
                self.assertTrue(is_count)
                calls += 1
                if calls == 4:
                    raise TimeoutError("ambiguous preflight")
                return {"input_tokens": 500}, f"req_{calls}"
            with self.assertRaises(TimeoutError):
                review.execute(self.plan(), output, counting)
            self.assertEqual(len(list(output.glob("*.count.json"))), 3)
            self.assertFalse((output / "reviews.json").exists())


if __name__ == "__main__":
    unittest.main()
