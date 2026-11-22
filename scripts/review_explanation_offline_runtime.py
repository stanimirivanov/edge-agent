"""Owned loopback-only llama.cpp transport for the offline held-out reviewer."""
from contextlib import contextmanager
import hashlib
import http.client
import json
from pathlib import Path
import re
import signal
import socket
import subprocess
import time

from qualify_explanation_model import HarnessError, check_file
from review_explanation_hosted import PROMPT, canonical, decode

CONTEXT = 16384
INPUT_LIMIT = 12000
OUTPUT_LIMIT = 512
ENV = {"PATH": "/usr/bin", "LANG": "C", "LD_LIBRARY_PATH": "/runtime"}


def require(ok):
    if not ok:
        raise HarnessError("offline_runtime_refused")


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def runtime_files(config):
    yield config["runtime"], "/runtime/llama-server", 256 * 1024**2
    for item in config["libraries"]:
        yield item, f'/runtime/{Path(item["path"]).name}', 256 * 1024**2


def judge_files(config, name):
    for role, limit in (("weights", 16 * 1024**3), ("license", 128 * 1024), ("template", 128 * 1024)):
        yield config["judges"][name][role], f"/models/{name}.{role}", limit


def command(config, plan, pin, output, *, preflight_only=False):
    args = ["/usr/bin/bwrap", "--unshare-net", "--unshare-pid", "--die-with-parent",
            "--new-session", "--clearenv", "--cap-drop", "ALL", "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
            "--ro-bind", str(Path(__file__).resolve().parent), "/app",
            "--ro-bind", str(plan.resolve()), "/input/plan.json",
            "--bind", str(output.resolve()), "/audit"]
    if not preflight_only:
        args.extend(("--ro-bind", str(plan.resolve().with_name(f"{pin}.spent")), "/input/spent.json"))
    for item, target, _ in (*runtime_files(config), *(pin for name in config["judges"] for pin in judge_files(config, name))):
        args.extend(("--ro-bind", item["path"], target))
    action = "_fit" if preflight_only else "_isolated"
    return args + ["/usr/bin/python3", "-B", "/app/review_explanation_offline.py", action,
                   "--plan", "/input/plan.json", "--plan-sha256", pin, "--output", "/audit/run"]


@contextmanager
def deadline(seconds):
    def expired(_signal, _frame):
        raise TimeoutError("offline_runtime_deadline")
    old = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, old)


def grammar(packet):
    def literal(value):
        return json.dumps(json.dumps(value))
    quote = json.dumps('"')
    return fr'''root ::= "{{" ws {literal("id")} ws ":" ws {literal(packet["id"])} ws "," ws {literal("view")} ws ":" ws {literal(packet["view"])} ws "," ws {literal("preference")} ws ":" ws preference ws "," ws {literal("rationale")} ws ":" ws rationale ws "," ws {literal("regression")} ws ":" ws boolean ws "}}" ws
preference ::= {literal("first")} | {literal("second")} | {literal("tie")}
rationale ::= {quote} [^"\\\x00-\x1f]+ {quote}
boolean ::= "true" | "false"
ws ::= [ \t\r\n]*
'''


def verify_libraries(config, pid):
    pins = {Path(item["path"]).name: item["sha256"] for item in config["libraries"]}
    mappings = Path(f"/proc/{pid}/maps").read_text().splitlines()
    paths = {line.split(maxsplit=5)[-1] for line in mappings}
    libraries = {p for p in paths if p.startswith("/") and ".so" in Path(p).name}
    require(libraries)
    for path in sorted(libraries):
        require(Path(path).name in pins)
        check_file({"path": path, "sha256": pins[Path(path).name]}, 256 * 1024**2)


def resource_status(pid):
    mem = re.search(r"^MemAvailable:\s+(\d+) kB$", Path("/proc/meminfo").read_text(), re.M)
    swap = re.search(r"^pswpout (\d+)$", Path("/proc/vmstat").read_text(), re.M)
    proc = Path(f"/proc/{pid}/status").read_text()
    peak = re.search(r"^VmHWM:\s+(\d+) kB$", proc, re.M)
    process_swap = re.search(r"^VmSwap:\s+(\d+) kB$", proc, re.M)
    require(all((mem, swap, peak, process_swap)))
    return dict(available_kib=int(mem[1]), host_swapout_pages=int(swap[1]),
                peak_rss_kib=int(peak[1]), process_swap_kib=int(process_swap[1]))


