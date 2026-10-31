"""Invented owner test double only; no production acquisition or rights claim."""
import json
import pathlib
import sys

raw = pathlib.Path(sys.argv[1]).read_bytes()
try:
    daily = json.loads(raw)
except (ValueError, UnicodeError):
    sys.stdout.buffer.write(raw)
    sys.exit(0)
request = json.load(sys.stdin)["data"]["request"]
def ms(value):
    return value["seconds"] * 1000 + value["nanos"] // 1_000_000
value = {"kind": "research.policy-evidence.v1", "data": {
    "request": request, "daily": daily, "events": {
        "status": "synthetic_complete", "revision": "77" * 32,
        "known_at_ms": ms(daily["captured_at"]),
        "interval": [0, ms(daily["delete_by"])], "events": []}}}
sys.stdout.write(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n")
