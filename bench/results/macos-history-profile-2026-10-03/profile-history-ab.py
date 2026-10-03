import sys,subprocess,os,json
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
meta=json.loads((root/'bench/results/macos-compact-history-2026-10-03/direct-binaries.json').read_text())
original=b.measure_phase

def profile(directory,processes,sequence,command,timeout):
    profiler=None;log=None
    if command['name'] in ('tui_full','tui_partial'):
        target=next(p for p in processes if p.label=='client')
        trace=directory/(command['name']+'-client.trace')
        log=(directory/(command['name']+'-client-xctrace.log')).open('w')
        profiler=subprocess.Popen(['xcrun','xctrace','record','--template','Time Profiler','--attach',str(target.pid),'--time-limit','20s','--output',str(trace)],stdout=log,stderr=subprocess.STDOUT)
    try:
        result=original(directory,processes,sequence,command,timeout)
        if profiler:
            code=profiler.wait(timeout=30)
            if code:raise RuntimeError('xctrace recording failed')
        return result
    finally:
        if profiler and profiler.poll() is None:
            profiler.terminate();profiler.wait(timeout=15)
        if log:log.close()

b.measure_phase=profile

import compare_loom as c
import time
original_run=c.run_one
def run_one(app,round_index,directory,args):
    deadline=time.monotonic()+180;quiet=None
    while quiet is None or time.monotonic()-quiet<10:
        if time.monotonic()>deadline:raise RuntimeError('No idle interval')
        try:
            b.check_build_interference()
            if quiet is None:quiet=time.monotonic()
        except RuntimeError:quiet=None
        time.sleep(1)
    return original_run(app,round_index,directory,args)
c.run_one=run_one
sys.argv=['compare_loom.py','--before',str(Path(meta['before_client']).parent),'--rounds','1','--mib','.25','--frames','2400','--latency-samples','20','--idle-seconds','.5','--loom-width','600','--loom-height','400','--loom',meta['after_client'],'--loom-server',meta['common_server'],'--output','/tmp/loom-history-profile-ab-1003']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:code=c.main()
finally:wake.terminate();wake.wait()
raise SystemExit(code)