class Judge:
    def __init__(self, child, port, config):
        self.child, self.port, self.config = child, port, config

    def request(self, endpoint, body=None, timeout=None):
        seconds = timeout or self.config["timeout_seconds"]
        require(self.child.poll() is None)
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=seconds)
        try:
            with deadline(seconds):
                connection.request("GET" if body is None else "POST", endpoint,
                                   None if body is None else canonical(body), {"Content-Type": "application/json"})
                response = connection.getresponse()
                require(response.status == 200)
                raw = response.read(512 * 1024 + 1)
                require(len(raw) <= 512 * 1024)
                return decode(raw)
        except http.client.HTTPException:
            raise HarnessError("offline_runtime_http") from None
        finally:
            connection.close()

    def tokenize(self, packet):
        # One user message works with both publisher chat templates.
        prompt = self.request("/apply-template", {"messages": [
            {"role": "user", "content": PROMPT + "\n" + self.config["diagnostic"] + "\n" + canonical(packet).decode()}
        ]})["prompt"]
        tokens = self.request("/tokenize", {"content": prompt, "add_special": True, "parse_special": True})["tokens"]
        require(isinstance(tokens, list) and all(type(t) is int and t >= 0 for t in tokens))
        return tokens

    def complete(self, tokens, limit, rules):
        value = self.request("/completion", dict(prompt=tokens, n_predict=limit, temperature=0, seed=42,
                                                   cache_prompt=False, stream=False, grammar=rules))
        require(value.get("truncated") is False and value.get("tokens_evaluated") == len(tokens)
                and isinstance(value.get("content"), str))
        return value

    def warm(self, tokens):
        def admitted(sample):
            return (sample["process_swap_kib"] == 0
                    and sample["available_kib"] >= self.config["memory_headroom_mib"] * 1024)

        before = resource_status(self.child.pid)
        started = time.monotonic()
        after = before
        probe = "memory_admission"
        if admitted(before):
            try:
                result = self.complete(tokens, 1, 'root ::= "0"')
                probe = "complete" if result["content"] == "0" else "unexpected_output"
            except TimeoutError:
                probe = "deadline"
            after = resource_status(self.child.pid)
        fits = (probe == "complete" and admitted(after)
                and after["host_swapout_pages"] == before["host_swapout_pages"])
        return {"before": before, "after": after, "largest_input_tokens": len(tokens), "fits": fits,
                "probe": probe, "elapsed_ms": round((time.monotonic() - started) * 1000)}

    def judge(self, packet, tokens):
        value = self.complete(tokens, OUTPUT_LIMIT, grammar(packet))
        require(value["stop_type"] == "eos" and len(value["content"]) <= 8192)
        return value["content"]


@contextmanager
def session(config, name, diagnostic):
    require(socket.if_nameindex() == [(1, "lo")])
    for item, target, limit in (*runtime_files(config), *judge_files(config, name)):
        check_file({"path": target, "sha256": item["sha256"]}, limit)
    executable = "/runtime/llama-server"
    version = subprocess.run([executable, "--version"], env=ENV, capture_output=True, check=True, timeout=10)
    require(b"(build 12375, commit 9d25e0014)" in version.stdout + version.stderr)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    argv = [executable, "--offline", "--host", "127.0.0.1", "--port", str(port), "--parallel", "1",
            "--no-webui", "--n-gpu-layers", "0", "--ctx-size", str(CONTEXT), "--no-context-shift",
            "--threads", str(config["threads"]), "--model", f"/models/{name}.weights",
            "--jinja", "--chat-template-file", f"/models/{name}.template"]
    with subprocess.Popen(argv, env=ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as child:
        try:
            judge = Judge(child, port, dict(config, diagnostic=diagnostic))
            end = time.monotonic() + min(120, config["timeout_seconds"])
            while child.poll() is None and time.monotonic() < end:
                try:
                    if judge.request("/health", timeout=1).get("status") == "ok":
                        break
                except (OSError, HarnessError):
                    pass
                time.sleep(0.2)
            else:
                raise HarnessError("offline_runtime_startup")
            verify_libraries(config, child.pid)
            yield judge
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
