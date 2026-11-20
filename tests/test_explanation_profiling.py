"""Numeric-only runtime profiling and one explicitly bounded comparison."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import profile_explanation as profiling
import compare_explanation_profiles as comparison


class ProfilingTests(unittest.TestCase):
    def test_slot_progress_drops_payloads_and_keeps_missing_data_unknown(self):
        slot={"id":0,"n_ctx":32768,"is_processing":True,"n_prompt_tokens":4000,
              "n_prompt_tokens_processed":2048,"n_prompt_tokens_cache":0,
              "next_token":[{"n_decoded":7,"text":"PRIVATE"}],"prompt":"PRIVATE","generated":"PRIVATE"}
        result=profiling.slot_metrics([slot])
        self.assertEqual(result,{"context_tokens":32768,"is_processing":True,"prompt_tokens":4000,
            "processed_tokens":2048,"cached_tokens":0,"decoded_tokens":7})
        self.assertNotIn("PRIVATE",json.dumps(result))
        self.assertIsNone(profiling.slot_metrics([dict(slot,id=1)]))
        self.assertIsNone(profiling.slot_metrics([slot,slot]))
        self.assertIsNone(profiling.slot_metrics([dict(slot,n_prompt_tokens=True)]))
        self.assertIsNone(profiling.slot_metrics([dict(slot,n_prompt_tokens=-1)]))
        self.assertIsNone(profiling.slot_metrics([dict(slot,n_prompt_tokens=2**32)]))
        idle=profiling.slot_metrics([{"id":0,"n_ctx":32768,"is_processing":False}])
        self.assertIsNone(idle["decoded_tokens"])

    def test_only_thread_count_can_differ(self):
        baseline={key:{"sha256":"a"*64,"path":"/invented/a"} for key in ("runtime","weights","license","helper","prompt")}
        baseline.update(threads=6,context=32768,hardware={},libraries=[])
        profile={"deployment":{"runtime":"a"*64,"weights":"b"*64},"settings":{"timeout_ms":30000}}
        candidate=copy.deepcopy(baseline);candidate["threads"]=4
        comparison.check_pair(baseline,candidate,profile,copy.deepcopy(profile))
        for field,value in (("context",16384),("threads",6),("weights",{"sha256":"c"*64,"path":"/invented/c"})):
            changed=copy.deepcopy(candidate);changed[field]=value
            with self.subTest(field=field),self.assertRaises(comparison.probe.owned.common.HarnessError):
                comparison.check_pair(baseline,changed,profile,copy.deepcopy(profile))

    def test_explicit_exploratory_pair_is_two_attempts_and_busy_default_never_launches(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);source=root/'pair.json';source.write_text('{}');pin=hashlib.sha256(source.read_bytes()).hexdigest()
            profiles=[(root/'baseline','a'*64,{'profile':{'sha256':'b'*64},'weights':{'path':str(source)}}),
                      (root/'candidate','c'*64,{'profile':{'sha256':'d'*64},'weights':{'path':str(source)}})]
            calls=[]
            def attempt(path,digest,mode,**kwargs):
                calls.append((path,mode))
                return comparison.probe.owned.common.canonical({'kind':comparison.probe.ATTEMPT,'case':'P01',
                    'profile':'b'*64 if path==profiles[0][0] else 'd'*64,'calibration':'e'*64,'input_bytes':100,
                    'total_ms':20,'measurement':{'status':'timeout','stage':'waiting_response','elapsed_ms':20,
                    'response':{'http_status':None,'context_capacity':None,'timings':None,'finish_reason':None}}})
            with patch.object(comparison,'profiles',return_value=profiles),patch.object(comparison.probe.cpu,'wait_idle',return_value=False),patch.object(comparison.probe.owned,'supervise',side_effect=attempt):
                self.assertEqual(comparison.run(source,pin,root/'busy.jsonl',pin),2)
                self.assertFalse(calls)
                self.assertEqual(comparison.run(source,pin,root/'explore.jsonl',pin,exploratory=True),0)
            self.assertEqual(calls,[(profiles[0][0],'performance-P01'),(profiles[1][0],'performance-P01')])
            rows=[json.loads(x) for x in (root/'explore.jsonl').read_text().splitlines()]
            self.assertTrue(rows[0]['exploratory']);self.assertFalse(rows[0]['permission_granted'])
            self.assertEqual(rows[-1]['attempts'],2)
            with self.assertRaises(FileExistsError):comparison.run(source,pin,root/'explore.jsonl',pin,exploratory=True)

    def test_slots_http_is_bounded_and_never_exports_provider_payloads(self):
        import socket
        import threading
        for wire in (b'HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n'+json.dumps([{'id':0,'n_ctx':32768,'is_processing':False,'prompt':'PRIVATE'}]).encode(),
                     b'HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\r\nPRIVATE'):
            with socket.socket() as listener:
                listener.bind(('127.0.0.1',0));listener.listen()
                def serve():
                    with listener.accept()[0] as connection:
                        connection.recv(4096);connection.sendall(wire)
                worker=threading.Thread(target=serve);worker.start()
                value=profiling.read_slots(listener.getsockname()[1]);worker.join(timeout=1)
                self.assertFalse(worker.is_alive());self.assertNotIn('PRIVATE',json.dumps(value))
                if b'Content-Encoding' in wire:self.assertIsNone(value)
                else:self.assertEqual(value['context_tokens'],32768)

    def test_missing_throttle_counters_are_unknown_not_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'cpu0/thermal_throttle').mkdir(parents=True)
            (root/'cpu0/thermal_throttle/core_throttle_count').write_text('7')
            self.assertEqual(profiling.throttle_counts(root),{'core_throttle_count':[7],'package_throttle_count':[None]})

    def test_comparison_refuses_missing_opt_in_and_aborts_on_first_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/'pair';path.write_text('{}');pin=hashlib.sha256(path.read_bytes()).hexdigest()
            with patch.object(comparison,'profiles',return_value=[(path,pin,{})]),patch.object(comparison.probe.owned,'supervise',side_effect=OSError('PRIVATE')) as call:
                self.assertEqual(comparison.run(path,pin,root/'dry.jsonl',None,exploratory=True),2)
                call.assert_not_called()
                self.assertEqual(comparison.run(path,pin,root/'fail.jsonl',pin,exploratory=True),2)
                self.assertEqual(call.call_count,1)
            self.assertNotIn('PRIVATE',(root/'fail.jsonl').read_text())
            self.assertEqual(json.loads((root/'fail.jsonl').read_text().splitlines()[-1])['status'],'incomplete')

    def test_budget_expiry_stops_before_the_candidate_and_clears_the_timer(self):
        import signal
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/'pair';path.write_text('{}');pin=hashlib.sha256(path.read_bytes()).hexdigest()
            with patch.object(comparison,'profiles',return_value=[(path,pin,{}),(path,pin,{})]),patch.object(comparison.probe.owned,'supervise',side_effect=comparison.probe.BudgetExpired()) as call:
                self.assertEqual(comparison.run(path,pin,root/'report',pin,exploratory=True),2)
                self.assertEqual(call.call_count,1)
            self.assertEqual(signal.getitimer(signal.ITIMER_REAL)[0],0)
            self.assertEqual(json.loads((root/'report').read_text().splitlines()[-1])['reason'],'run_budget_exhausted')

    def test_slot_byte_and_absolute_time_limits_refuse_partial_data(self):
        from unittest.mock import MagicMock
        connection=MagicMock();connection.recv.return_value=b'x'*65537
        manager=MagicMock();manager.__enter__.return_value=connection
        ready=MagicMock();ready.select.return_value=[object()]
        selector=MagicMock();selector.__enter__.return_value=ready
        with patch.object(profiling.socket,'create_connection',return_value=manager),patch.object(profiling.selectors,'DefaultSelector',return_value=selector):
            self.assertIsNone(profiling.read_slots(1234))
            connection.recv.return_value=b'partial'
            with patch.object(profiling.time,'monotonic',side_effect=[0,0.1,0.2,0.3]):
                self.assertIsNone(profiling.read_slots(1234))

    def test_budget_interrupt_during_stop_reaps_the_real_child_before_propagating(self):
        import subprocess
        process=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])
        real_wait=process.wait;interrupted=False
        def wait(*args,**kwargs):
            nonlocal interrupted
            if not interrupted:
                interrupted=True
                raise comparison.probe.BudgetExpired()
            return real_wait(*args,**kwargs)
        try:
            with patch.object(process,'wait',side_effect=wait),self.assertRaises(comparison.probe.BudgetExpired):
                comparison.probe.owned.stop(process)
            self.assertIsNotNone(process.returncode)
        finally:
            if process.poll() is None:process.kill()
            real_wait(timeout=5)
