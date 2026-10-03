"""One diagnostic phase, using the shared runner for isolated launch/cleanup.
ProfileFinished stops the ordinary workload sequence after saving this phase.
No data from this diagnostic is presented as an uninstrumented benchmark.
"""
import sys,subprocess,json,os,hashlib
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
output=Path('/tmp/loom-scroll-pc-1003')
bundle=root/'target/main-bench-20261003/loomtty.app/Contents/MacOS'
args=b.argument_parser().parse_args(['--apps','loomtty','--rounds','1','--frames','600','--mib','.25','--loom-width','600','--loom-height','400','--loom',str(bundle/'loomtty'),'--loom-server',str(bundle/'loomtty-server'),'--vtebench',str(root.parent/'vtebench-extras-20261003'),'--confirm-visible','--output',str(output)])
class ProfileFinished(BaseException):pass
original=b.measure_phase

def measure(directory,processes,sequence,command,timeout):
    manifest=json.loads((directory/'vtebench/manifest.json').read_text())
    benchmark=next(x for x in manifest if x['name']=='scrolling')
    command=dict(name='vtebench/scrolling',benchmark=benchmark,directory=str(directory),binary=str(args.vtebench/'target/release/vtebench'),min_bytes=16*b.MIB,max_secs=30,output_mode='original')
    server=next(p for p in processes if p.label=='server')
    b.check_build_interference()
    with (directory/'record.log').open('w') as log:
        profiler=subprocess.Popen(['xcrun','xctrace','record','--template','Time Profiler','--attach',str(server.pid),'--time-limit','20s','--output',str(directory/'server.trace')],stdout=log,stderr=subprocess.STDOUT)
        try:
            result=original(directory,processes,sequence,command,timeout)
            if profiler.wait(timeout=30):raise RuntimeError('Time Profiler failed')
            result.update(diagnostic_only=True,source_commit='d60c18a',manifest=manifest,
                          server_sha256=hashlib.sha256(args.loom_server.read_bytes()).hexdigest(),
                          processes={p.label:p.pid for p in processes})
            b.save_json(directory/'diagnostic.json',result)
        finally:
            if profiler.poll() is None:profiler.terminate();profiler.wait(timeout=10)
    raise ProfileFinished()
b.measure_phase=measure
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:
    try:
        result=b.run_one('loomtty',0,output,args)
        b.save_json(output/'failure.json',result)
        raise SystemExit(1)
    except ProfileFinished:
        print('Diagnostic saved',output,flush=True)
finally:
    wake.terminate();wake.wait()
