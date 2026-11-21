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
import profile_explanation as profiling
import math

KIND = "research.explanation-performance.v3"
ATTEMPT = "research.explanation-performance.attempt.v3"
CASES = ("P01", "P02", "P03")
STATUSES = {"valid", "invalid_proposal", "invalid_request", "unavailable", "timeout", "refused", "budget_exhausted", "deployment_mismatch", "invalid_response"}
STAGES = {"idle", "checking_deployment", "waiting_response", "reading_response", "decoding_response", "validating_proposal", "complete"}


class BudgetExpired(BaseException):
    """Cancellation must escape readiness/telemetry OSError recovery loops."""


def check_attempt(value, case, *, observation_ceiling_ms=owned.DEFAULT_OBSERVATION_SECONDS * 1000):
    if (not isinstance(value,dict) or set(value) != {"kind","case","profile","calibration","input_bytes","total_ms","measurement"}
            or value["kind"] != ATTEMPT or value["case"] != case):
        raise owned.common.HarnessError("invalid_measurement")
    for key in ("profile","calibration"):
        if not isinstance(value[key],str) or len(value[key]) != 64 or any(c not in "0123456789abcdef" for c in value[key]):
            raise owned.common.HarnessError("invalid_measurement")
    m=value["measurement"]
    if (not isinstance(m,dict) or set(m) != {"status","stage","elapsed_ms","response"}
            or not isinstance(m["status"],str) or m["status"] not in STATUSES
            or not isinstance(m["stage"],str) or m["stage"] not in STAGES
            or type(value["input_bytes"]) is not int or not 1 <= value["input_bytes"] <= 65_536
            or any(type(v) is not int or not 0 <= v <= 5_400_000 for v in (value["total_ms"],m["elapsed_ms"]))
            or value["total_ms"] < m["elapsed_ms"]
            or (m["status"] == "valid" and m["elapsed_ms"] > observation_ceiling_ms)):
        raise owned.common.HarnessError("invalid_measurement")

    response=m["response"]
    if not isinstance(response,dict) or set(response) != {"http_status","context_capacity","timings","finish_reason"}:
        raise owned.common.HarnessError("invalid_measurement")
    status=response["http_status"]
    capacity=response["context_capacity"]
    if status is not None and (type(status) is not int or not 100 <= status <= 599):
        raise owned.common.HarnessError("invalid_measurement")
    if capacity is not None and (
            not isinstance(capacity,dict) or set(capacity) != {"prompt_tokens","context_tokens"}
            or any(type(v) is not int or not 1 <= v <= 2**32-1 for v in capacity.values())
            or capacity["prompt_tokens"] <= capacity["context_tokens"]
            or status != 400 or m["status"] != "invalid_response" or m["stage"] != "reading_response"):
        raise owned.common.HarnessError("invalid_measurement")
    if m["status"] == "valid" and (status != 200 or m["stage"] != "complete"):
        raise owned.common.HarnessError("invalid_measurement")

    if response["finish_reason"] not in (None,"stop","length","content_filter","other"):
        raise owned.common.HarnessError("invalid_measurement")
    timings=response["timings"]
    if timings is not None and (not isinstance(timings,dict)
            or set(timings)!={"cache_n","prompt_n","predicted_n","prompt_ms","predicted_ms"}
            or any(type(timings[k]) is not int or not 0 <= timings[k] < 2**32 for k in ("cache_n","prompt_n","predicted_n"))
            or any(type(timings[k]) not in (int,float) or not math.isfinite(timings[k]) or not 0 <= timings[k] <= 5_400_000 for k in ("prompt_ms","predicted_ms"))):
        raise owned.common.HarnessError("invalid_measurement")


