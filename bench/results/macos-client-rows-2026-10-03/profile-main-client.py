import sys,subprocess,os,json
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
meta=json.loads((root/'bench/results/macos-scroll-one-probe-2026-10-03/binaries-v3.json').read_text())
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
sys.argv=['macos_bench.py','--apps','loomtty','--rounds','1','--mib','.25','--frames','2400','--latency-samples','20','--idle-seconds','.5','--loom-width','600','--loom-height','400','--loom',meta['common_client'],'--loom-server',meta['baseline_server'],'--output','/tmp/loom-client-profile-1003']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:code=b.main()
finally:wake.terminate();wake.wait()
raise SystemExit(code)
