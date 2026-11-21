"""Nonqualifying capacity census and reuse of owned-process cleanup."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import probe_explanation_performance as probe


class PerformanceTests(unittest.TestCase):
    def fixture(self, root):
        weights = root / "weights"
        weights.write_bytes(b"invented")
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps({"weights": {"path": str(weights)}, "profile": {"sha256": "a"*64}}))
        return manifest, hashlib.sha256(manifest.read_bytes()).hexdigest()

    def test_gpu_feasibility_is_an_explicit_two_case_run(self):
        import explanation_gpu_metrics as gpu
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, _ = self.fixture(root)
            data = json.loads(manifest.read_text())
            data.update(version=3, cuda={})
            manifest.write_text(json.dumps(data))
            pin = hashlib.sha256(manifest.read_bytes()).hexdigest()

            def attempt(path, digest, mode, **kwargs):
                kwargs["performance_observer"](42, 99, 1234)
                return probe.owned.common.canonical({
                    "kind": probe.ATTEMPT, "case": mode[-3:], "profile": "a" * 64,
                    "calibration": "1" * 64, "input_bytes": 100, "total_ms": 1,
                    "measurement": {"status": "timeout", "stage": "waiting_response", "elapsed_ms": 1,
                                    "response": {"http_status": None, "context_capacity": None,
                                                 "timings": None, "finish_reason": None}},
                })

            with patch.object(probe.owned, "preflight"), patch.object(gpu, "Monitor") as monitor, \
                 patch.object(probe.profiling, "Observer"), patch.object(probe.cpu, "wait_idle") as wait, \
                 patch.object(probe.owned, "supervise", side_effect=attempt) as launch:
                monitor.return_value.policy.return_value = {}
                monitor.return_value.sample.return_value = {"foreign_compute": 0}
                opts = {"gpu_required_free_mib": 3656, "gpu_feasibility": True}
                self.assertEqual(probe.run(manifest, pin, root / "no-consent", None, **opts), 2)
                monitor.return_value.admission.return_value = False
                self.assertEqual(probe.run(manifest, pin, root / "blocked", pin, **opts), 2)
                launch.assert_not_called()
                monitor.return_value.admission.return_value = True
                report = root / "result"
                self.assertEqual(probe.run(manifest, pin, report, pin, **opts), 0)
                self.assertEqual([call.args[2] for call in launch.call_args_list],
                                 ["performance-P01", "performance-P03"])
                self.assertEqual(monitor.return_value.admission.call_count, 3)
                wait.assert_not_called()
            rows = [json.loads(line) for line in report.read_text().splitlines()]
            self.assertEqual(rows[0]["kind"], "research.explanation-gpu-feasibility.v1")
            self.assertEqual(rows[0]["census"], [["P01", 1], ["P03", 1]])
            self.assertEqual(rows[-1]["attempts"], 2)

    def test_cuda_attempts_require_repeated_admission_and_stop_on_contamination(self):
        import explanation_gpu_metrics as gpu
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);manifest,pin=self.fixture(root)
            data=json.loads(manifest.read_text());data.update(version=3,cuda={'invented':True})
            manifest.write_text(json.dumps(data));pin=hashlib.sha256(manifest.read_bytes()).hexdigest()
            value={'kind':probe.ATTEMPT,'case':'P01','profile':'a'*64,'calibration':'1'*64,'input_bytes':100,'total_ms':1,
                   'measurement':{'status':'valid','stage':'complete','elapsed_ms':1,'response':{'http_status':200,'context_capacity':None,'timings':None,'finish_reason':'stop'}}}
            def attempt(path,pin,mode,**kwargs):
                kwargs['performance_observer'](42,99,1234)
                return probe.owned.common.canonical(dict(value,case=mode.removeprefix("performance-")))
            with patch.object(probe.owned,'preflight'),patch.object(gpu,'Monitor') as monitor, \
                 patch.object(probe.cpu,'wait_idle',return_value=True) as gate, \
                 patch.object(probe.profiling,'Observer'),patch.object(probe.owned,'supervise',side_effect=attempt) as run:
                monitor.return_value.policy.return_value={}
                monitor.return_value.sample.return_value=None
                monitor.side_effect=probe.owned.common.HarnessError('gpu_headroom_required')
                self.assertEqual(probe.run(manifest,pin,root/'missing.jsonl',pin),2)
                monitor.side_effect=None
                run.assert_not_called()
                self.assertEqual(probe.run(manifest,pin,root/'observed.jsonl',pin,gpu_required_free_mib=3656),2)
                self.assertEqual(run.call_count,1)
                self.assertIsNotNone(gate.call_args.kwargs['additional_idle'])
                run.reset_mock();gate.reset_mock()
                monitor.return_value.sample.return_value={'gpu_percent':100,'memory_percent':100,'total_mib':4096,'free_mib':100,'foreign_compute':0}
                self.assertEqual(probe.run(manifest,pin,root/'clean.jsonl',pin,gpu_required_free_mib=3656),0)
                self.assertEqual(run.call_count,9)
                self.assertEqual(gate.call_count,9)
                self.assertTrue(all(call.kwargs['additional_idle'] is not None for call in gate.call_args_list))
                run.reset_mock();gate.reset_mock();gate.side_effect=[True,False]
                self.assertEqual(probe.run(manifest,pin,root/'readmission.jsonl',pin,gpu_required_free_mib=3656),2)
                self.assertEqual(run.call_count,1)
                gate.side_effect=None
                run.side_effect=lambda *args,**kwargs:probe.owned.common.canonical(value)
                self.assertEqual(probe.run(manifest,pin,root/'unobserved.jsonl',pin,gpu_required_free_mib=3656),2)
            rows=[json.loads(line) for line in (root/'observed.jsonl').read_text().splitlines()]
            self.assertEqual(rows[0]['kind'],'research.explanation-performance.v4')
            self.assertEqual(rows[-1]['reason'],'gpu_observation_contaminated')
            self.assertTrue(next(r for r in rows if r.get('event')=='attempt_finished')['gpu_contaminated'])

    def test_ordinary_supervisor_rejects_performance_helper(self):
        manifest = {key: None for key in probe.owned.common.FIELDS}
        manifest.update(version=2, helper={"path": "/invented/probe_explanation"})
        with patch.object(probe.owned.common, "preflight"):
            with self.assertRaises(probe.owned.common.HarnessError):
                probe.owned.preflight(manifest)
            probe.owned.preflight(manifest, performance=True)

    def test_fixed_census_preserves_timeouts_and_never_exports_payloads(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, pin = self.fixture(root)
            report = root / "report.jsonl"
            calls = []
            def attempt(path, digest, mode, **kwargs):
                calls.append(mode)
                case = mode.removeprefix("performance-")
                return probe.owned.common.canonical({"kind": probe.ATTEMPT, "case": case,
                    "profile": "a"*64, "calibration": case[-1]*64, "input_bytes": 100,
                    "total_ms": 31_000, "measurement": {"status": "timeout", "stage": "waiting_response", "elapsed_ms": 30_001,"response":{"http_status":None,"context_capacity":None,"timings":None,"finish_reason":None}}})
            with patch.object(probe.owned, "preflight"), patch.object(probe.cpu, "wait_idle", return_value=True), \
                 patch.object(probe.owned, "supervise", side_effect=attempt):
                self.assertEqual(probe.run(manifest,pin,report,pin), 0)
            self.assertEqual(calls, ["performance-"+c for c in ("P01","P02","P03") for _ in range(3)])
            rows = [json.loads(line) for line in report.read_text().splitlines()]
            self.assertEqual(sum(row.get("event") == "attempt_finished" for row in rows), 9)
            self.assertEqual(rows[-1]["status"], "completed")
            self.assertIs(rows[0]["permission_granted"], False)
            self.assertEqual(report.stat().st_mode & 0o777, 0o600)
            self.assertNotIn('"proposal"',report.read_text())
            with self.assertRaises(FileExistsError):
                probe.run(manifest,pin,report,pin)

    def test_busy_host_and_missing_opt_in_never_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest,pin=self.fixture(root)
            for consent in (None,pin):
                report=root / ("dry.jsonl" if consent is None else "busy.jsonl")
                with patch.object(probe.owned,"preflight"), patch.object(probe.cpu,"wait_idle",return_value=False), \
                     patch.object(probe.owned,"supervise") as launch:
                    self.assertEqual(probe.run(manifest,pin,report,consent),2)
                    launch.assert_not_called()
                self.assertEqual(json.loads(report.read_text().splitlines()[-1])["status"],"not_run")

    def test_corrupt_or_payload_bearing_measurements_refuse_import(self):
        value={"kind":probe.ATTEMPT,"case":"P01","profile":"a"*64,"calibration":"b"*64,
               "input_bytes":10,"total_ms":100,"measurement":{"status":"valid","stage":"complete","elapsed_ms":10,"response":{"http_status":200,"context_capacity":None,"timings":None,"finish_reason":None}}}
        probe.check_attempt(value,"P01")
        for mutation in (dict(value,proposal="PRIVATE"),dict(value,kind="research.explanation-runtime.collection.v2"),
                         dict(value,case="U01"),dict(value,measurement={"status":"PRIVATE","stage":"complete","elapsed_ms":0})):
            with self.assertRaises(probe.owned.common.HarnessError):
                probe.check_attempt(mutation,"P01")

    def test_supervisor_failure_stops_census_before_any_further_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            manifest,pin=self.fixture(root)
            report=root / "report.jsonl"
            with patch.object(probe.owned,"preflight"), patch.object(probe.cpu,"wait_idle",return_value=True), \
                 patch.object(probe.owned,"supervise",side_effect=OSError("PRIVATE")) as launch:
                self.assertEqual(probe.run(manifest,pin,report,pin),2)
                self.assertEqual(launch.call_count,1)
            self.assertNotIn("PRIVATE",report.read_text())
            self.assertEqual(json.loads(report.read_text().splitlines()[-1])["status"],"incomplete")

    def test_performance_helper_failure_reaps_the_real_owned_runtime(self):
        for readiness_error in (None,probe.BudgetExpired()):
            import subprocess
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory)
                helper=root / "probe_explanation"
                helper.write_text("#!/bin/sh\nprintf '%s\\n' '{\"kind\":\"research.explanation-runtime.required.v2\"}'\ncat >/dev/null\nexit 2\n")
                helper.chmod(0o700)
                data=dict.fromkeys(probe.owned.common.FIELDS)
                data.update(version=2,helper={"path":str(helper),"sha256":hashlib.sha256(helper.read_bytes()).hexdigest()},runtime={"sha256":"a"*64},libraries=[])
                manifest=root / "manifest.json"
                manifest.write_bytes(probe.owned.common.canonical(data))
                processes=[]
                real=subprocess.Popen
                def start(*args,**kwargs):
                    process=real(*args,**kwargs)
                    processes.append(process)
                    return process
                with patch.object(probe.owned,"preflight"), patch.object(probe.owned.common,"command",return_value=[sys.executable,"-c","import time; time.sleep(30)"]), \
                     patch.object(probe.owned.common,"wait_ready",side_effect=readiness_error), patch.object(probe.owned.subprocess,"Popen",side_effect=start):
                    with self.assertRaises(type(readiness_error) if readiness_error is not None else probe.owned.common.HarnessError):
                        probe.owned.supervise(manifest,hashlib.sha256(manifest.read_bytes()).hexdigest(),"performance-P01")
                self.assertEqual(len(processes),2)
                self.assertTrue(all(p.poll() is not None for p in processes))

    def test_whole_run_budget_cannot_be_swallowed_as_a_network_error(self):
        handlers={}
        def handler(number,function):
            handlers[number]=function
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            manifest,pin=self.fixture(root)
            report=root / "report.jsonl"
            def attempt(path,digest,mode,**kwargs):
                try:
                    handlers[probe.signal.SIGALRM](probe.signal.SIGALRM,None)
                except OSError:
                    # The existing readiness loop catches network OSError: expiry
                    # must escape it rather than silently extending the census.
                    return probe.owned.common.canonical({"kind":probe.ATTEMPT,"case":mode.removeprefix("performance-"),
                        "profile":"a"*64,"calibration":"b"*64,"input_bytes":10,"total_ms":10,
                        "measurement":{"status":"valid","stage":"complete","elapsed_ms":1,"response":{"http_status":200,"context_capacity":None,"timings":None,"finish_reason":None}}})
            with patch.object(probe.signal,"signal",side_effect=handler), patch.object(probe.signal,"setitimer"), \
                 patch.object(probe.owned,"preflight"), patch.object(probe.cpu,"wait_idle",return_value=True), \
                 patch.object(probe.owned,"supervise",side_effect=attempt) as launch:
                self.assertEqual(probe.run(manifest,pin,report,pin),2)
                self.assertEqual(launch.call_count,1)
            self.assertEqual(json.loads(report.read_text().splitlines()[-1])["reason"],"run_budget_exhausted")

    def test_performance_helper_without_a_runtime_handshake_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            helper=root / "probe_explanation"
            helper.write_text("#!/bin/sh\nexit 2\n")
            helper.chmod(0o700)
            manifest=root / "manifest.json"
            value=dict.fromkeys(probe.owned.common.FIELDS)
            value.update(version=2,helper={"path":str(helper),"sha256":hashlib.sha256(helper.read_bytes()).hexdigest()})
            manifest.write_bytes(probe.owned.common.canonical(value))
            with patch.object(probe.owned,"preflight"), patch.object(probe.owned.common,"command") as command:
                with self.assertRaises(probe.owned.common.HarnessError):
                    probe.owned.supervise(manifest,hashlib.sha256(manifest.read_bytes()).hexdigest(),"performance-P01",performance_observer=lambda *_: None)
                command.assert_not_called()

    def test_v3_context_capacity_is_numeric_closed_and_not_an_authority(self):
        value={"kind":"research.explanation-performance.attempt.v3","case":"P03","profile":"a"*64,"calibration":"b"*64,
               "input_bytes":39218,"total_ms":20,"measurement":{"status":"invalid_response","stage":"reading_response","elapsed_ms":15,
               "response":{"http_status":400,"context_capacity":{"prompt_tokens":24762,"context_tokens":16384},"timings":None,"finish_reason":None}}}
        probe.check_attempt(value,"P03")
        import copy
        mutations=[("http_status",True),("http_status",700),("message","PRIVATE"),
                   ("context_capacity",{"prompt_tokens":True,"context_tokens":16384}),
                   ("context_capacity",{"prompt_tokens":16384,"context_tokens":24762}),
                   ("context_capacity",{"prompt_tokens":2**32,"context_tokens":16384}),
                   ("context_capacity",{"prompt_tokens":24762,"context_tokens":0})]
        for key,replacement in mutations:
            changed=copy.deepcopy(value)
            changed["measurement"]["response"][key]=replacement
            with self.subTest(key=key,replacement=replacement), self.assertRaises(probe.owned.common.HarnessError):
                probe.check_attempt(changed,"P03")
        changed=copy.deepcopy(value)
        changed["measurement"]["response"]["http_status"]=200
        with self.assertRaises(probe.owned.common.HarnessError):probe.check_attempt(changed,"P03")
        changed=copy.deepcopy(value)
        changed["kind"]="research.explanation-performance.attempt.v1"
        with self.assertRaises(probe.owned.common.HarnessError):probe.check_attempt(changed,"P03")
