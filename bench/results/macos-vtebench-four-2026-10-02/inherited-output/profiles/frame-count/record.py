import sys, json, subprocess, os
from pathlib import Path
sys.path.insert(0, '/Users/linxy/.codex/worktrees/macos-native/loomtty/bench')
import macos_bench as b
binary='/tmp/Loomtty-Frame-Count.app/Contents/MacOS/loomtty'
original_popen=subprocess.Popen

def popen(command,*args,**kwargs):
    if str(command[0]).endswith('/Loomtty-Frame-Count.app/Contents/MacOS/loomtty'):
        kwargs['env']=dict(kwargs.get('env',os.environ),RUST_LOG='info,loom_gpu=trace')
    return original_popen(command,*args,**kwargs)
subprocess.Popen=popen
original=b.measure_phase

def measure(directory,processes,sequence,command,timeout):
    def count():
        return sum('draw_frame backend=blade' in line for line in (directory/'terminal.log').read_text().splitlines())
    before=count()
    result=original(directory,processes,sequence,command,timeout)
    result['draw_frame_records']=count()-before
    result['diagnostic_only']='Per-frame trace logging enabled; frame counts, not formal CPU timing'
    return result
b.measure_phase=measure
sys.argv=['macos_bench.py','--apps','loomtty','--rounds','1','--mib','.25','--frames','600','--latency-samples','20','--idle-seconds','.5','--confirm-visible','--loom-width','600','--loom-height','400','--loom',binary,'--loom-server','/tmp/loom-history-before/loomtty-server','--output','/tmp/loom-frame-count-2']
raise SystemExit(b.main())