def run(manifest_path,pin,report_path,execute,*,gpu_required_free_mib=None, gpu_feasibility=False, gpu_p03_extended=False):
    if len(pin) != 64 or any(c not in "0123456789abcdef" for c in pin):
        raise owned.common.HarnessError("invalid_manifest_pin")
    if gpu_feasibility and gpu_p03_extended:
        raise owned.common.HarnessError("conflicting_feasibility_modes")
    exploratory = gpu_feasibility or gpu_p03_extended
    if signal.getitimer(signal.ITIMER_REAL)[0]:
        raise owned.common.HarnessError("existing_timer_refused")
    with cpu.open_report(report_path) as stream:
        started=time.monotonic()
        def emit(value):
            cpu.record(stream,dict(value,journal_ms=round((time.monotonic()-started)*1000)))
        census = [("P01", 1), ("P03", 1)] if gpu_feasibility else [(c, r) for c in CASES for r in range(1, 4)]
        budget = 900 if exploratory else 5400
        ceiling_ms = owned.DEFAULT_OBSERVATION_SECONDS * 1000
        kind = "research.explanation-performance.v4" if gpu_required_free_mib is not None else KIND
        if gpu_feasibility:
            kind = "research.explanation-gpu-feasibility.v1"
        if gpu_p03_extended:
            census = [("P03", 1)]
            ceiling_ms = owned.EXTENDED_P03_SECONDS * 1000
            kind = "research.explanation-gpu-p03-feasibility.v1"
        emit({"kind":kind,"permission_granted":False,"manifest":pin,
              "launcher":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "profiling":hashlib.sha256(Path(profiling.__file__).read_bytes()).hexdigest(),
              "cpu":hashlib.sha256(Path(cpu.__file__).read_bytes()).hexdigest(),
              "observation_ceiling_ms":ceiling_ms,"run_budget_ms":budget * 1000,"census":census})
        launched=False
        previous_handler=signal.getsignal(signal.SIGALRM)
        def expired(_signal,_frame):
            raise BudgetExpired()
        signal.signal(signal.SIGALRM,expired)
        signal.setitimer(signal.ITIMER_REAL,budget)
        try:
            if execute != pin:
                raise owned.common.HarnessError("execution_opt_in_required")
            owned.common.check_file({"path":str(manifest_path),"sha256":pin},32768)
            manifest=owned.common.load(manifest_path,32768)
            owned.preflight(manifest,performance=True)
            monitor=None
            if manifest.get('version')==3:
                import explanation_gpu_metrics as gpu
                monitor=gpu.Monitor(manifest['cuda'],gpu_required_free_mib)
                policy = monitor.policy()
                if exploratory:
                    policy = dict(policy, window_seconds=0)
                emit({'event':'gpu_policy',**policy})
            elif gpu_required_free_mib is not None or exploratory:
                raise owned.common.HarnessError('cuda_manifest_required')
            memory=Path(manifest["weights"]["path"]).stat().st_size+2*1024**3
            if monitor is None and not cpu.wait_idle(cpu.sample,time.monotonic,time.sleep,emit,min(started+5400,time.monotonic()+3600),memory):
                emit({"event":"complete","status":"not_run","reason":"idle_wait_expired"})
                return 2
            identities={}
            for case, repetition in census:
                if exploratory and not monitor.admission(emit):
                    emit({"event":"complete","status":"incomplete" if launched else "not_run","reason":"gpu_admission_refused"})
                    return 2
                if monitor is not None and not exploratory and not cpu.wait_idle(cpu.sample,time.monotonic,time.sleep,emit,
                        min(started+5400,time.monotonic()+3600),memory,
                        additional_idle=lambda:monitor.admission(emit)):
                    emit({'event':'complete','status':'incomplete' if launched else 'not_run','reason':'idle_wait_expired'})
                    return 2
                if exploratory:
                    emit({"event":"host_before_attempt","case":case,"host":cpu.sample()})
                emit({"event":"attempt_started","case":case,"repetition":repetition})
                launched=True
                attempt_start=time.monotonic()
                observe=profiling.Observer(lambda value:emit(dict(value,case=case,repetition=repetition)))
                gpu_observer=None
                if monitor is not None:
                    gpu_observer=gpu.Observer(monitor,lambda value:emit(dict(value,case=case,repetition=repetition)))
                def observe_both(model,helper,port):
                    observe(model,helper,port)
                    if gpu_observer is not None:gpu_observer(model,helper,port)
                mode = owned.EXTENDED_P03 if gpu_p03_extended else "performance-" + case
                raw=owned.supervise(manifest_path,pin,mode,performance_observer=observe_both)
                value=json.loads(raw)
                check_attempt(value,case,observation_ceiling_ms=ceiling_ms)
                identity=(value["calibration"],value["input_bytes"])
                if (value["profile"] != manifest["profile"]["sha256"] or identities.setdefault(case,identity) != identity):
                    raise owned.common.HarnessError("measurement_identity_changed")
                extra={} if gpu_observer is None else {'gpu_contaminated':gpu_observer.contaminated or gpu_observer.observations==0}
                emit({"event":"attempt_finished","case":case,"repetition":repetition,
                      "measurement":value,"orchestration_ms":round((time.monotonic()-attempt_start)*1000),**extra})
                if extra.get('gpu_contaminated',False):
                    emit({'event':'complete','status':'incomplete','reason':'gpu_observation_contaminated'})
                    return 2
            emit({"event":"complete","status":"completed","attempts":len(census)})
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
    parser.add_argument("--gpu-required-free-mib",type=int)
    feasibility = parser.add_mutually_exclusive_group()
    feasibility.add_argument("--gpu-feasibility",action="store_true",help="Run P01 and P03 once with a single GPU admission sample per attempt")
    feasibility.add_argument("--gpu-p03-extended", action="store_true", help="Observe one CUDA P03 request for up to ten minutes")
    args=parser.parse_args()
    def cancelled(_signal,_frame):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM,cancelled)
    try:
        return run(args.manifest,args.manifest_sha256,args.report,args.execute_manifest,gpu_required_free_mib=args.gpu_required_free_mib,gpu_feasibility=args.gpu_feasibility,gpu_p03_extended=args.gpu_p03_extended)
    except (OSError,owned.common.HarnessError):
        print("performance report refused",file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
