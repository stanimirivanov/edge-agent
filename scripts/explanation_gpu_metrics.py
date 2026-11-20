"""Read-only, bounded NVIDIA observations; never model or approval authority."""
import hashlib
from pathlib import Path
import subprocess
import tempfile
import time
import benchmark_explanation_cpu as cpu
import explanation_cuda as cuda

TOOL=Path('/usr/bin/nvidia-smi')
FIELDS='uuid,utilization.gpu,utilization.memory,memory.total,memory.free'


def query(arguments):
    with tempfile.TemporaryFile() as output:
        result=subprocess.run([str(TOOL),*arguments],stdout=output,stderr=subprocess.DEVNULL,
                              timeout=0.5,env={'PATH':'/usr/bin:/bin','LANG':'C'},check=False)
        output.seek(0);raw=output.read(4097)
    if result.returncode or len(raw)>4096:raise ValueError('gpu_query_refused')
    return raw.decode('ascii')


def parse(raw,processes,uuid,owned_pid):
    rows=raw.strip().splitlines()
    if len(rows)!=1:raise ValueError('gpu_observation_shape')
    parts=[p.strip() for p in rows[0].split(',')]
    if len(parts)!=5 or parts[0]!=uuid:raise ValueError('gpu_identity_mismatch')
    def integer(text):
        if not text.isascii() or not text.isdigit() or len(text)>10:raise ValueError('gpu_unknown')
        return int(text)
    utilization,memory,total,free=map(integer,parts[1:])
    if not (0<=utilization<=100 and 0<=memory<=100 and 0<total<=1_048_576 and 0<=free<=total):
        raise ValueError('gpu_observation_range')
    lines=processes.strip().splitlines()
    if len(lines)>256:raise ValueError('gpu_process_limit')
    pids=[integer(p.strip()) for p in lines]
    if len(set(pids))!=len(pids) or any(not 0<p<2**32 for p in pids):raise ValueError('gpu_process_shape')
    return {'gpu_percent':utilization,'memory_percent':memory,'total_mib':total,'free_mib':free,
            'foreign_compute':sum(p!=owned_pid for p in pids)}


def idle(value,required):
    return (value is not None and value['gpu_percent']<=5 and value['memory_percent']<=5
            and value['foreign_compute']==0 and value['free_mib']>=required)


class Monitor:
    def __init__(self,descriptor,required):
        cuda.validate(descriptor)
        if type(required) is not int or not 1<=required<=1_048_576:raise cpu.HarnessError('gpu_headroom_required')
        self.descriptor,self.required=descriptor,required
        self.tool_pin=None

    def policy(self):
        self.tool_pin=cpu._hash_file(TOOL.resolve(strict=True),16*1024**2)
        return {'kind':'research.gpu-admission.v1','required_free_mib':self.required,
                'max_gpu_percent':5,'max_memory_percent':5,'window_seconds':300,
                'monitor_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                'tool_sha256':self.tool_pin}

    def sample(self,owned_pid=None):
        try:
            cuda.observe(self.descriptor)
            if self.tool_pin is not None and cpu._hash_file(TOOL.resolve(strict=True),16*1024**2)!=self.tool_pin:
                return None
            arguments=['--format=csv,noheader,nounits','--id='+self.descriptor['uuid']]
            return parse(query(['--query-gpu='+FIELDS,*arguments]),
                         query(['--query-compute-apps=pid',*arguments]),self.descriptor['uuid'],owned_pid)
        except (OSError,ValueError,UnicodeError,subprocess.SubprocessError,cpu.HarnessError):return None

    def admission(self,emit):
        value=self.sample();ready=idle(value,self.required)
        emit({'event':'gpu_admission','gpu':value,'idle':ready})
        return ready


class Observer:
    def __init__(self,monitor,emit):
        self.monitor,self.emit=monitor,emit
        self.sampled_at=None
        self.contaminated=False
        self.observations=0

    def __call__(self,model_pid,helper_pid,port):
        now=time.monotonic()
        if self.sampled_at is not None and now-self.sampled_at<1:return
        self.sampled_at=now
        self.observations+=1
        value=self.monitor.sample(model_pid)
        self.contaminated |= value is None or value['foreign_compute']>0
        self.emit({'event':'gpu_load','gpu':value,'contaminated':self.contaminated})
