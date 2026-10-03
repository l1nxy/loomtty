import sys,os,subprocess,json,time
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
import compare_loom as c
meta=json.loads((root/'bench/results/macos-delta-empty-2026-10-03/binaries.json').read_text())
original=c.run_one

original_check=b.check_build_interference
interference=[]
def diagnostic_check():
    import shlex,datetime
    snapshot=[]
    for row in subprocess.check_output(['ps','-axo','pid=,ppid=,%cpu=,etime=,comm=,args='],text=True,errors='replace').splitlines():
        fields=row.split(None,5)
        if len(fields)<6 or Path(fields[4]).name not in ('cargo','rustc'):continue
        try:arguments=shlex.split(fields[5])
        except ValueError:arguments=[]
        operation=next((a for a in arguments[1:] if a in ('build','check','test','clippy','fmt','metadata','fetch','update','run','doc','rustc','--version','--print')),'unclassified')
        record=dict(pid=int(fields[0]),parent=int(fields[1]),cpu_percent=float(fields[2]),elapsed=fields[3],binary=Path(fields[4]).name,operation=operation)
        if '--crate-name' in arguments:
            index=arguments.index('--crate-name')
            if index+1<len(arguments) and arguments[index+1].replace('_','').isalnum():record['crate']=arguments[index+1]
        snapshot.append(record)
    if snapshot:
        interference.append(dict(time=datetime.datetime.now(datetime.timezone.utc).isoformat(),processes=snapshot))
        b.save_json(Path('/tmp/loom-delta-empty-ab-1003/interference.json'),interference)
        raise RuntimeError('concurrent Cargo/Rustc process: '+str(snapshot))
b.check_build_interference=diagnostic_check


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
sys.argv=['compare_loom.py','--before',str(Path(meta['before_client']).parent),'--loom',meta['after_client'],'--loom-server',meta['common_server'],'--rounds','3','--frames','1200','--loom-width','600','--loom-height','400','--output','/tmp/loom-delta-empty-ab-1003']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:code=c.main()
finally:wake.terminate();wake.wait()
raise SystemExit(code)
