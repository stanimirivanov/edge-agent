#!/usr/bin/env python3
"""Finite nonqualifying end-to-end calibration. Never changes production deadlines."""
import argparse
import hashlib
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path
import benchmark_explanation_cpu as cpu
import run_explanation_owned as owned

KIND = "research.explanation-performance.v1"
ATTEMPT = "research.explanation-performance.attempt.v1"
CASES = ("P01", "P02", "P03")
STATUSES = {"valid", "invalid_proposal", "invalid_request", "unavailable", "timeout", "refused", "budget_exhausted", "deployment_mismatch", "invalid_response"}
STAGES = {"idle", "checking_deployment", "waiting_response", "reading_response", "decoding_response", "validating_proposal", "complete"}


class BudgetExpired(BaseException):
    """Cancellation must escape readiness/telemetry OSError recovery loops."""


def check_attempt(value, case):
    if (not isinstance(value,dict) or set(value) != {"kind","case","profile","calibration","input_bytes","total_ms","measurement"}
            or value["kind"] != ATTEMPT or value["case"] != case):
        raise owned.common.HarnessError("invalid_measurement")
    for key in ("profile","calibration"):
        if not isinstance(value[key],str) or len(value[key]) != 64 or any(c not in "0123456789abcdef" for c in value[key]):
            raise owned.common.HarnessError("invalid_measurement")
    m=value["measurement"]
    if (not isinstance(m,dict) or set(m) != {"status","stage","elapsed_ms"}
            or not isinstance(m["status"],str) or m["status"] not in STATUSES
            or not isinstance(m["stage"],str) or m["stage"] not in STAGES
            or type(value["input_bytes"]) is not int or not 1 <= value["input_bytes"] <= 65_536
            or any(type(v) is not int or not 0 <= v <= 5_400_000 for v in (value["total_ms"],m["elapsed_ms"]))
            or value["total_ms"] < m["elapsed_ms"]
            or (m["status"] == "valid" and m["elapsed_ms"] > 180_000)):
        raise owned.common.HarnessError("invalid_measurement")


def run(manifest_path,pin,report_path,execute):
    if len(pin) != 64 or any(c not in "0123456789abcdef" for c in pin):
        raise owned.common.HarnessError("invalid_manifest_pin")
    if signal.getitimer(signal.ITIMER_REAL)[0]:
        raise owned.common.HarnessError("existing_timer_refused")
    with cpu.open_report(report_path) as stream:
        started=time.monotonic()
        def emit(value):
            cpu.record(stream,dict(value,journal_ms=round((time.monotonic()-started)*1000)))
        emit({"kind":KIND,"permission_granted":False,"manifest":pin,
              "launcher":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "observation_ceiling_ms":180_000,"run_budget_ms":5_400_000,"census":[[c,r] for c in CASES for r in range(1,4)]})
        launched=False
        previous_handler=signal.getsignal(signal.SIGALRM)
        def expired(_signal,_frame):
            raise BudgetExpired()
        signal.signal(signal.SIGALRM,expired)
        signal.setitimer(signal.ITIMER_REAL,5400)
        try:
            if execute != pin:
                raise owned.common.HarnessError("execution_opt_in_required")
            owned.common.check_file({"path":str(manifest_path),"sha256":pin},32768)
            manifest=owned.common.load(manifest_path,32768)
            owned.preflight(manifest,performance=True)
            memory=Path(manifest["weights"]["path"]).stat().st_size+2*1024**3
            if not cpu.wait_idle(cpu.sample,time.monotonic,time.sleep,emit,min(started+5400,time.monotonic()+3600),memory):
                emit({"event":"complete","status":"not_run","reason":"idle_wait_expired"})
                return 2
            identities={}
            for case in CASES:
                for repetition in range(1,4):
                    emit({"event":"attempt_started","case":case,"repetition":repetition})
                    launched=True
                    attempt_start=time.monotonic()
                    previous = None
                    sampled_at = 0
                    def observe(model_pid,helper_pid):
                        nonlocal previous,sampled_at
                        now=time.monotonic()
                        if now-sampled_at < 1:
                            return
                        sampled_at=now
                        host=cpu.sample()
                        try:
                            own=sum(cpu.process_ticks(pid) for pid in (os.getpid(),model_pid,helper_pid))
                        except (OSError,ValueError,IndexError):
                            own=None
                        external=None
                        if previous is not None and own is not None and previous[1] is not None:
                            dt=host["cpu"]["cpu"][0]-previous[0]["cpu"]["cpu"][0]
                            busy=host["cpu"]["cpu"][1]-previous[0]["cpu"]["cpu"][1]
                            if dt > 0 and own >= previous[1] and busy >= 0:
                                external=max(0,busy-(own-previous[1]))/dt
                        previous=host,own
                        try:
                            hardware=cpu.hardware()
                        except (OSError,ValueError):
                            hardware=None
                        emit({"event":"load","case":case,"repetition":repetition,
                              "external_busy_fraction":external,"host":host,"hardware":hardware})
                    raw=owned.supervise(manifest_path,pin,"performance-"+case,performance_observer=observe)
                    value=json.loads(raw)
                    check_attempt(value,case)
                    identity=(value["calibration"],value["input_bytes"])
                    if (value["profile"] != manifest["profile"]["sha256"] or identities.setdefault(case,identity) != identity):
                        raise owned.common.HarnessError("measurement_identity_changed")
                    emit({"event":"attempt_finished","case":case,"repetition":repetition,
                          "measurement":value,"orchestration_ms":round((time.monotonic()-attempt_start)*1000)})
            emit({"event":"complete","status":"completed","attempts":9})
            return 0
        except BudgetExpired:
            emit({"event":"complete","status":"incomplete" if launched else "not_run","reason":"run_budget_exhausted"})
            return 2
        except (owned.common.HarnessError,OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError,KeyboardInterrupt):
            emit({"event":"complete","status":"incomplete" if launched else "not_run","reason":"probe_refused_or_interrupted"})
            return 2
        finally:
            signal.setitimer(signal.ITIMER_REAL,0)
            signal.signal(signal.SIGALRM,previous_handler)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest",type=Path,required=True)
    parser.add_argument("--manifest-sha256",required=True)
    parser.add_argument("--execute-manifest")
    parser.add_argument("--report",type=Path,required=True)
    args=parser.parse_args()
    def cancelled(_signal,_frame):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM,cancelled)
    try:
        return run(args.manifest,args.manifest_sha256,args.report,args.execute_manifest)
    except (OSError,owned.common.HarnessError):
        print("performance report refused",file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
