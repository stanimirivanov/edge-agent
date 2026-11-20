"""One explicit CUDA execution contract; no downloads, tuning or CPU fallback."""
import hashlib
from pathlib import Path
import re
from qualify_daily import HarnessError


def validate(value):
    if (not isinstance(value,dict) or set(value)!={'pci','uuid','driver','layers','cache'}
            or not isinstance(value['pci'],str) or not re.fullmatch(r'[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]',value['pci'])
            or not isinstance(value['uuid'],str) or not re.fullmatch(r'GPU-[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}',value['uuid'])
            or not isinstance(value['driver'],str) or not re.fullmatch(r'[0-9a-f]{64}',value['driver'])
            or type(value['layers']) is not int or not 1 <= value['layers'] <= 64
            or value['cache'] not in ('f16','q8_0','q4_0')):
        raise HarnessError('invalid_cuda_contract')


def read(path):
    with path.open('rb') as stream:raw=stream.read(65537)
    if len(raw)>65536:raise HarnessError('cuda_observation_limit')
    return raw


def observe(value,root=Path('/proc/driver/nvidia')):
    validate(value)
    info=read(root/'gpus'/value['pci']/'information').decode()
    fields=dict(line.split(':',1) for line in info.splitlines() if ':' in line)
    if (fields.get('GPU UUID','').strip()!=value['uuid']
            or hashlib.sha256(read(root/'version')).hexdigest()!=value['driver']):
        raise HarnessError('cuda_identity_changed')
    minor=int(fields.get('Device Minor','-1').strip())
    if not 0 <= minor <= 255:raise HarnessError('cuda_device_missing')
    return minor


def environment(value):
    validate(value)
    return {'CUDA_VISIBLE_DEVICES':value['uuid'],'CUDA_DEVICE_ORDER':'PCI_BUS_ID'}


def arguments(value):
    validate(value)
    return ['--fit','off','--device','CUDA0','--n-gpu-layers',str(value['layers']),'--flash-attn','on',
            '--cache-type-k',value['cache'],'--cache-type-v',value['cache']]


def check_process(value,pid):
    minor=observe(value)
    entries=read(Path(f'/proc/{pid}/environ')).split(b'\0')
    for key,item in environment(value).items():
        matches=[v for v in entries if v.startswith((key+'=').encode())]
        if matches!=[(key+'='+item).encode()]:raise HarnessError('cuda_environment_changed')
    target=Path(f'/dev/nvidia{minor}')
    def link(path):
        try:return path.readlink()
        except OSError:return None
    if not any(link(p)==target for p in Path(f'/proc/{pid}/fd').iterdir()):
        raise HarnessError('cuda_device_not_open')
