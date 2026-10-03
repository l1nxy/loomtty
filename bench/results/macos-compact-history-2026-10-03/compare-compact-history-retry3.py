import sys,os,subprocess,json,time
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
import compare_loom as c
meta=json.loads((root/'bench/results/macos-compact-history-2026-10-03/binaries.json').read_text())
original=c.run_one

def run_one(app,round_index,directory,args):
    deadline=time.monotonic()+180
    quiet=None
    while True:
        if time.monotonic()>deadline:raise RuntimeError('No stable idle interval within 180 seconds')
        try:
            b.check_build_interference()
            if quiet is None:quiet=time.monotonic()
            if time.monotonic()-quiet>=10:break
        except RuntimeError:quiet=None
        time.sleep(2)
    result=original(app,round_index,directory,args)
    result['visibility_confirmation']={'method':'CLI auto-foreground per user instruction; not independently verified by CUA','continuous_visibility_monitoring':False}
    b.save_json(directory/'result.json',result)
    return result
c.run_one=run_one
sys.argv=['compare_loom.py','--before',str(Path(meta['before_client']).parent),'--loom',meta['after_client'],'--loom-server',meta['common_server'],'--rounds','3','--frames','1200','--loom-width','600','--loom-height','400','--output','/tmp/loom-compact-history-ab-r3']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:code=c.main()
finally:wake.terminate();wake.wait()
raise SystemExit(code)
