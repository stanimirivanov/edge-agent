"""GPU admission refuses unknowns and preserves bounded numeric observations."""
import sys
from pathlib import Path
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
import explanation_gpu_metrics as gpu

CUDA={'pci':'0000:01:00.0','uuid':'GPU-11111111-1111-1111-1111-111111111111','driver':'a'*64,'layers':32,'cache':'q4_0'}

class GpuTests(unittest.TestCase):
    def test_parser_rejects_wrong_device_unknowns_duplicates_and_bounds(self):
        row=CUDA['uuid']+', 0, 5, 4096, 3700'
        value=gpu.parse(row,'',CUDA['uuid'],None)
        self.assertTrue(gpu.idle(value,3656))
        self.assertNotIn('GPU-',str(value))
        for bad in (row.replace(CUDA['uuid'],'GPU-other'),row+'\n'+row,row.replace('3700','N/A'),row.replace('3700','5000'),row.replace(', 0,',', nan,')):
            with self.subTest(bad=bad),self.assertRaises(ValueError):gpu.parse(bad,'',CUDA['uuid'],None)
        for changed in (dict(value,free_mib=3655),dict(value,gpu_percent=6),dict(value,memory_percent=6),dict(value,foreign_compute=1),None):
            self.assertFalse(gpu.idle(changed,3656))
        self.assertEqual(gpu.parse(row,'42\n43',CUDA['uuid'],42)['foreign_compute'],1)
        with self.assertRaises(ValueError):gpu.parse(row,'42\n42',CUDA['uuid'],42)

    def test_monitor_fails_closed_and_only_queries_selected_device(self):
        monitor=gpu.Monitor(CUDA,3656)
        with patch.object(gpu.cuda,'observe'),patch.object(gpu,'query',side_effect=[CUDA['uuid']+', 0, 0, 4096, 3700',''] ) as query:
            self.assertTrue(gpu.idle(monitor.sample(),3656))
            self.assertEqual(query.call_args.args[0][-1],'--id='+CUDA['uuid'])
        with patch.object(gpu.cuda,'observe',side_effect=OSError('PRIVATE')):
            self.assertIsNone(monitor.sample())
        with self.assertRaises(gpu.cpu.HarnessError):gpu.Monitor(CUDA,0)

    def test_joint_idle_window_resets_for_busy_gpu(self):
        ticks=[0];samples=[]
        def read():
            ticks[0]+=1
            return {'n':ticks[0]}
        events=[]
        readiness=iter([True,True,False,True,True,True,True])
        with patch.object(gpu.cpu,'is_idle',return_value=True):
            self.assertTrue(gpu.cpu.wait_idle(read,lambda:ticks[0],lambda _:None,events.append,20,0,window=2,additional_idle=lambda:next(readiness)))
        self.assertIn(False,[e['idle'] for e in events])
        self.assertGreaterEqual(len(events),6)

    def test_active_monitor_excludes_owned_process_and_marks_unknown_or_foreign(self):
        events=[];monitor=gpu.Monitor(CUDA,3656)
        quiet={'gpu_percent':100,'memory_percent':80,'total_mib':4096,'free_mib':100,'foreign_compute':0}
        observer=gpu.Observer(monitor,events.append)
        with patch.object(monitor,'sample',return_value=quiet) as sample:
            observer(42,99,1234);sample.assert_called_once_with(42)
        self.assertFalse(observer.contaminated)
        foreign=gpu.Observer(monitor,events.append)
        with patch.object(monitor,'sample',return_value=dict(quiet,foreign_compute=1)):
            foreign(42,99,1234)
        self.assertTrue(foreign.contaminated)
        observer.sampled_at=None
        with patch.object(monitor,'sample',return_value=None):observer(42,99,1234)
        self.assertTrue(observer.contaminated)
        self.assertNotIn('GPU-',str(events))

    def test_query_has_deadline_output_cap_and_clean_environment(self):
        import subprocess
        with patch.object(gpu.subprocess,'run',side_effect=subprocess.TimeoutExpired('tool',1)):
            with self.assertRaises(subprocess.TimeoutExpired):gpu.query(['--invented'])
        def oversized(*args,**kwargs):
            self.assertEqual(kwargs['timeout'],0.5)
            self.assertEqual(kwargs['env'],{'PATH':'/usr/bin:/bin','LANG':'C'})
            kwargs['stdout'].write(b'x'*4097)
            return subprocess.CompletedProcess(args,0)
        with patch.object(gpu.subprocess,'run',side_effect=oversized):
            with self.assertRaises(ValueError):gpu.query(['--invented'])
