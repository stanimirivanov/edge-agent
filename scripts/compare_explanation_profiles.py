#!/usr/bin/env python3
"""One same-input thread candidate, never qualification or automatic tuning."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time
import probe_explanation_performance as probe
import profile_explanation as profiling

KIND='research.explanation-profile-comparison.v1'


def check_pair(baseline,candidate,old,new):
    # Paths may differ; byte identities and all semantics except threads may not.
    def identity(manifest):
        return {key:({k:v for k,v in value.items() if k!='path'} if key in
                ('runtime','weights','license','helper','prompt') else value)
                for key,value in manifest.items() if key not in ('threads','profile')}
    def profile(value):return {key:item for key,item in value.items() if key!='deployment'}
    if (baseline['threads']==candidate['threads'] or identity(baseline)!=identity(candidate)
            or profile(old)!=profile(new)):
        raise probe.owned.common.HarnessError('not_a_single_thread_candidate')


def profiles(path,pin):
    probe.owned.common.check_file({'path':str(path),'sha256':pin},32768)
    pair=probe.owned.common.load(path,32768)
    if set(pair)!={'kind','baseline','candidate'} or pair['kind']!=KIND:
        raise probe.owned.common.HarnessError('invalid_comparison')
    result=[];data=[]
    for label in ('baseline','candidate'):
        binding=pair[label];probe.owned.common.check_file(binding,32768)
        source=Path(binding['path']);manifest=probe.owned.common.load(source,32768)
        probe.owned.preflight(manifest,performance=True)
        data.append(probe.owned.common.load(Path(manifest['profile']['path']),2048)['data'])
        result.append((source,binding['sha256'],manifest))
    check_pair(result[0][2],result[1][2],*data)
    return result


def run(path,pin,report,execute,*,exploratory=False):
    if len(pin)!=64 or any(c not in '0123456789abcdef' for c in pin):
        raise probe.owned.common.HarnessError('invalid_comparison_pin')
    if signal.getitimer(signal.ITIMER_REAL)[0]:raise probe.owned.common.HarnessError('existing_timer_refused')
    with probe.cpu.open_report(report) as stream:
        start=time.monotonic()
        def emit(value):probe.cpu.record(stream,dict(value,journal_ms=round((time.monotonic()-start)*1000)))
        emit({'kind':KIND,'pair':pin,'script':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'profiling':hashlib.sha256(Path(profiling.__file__).read_bytes()).hexdigest(),
              'probe':hashlib.sha256(Path(probe.__file__).read_bytes()).hexdigest(),
              'cpu':hashlib.sha256(Path(probe.cpu.__file__).read_bytes()).hexdigest(),
              'exploratory':exploratory,'permission_granted':False,'run_budget_ms':1200000,
              'observation_ceiling_ms':180000,'census':[['baseline','P01'],['candidate','P01']]})
        launched=False;previous=signal.getsignal(signal.SIGALRM)
        def expired(_number,_frame):raise probe.BudgetExpired()
        signal.signal(signal.SIGALRM,expired);signal.setitimer(signal.ITIMER_REAL,1200)
        try:
            if execute!=pin:raise probe.owned.common.HarnessError('execution_opt_in_required')
            pair=profiles(path,pin)
            if not exploratory:
                memory=max(Path(m['weights']['path']).stat().st_size for _,_,m in pair)+2*1024**3
                if not probe.cpu.wait_idle(probe.cpu.sample,time.monotonic,time.sleep,emit,min(start+1200,time.monotonic()+600),memory):
                    emit({'event':'complete','status':'not_run','reason':'idle_wait_expired'});return 2
            identity=None
            for label,(source,digest,manifest) in zip(('baseline','candidate'),pair):
                emit({'event':'attempt_started','profile':label,'manifest':digest});launched=True
                observer=profiling.Observer(lambda value:emit(dict(value,profile=label)))
                value=json.loads(probe.owned.supervise(source,digest,'performance-P01',performance_observer=observer))
                probe.check_attempt(value,'P01')
                current=(value['calibration'],value['input_bytes'])
                if value['profile']!=manifest['profile']['sha256'] or (identity is not None and identity!=current):
                    raise probe.owned.common.HarnessError('comparison_input_changed')
                identity=current
                emit({'event':'attempt_finished','profile':label,'measurement':value})
            emit({'event':'complete','status':'completed','attempts':2});return 0
        except probe.BudgetExpired:
            emit({'event':'complete','status':'incomplete' if launched else 'not_run','reason':'run_budget_exhausted'});return 2
        except (probe.owned.common.HarnessError,OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError,KeyboardInterrupt):
            emit({'event':'complete','status':'incomplete' if launched else 'not_run','reason':'comparison_refused_or_interrupted'});return 2
        finally:
            signal.setitimer(signal.ITIMER_REAL,0);signal.signal(signal.SIGALRM,previous)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pair',type=Path,required=True);parser.add_argument('--pair-sha256',required=True)
    parser.add_argument('--execute-pair');parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--exploratory',action='store_true',help='Skip idle admission; provisional under-load evidence only')
    args=parser.parse_args()
    def cancelled(_number,_frame):raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM,cancelled)
    try:return run(args.pair,args.pair_sha256,args.report,args.execute_pair,exploratory=args.exploratory)
    except (probe.owned.common.HarnessError,OSError):return 2


if __name__=='__main__':raise SystemExit(main())
