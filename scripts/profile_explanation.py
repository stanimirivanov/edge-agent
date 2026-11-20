"""Bounded, numeric-only observations of an already owned local runtime."""
import json
import os
from pathlib import Path
import selectors
import socket
import time
import benchmark_explanation_cpu as cpu


def slot_metrics(value):
    try:
        if not isinstance(value,list) or len(value)!=1:
            return None
        slot=value[0]
        if type(slot['id']) is not int or slot['id']!=0 or type(slot['is_processing']) is not bool:
            return None
        def count(v):
            if v is not None and (type(v) is not int or not 0 <= v < 2**32):
                raise ValueError()
            return v
        context=count(slot['n_ctx'])
        if context is None or not 1 <= context <= 32768:
            return None
        token=slot.get('next_token',{})
        if isinstance(token,list):
            if len(token)!=1:return None
            token=token[0]
        return {'context_tokens':context,'is_processing':slot['is_processing'],
                'prompt_tokens':count(slot.get('n_prompt_tokens')),
                'processed_tokens':count(slot.get('n_prompt_tokens_processed')),
                'cached_tokens':count(slot.get('n_prompt_tokens_cache')),
                'decoded_tokens':count(token.get('n_decoded'))}
    except (KeyError,TypeError,ValueError,AttributeError):
        return None


def read_slots(port):
    # Absolute 250 ms budget, including slow headers/body; no redirects/proxy or
    # per-byte timeout resets. Unknown/oversized/encoded replies stay unknown.
    deadline=time.monotonic()+0.25
    try:
        with socket.create_connection(('127.0.0.1',port),timeout=0.25) as connection:
            connection.setblocking(False)
            connection.sendall(b'GET /slots HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n')
            raw=bytearray()
            with selectors.DefaultSelector() as ready:
                ready.register(connection,selectors.EVENT_READ)
                while time.monotonic()<deadline:
                    if not ready.select(max(0,deadline-time.monotonic())):return None
                    part=connection.recv(4096)
                    if not part:break
                    raw.extend(part)
                    if len(raw)>65536:return None
                else:return None
        head,separator,body=bytes(raw).partition(b'\r\n\r\n')
        lines=head.split(b'\r\n')
        if not separator or lines[0].split()[:2] not in ([b'HTTP/1.0',b'200'],[b'HTTP/1.1',b'200']):return None
        headers=dict(line.lower().split(b':',1) for line in lines[1:])
        if (b'content-encoding' in headers or b'transfer-encoding' in headers
                or headers.get(b'content-type',b'').strip().split(b';')[0]!=b'application/json'):
            return None
        if b'content-length' in headers and int(headers[b'content-length'])!=len(body):return None
        return slot_metrics(json.loads(body))
    except (OSError,ValueError,TypeError):
        return None


def number(path):
    try:
        value=int(path.read_text().strip())
        return value if 0 <= value < 2**64 else None
    except (OSError,ValueError):return None


def throttle_counts(root=Path('/sys/devices/system/cpu')):
    return {name:[number(p/'thermal_throttle'/name) for p in sorted(root.glob('cpu[0-9]*'))[:256]]
            for name in ('core_throttle_count','package_throttle_count')}


def power_observations(root=Path('/sys/class/powercap')):
    return [{name:number(p/name) for name in ('energy_uj','max_energy_range_uj',
                'constraint_0_power_limit_uw','constraint_1_power_limit_uw')}
            for p in sorted(root.glob('intel-rapl:[0-9]'))[:8]]


class Observer:
    def __init__(self,emit):
        self.emit,self.previous,self.sampled_at=emit,None,None

    def __call__(self,model_pid,helper_pid,port):
        now=time.monotonic()
        if self.sampled_at is not None and now-self.sampled_at<1:return
        self.sampled_at=now
        host=cpu.sample()
        try:own=sum(cpu.process_ticks(pid) for pid in (os.getpid(),model_pid,helper_pid))
        except (OSError,ValueError,IndexError):own=None
        external=None
        if self.previous is not None and own is not None and self.previous[1] is not None:
            dt=host['cpu']['cpu'][0]-self.previous[0]['cpu']['cpu'][0]
            busy=host['cpu']['cpu'][1]-self.previous[0]['cpu']['cpu'][1]
            if dt>0 and own>=self.previous[1] and busy>=0:external=max(0,busy-(own-self.previous[1]))/dt
        self.previous=host,own
        try:hardware=cpu.hardware()
        except (OSError,ValueError):hardware=None
        self.emit({'event':'load','external_busy_fraction':external,'host':host,'hardware':hardware,
                   'slot':read_slots(port),'throttle_counts':throttle_counts(),'power':power_observations()})
