#!/usr/bin/env python3
"""Offline blinded review of the six invented held-out development tasks."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys

from qualify_explanation_model import HarnessError, check_file, load
from review_explanation_hosted import PROMPT, canonical, decode, digest, pin, private, rating

CASES = ("H01", "H02", "H03", "H04", "H05", "H06")
TARGETS = CASES[:4]
KIND = "research.explanation-heldout-review.v1"
STANDARD_HEADER = "research-explanation-runtime.v1\n"
READER_FIRST_HEADER = "research-explanation-reader-first.v1\n\n"
DIVIDER = "\nFull validated presentation\n"


def require(condition):
    if not condition:
        raise HarnessError("heldout_review_refused")


def text_pin(text):
    return hashlib.sha256(text.encode()).hexdigest()


def make_plan(rows, attempts, manifest, profile):
    require(pin(manifest) and pin(profile))
    require([row["id"] for row in rows] == [attempt["case"] for attempt in attempts] == list(CASES))
    views, packets, identical = {}, [], []
    for row, attempt in zip(rows, attempts, strict=True):
        case = row["id"]
        eligible = ["standard", "reader_first"] if case in TARGETS else ["standard"]
        layout = attempt.get("layout")
        require(attempt.get("kind") == "research.explanation-development-model-attempt.v1"
                and row["eligible_layouts"] == eligible and layout in eligible
                and attempt.get("status") == "selected" and pin(row["choice_binding"])
                and (attempt.get("manifest"), attempt.get("profile"), attempt.get("input"))
                == (manifest, profile, row["choice_binding"]))
        standard, reader_first, task = row["standard"], row["reader_first"], row["task"]
        require(all(isinstance(s, str) and 0 < len(s) <= 65536 for s in (standard, reader_first, task))
                and standard.startswith(STANDARD_HEADER)
                and reader_first.startswith(READER_FIRST_HEADER)
                and reader_first.endswith(DIVIDER + standard)
                and attempt.get("presentation_sha256") == text_pin(row[layout]))
        baseline = standard.removeprefix(STANDARD_HEADER)
        if layout == "standard":
            candidate = baseline
            identical.append(case)
        else:
            opening = reader_first.removeprefix(READER_FIRST_HEADER).removesuffix(DIVIDER + standard)
            candidate = opening + DIVIDER + baseline
        pair = digest({"kind": KIND, "manifest": manifest, "profile": profile,
                       "fixture": row["choice_binding"], "baseline": text_pin(standard),
                       "candidate": attempt["presentation_sha256"], "rubric": PROMPT})
        for side in ("first", "second"):
            first, second = (candidate, baseline) if side == "first" else (baseline, candidate)
            blind_id = digest({"pair": pair, "order": side})[:24]
            packet = dict(id=blind_id, task=task, first=first, second=second)
            view = digest({"pair": pair, "packet": packet})
            packets.append(dict(packet, view=view))
            views[view] = dict(id=blind_id, case=case, candidate=side)
    packets.sort(key=lambda packet: packet["id"])
    return dict(kind=KIND, identical=identical, views=views), packets


def score(plan, reviews):
    require(plan["kind"] == KIND and len(reviews) <= 2)
    results = []
    for review in reviews:
        require(isinstance(review, dict) and set(review) == {"reviewer", "ratings"})
        name, ratings = review["reviewer"], review["ratings"]
        require(isinstance(name, str) and 0 < len(name) <= 128
                and all(name != result["reviewer"] for result in results)
                and isinstance(ratings, list) and len(ratings) == len(plan["views"]))
        seen, votes, regressions = set(), {case: [] for case in CASES}, set()
        for item in ratings:
            require(isinstance(item, dict)
                    and set(item) == {"id", "view", "preference", "rationale", "regression"}
                    and type(item["regression"]) is bool and isinstance(item["view"], str))
            key = item["view"]
            require(key in plan["views"] and key not in seen)
            seen.add(key)
            view = plan["views"][key]
            choice = rating(canonical({k: v for k, v in item.items() if k != "regression"}),
                            {"id": view["id"], "view": key})["preference"]
            case = view["case"]
            votes[case].append("tie" if choice == "tie" else
                               "candidate" if choice == view["candidate"] else "baseline")
            if item["regression"]:
                regressions.add(case)
        outcomes = {}
        control_ties = 0
        for case, pair in votes.items():
            require(len(pair) == 2)
            same = case in plan["identical"]
            outcomes[case] = pair[0] if pair[0] == pair[1] and not same else "tie"
            if case not in TARGETS and pair == ["tie", "tie"]:
                control_ties += 1
        results.append(dict(reviewer=name, outcomes=outcomes, control_ties=control_ties,
                            target_preferences=sum(outcomes[case] == "candidate" for case in TARGETS),
                            regressions=sorted(regressions)))
    advance = all(row["target_preferences"] >= 3 and row["control_ties"] == 2
                  and not row["regressions"] for row in results)
    status = "incomplete" if len(results) < 2 else "advance" if advance else "hold"
    return dict(kind=KIND, status=status, reviews=results,
                max_target_preferences=sum(case not in plan["identical"] for case in TARGETS))


def packetize(manifest_path, manifest_pin, attempts_path, output):
    check_file({"path": str(manifest_path), "sha256": manifest_pin}, 32768)
    manifest = load(manifest_path, 32768)
    check_file(manifest["helper"], 512 * 1024**2)
    report = subprocess.run([manifest["helper"]["path"], "compare-heldout-presentations"],
                            capture_output=True, check=True, timeout=15)
    export = decode(report.stdout)
    require(export["kind"] == "research.explanation-heldout-comparison.v1")
    attempts = [load(attempts_path / f"{case}.json", 8192) for case in CASES]
    plan, packets = make_plan(export["cases"], attempts, manifest_pin, manifest["profile"]["sha256"])
    output.mkdir(mode=0o700)
    packet_dir = output / "packets"
    packet_dir.mkdir(mode=0o700)
    plan["packets"] = {packet["view"]: digest(packet) for packet in packets}
    private(output / "plan.json", plan)
    for packet in packets:
        private(packet_dir / f'{packet["id"]}.json', packet)
    print(digest(plan))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    prepare = actions.add_parser("packetize")
    prepare.add_argument("manifest", type=Path)
    prepare.add_argument("manifest_pin")
    prepare.add_argument("attempts", type=Path)
    prepare.add_argument("output", type=Path)
    assess = actions.add_parser("score")
    assess.add_argument("plan", type=Path)
    assess.add_argument("plan_pin")
    assess.add_argument("output", type=Path)
    assess.add_argument("reviews", type=Path, nargs="*")
    args = parser.parse_args(argv)
    os.umask(0o077)
    try:
        if args.action == "packetize":
            packetize(args.manifest, args.manifest_pin, args.attempts, args.output)
        else:
            check_file({"path": str(args.plan), "sha256": args.plan_pin}, 16384)
            require(len(args.reviews) <= 2)
            private(args.output, score(load(args.plan, 16384), [load(p, 65536) for p in args.reviews]))
    except (HarnessError, OSError, subprocess.SubprocessError, KeyError, TypeError, ValueError):
        print("Held-out review refused", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
