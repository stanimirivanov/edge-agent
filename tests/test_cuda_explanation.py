"""CUDA identity/configuration is separate from historical CPU manifests."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
import run_explanation_owned as owned
import explanation_cuda as cuda

class CudaTests(unittest.TestCase):
    def value(self):
        return {'pci':'0000:01:00.0','uuid':'GPU-11111111-1111-1111-1111-111111111111','driver':'a'*64,'layers':32,'cache':'q4_0'}

    def test_cpu_import_does_not_require_unbound_cuda_support(self):
        import shutil,subprocess,tempfile
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for name in ('run_explanation_owned.py','qualify_explanation_model.py','qualify_daily.py'):
                shutil.copyfile(Path(owned.__file__).parent/name,root/name)
            result=subprocess.run([sys.executable,'-I','-c',"import sys;sys.path.insert(0,sys.argv[1]);import run_explanation_owned",directory],capture_output=True)
            self.assertEqual(result.returncode,0,result.stderr)

    def test_cuda_settings_and_environment_are_closed(self):
        value=self.value();cuda.validate(value)
        for field,wrong in [('layers',0),('layers',True),('cache','shell'),('pci','../../etc'),('uuid','PRIVATE'),('driver','short')]:
            bad=dict(value);bad[field]=wrong
            with self.subTest(field=field),self.assertRaises(owned.common.HarnessError):cuda.validate(bad)
        self.assertEqual(cuda.environment(value),{'CUDA_VISIBLE_DEVICES':value['uuid'],'CUDA_DEVICE_ORDER':'PCI_BUS_ID'})
        m={'runtime':{'path':'/runtime'},'weights':{'path':'/weights','sha256':'a'*64},'threads':6,'context':32768,'version':3,'cuda':value}
        command=owned.common.command(m,1234)
        self.assertEqual(command[command.index('--device')+1],'CUDA0')
        self.assertEqual(command[command.index('--cache-type-k')+1],'q4_0')
        self.assertNotIn('none',command)

    def test_kernel_identity_drift_is_not_a_provider_claim(self):
        import hashlib
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);value=self.value();value['driver']=hashlib.sha256(b'driver').hexdigest()
            gpu=root/'gpus'/value['pci'];gpu.mkdir(parents=True)
            (gpu/'information').write_text('GPU UUID: '+value['uuid']+'\nDevice Minor: 0\n')
            (root/'version').write_bytes(b'driver')
            self.assertEqual(cuda.observe(value,root),0)
            (root/'version').write_bytes(b'changed')
            with self.assertRaises(owned.common.HarnessError):cuda.observe(value,root)

    def test_process_requires_exact_gpu_environment_and_device_fd(self):
        from unittest.mock import patch,MagicMock
        value=self.value();env=b'\0'.join((key+'='+item).encode() for key,item in cuda.environment(value).items())
        fd=MagicMock();fd.readlink.return_value=Path('/dev/nvidia0')
        with patch.object(cuda,'observe',return_value=0),patch.object(cuda,'read',return_value=env),patch.object(cuda.Path,'iterdir',return_value=[fd]):
            cuda.check_process(value,1234)
            fd.readlink.return_value=Path('/dev/null')
            with self.assertRaises(owned.common.HarnessError):cuda.check_process(value,1234)
            fd.readlink.return_value=Path('/dev/nvidia0')
            with patch.object(cuda,'read',return_value=b'CUDA_VISIBLE_DEVICES=other'):
                with self.assertRaises(owned.common.HarnessError):cuda.check_process(value,1234)

    def test_gpu_identity_binds_every_setting_and_cpu_does_not_admit_cuda(self):
        import hashlib
        from unittest.mock import patch
        m={'version':3,'runtime':{'sha256':'a'*64},'weights':{'sha256':'b'*64},'helper':{'sha256':'c'*64},'hardware':{},'libraries':[],'threads':6,'context':32768,'cuda':self.value()}
        initial=owned.runtime_id(m)
        for field,value in [('cache','q8_0'),('layers',24),('driver','d'*64),('uuid','GPU-22222222-2222-2222-2222-222222222222')]:
            changed=copy.deepcopy(m);changed['cuda'][field]=value
            self.assertNotEqual(initial,owned.runtime_id(changed))
        with patch.object(owned.common,'preflight') as preflight:
            m['version']=4
            with self.assertRaises(owned.common.HarnessError):owned.preflight(m)
            preflight.assert_not_called()
