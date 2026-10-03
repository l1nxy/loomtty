import sys,os,subprocess,json,time
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
import compare_loom as c
meta=json.loads((root/'bench/results/macos-combined-cpu-memory-2026-10-04/binaries.json').read_text())
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
        b.save_json(Path('/tmp/loom-combined-controlled-resume-ab-1004/interference.json'),interference)
        raise RuntimeError('concurrent Cargo/Rustc process: '+str(snapshot))
b.check_build_interference=diagnostic_check


def run_one(app,round_index,directory,args):
    if round_index == 0:
        import shutil
        source=Path("/tmp/loom-combined-controlled-ab-1004")/directory.name
        shutil.copytree(source,directory)
        result=json.loads((directory/"result.json").read_text())
        result["resumed_from"]=str(source)
        return result
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
    result['continuous_visibility_monitoring']=False
    b.save_json(directory/'result.json',result)
    return result
c.run_one=run_one
original_wait=b.wait_file
output=Path('/tmp/loom-combined-controlled-resume-ab-1004')
def foreground_gate(directory,ack,pid,phase):
    app=meta['after_client'] if directory.name.endswith('-after') else meta['before_client']
    request=dict(gate_id=directory.name+'/'+phase,ack=str(ack),client_pid=pid,app_path=str(Path(app).parents[2]),phase=phase)
    b.save_json(output/'foreground-request.json',request)
    response=original_wait(ack,timeout=1200)
    if response.get('client_pid')!=pid:raise RuntimeError('Foreground acknowledgement PID mismatch')
    (output/'foreground-request.json').unlink(missing_ok=True)
    time.sleep(1)
    return response

def wait_file(path,timeout=40):
    if path.name=='visible.json':
        state=json.loads((path.parent/'needs-visibility.json').read_text())
        return foreground_gate(path.parent,path,state['processes']['client'],'launch')
    return original_wait(path,timeout=timeout)
b.wait_file=wait_file
original_measure=b.measure_phase
def measure(directory,processes,sequence,command,timeout):
    response=None
    if command['name'] in ('tui_full','tui_partial'):
        pid=next(p.pid for p in processes if p.label=='client')
        response=foreground_gate(directory,directory/('foreground-'+str(sequence)+'.json'),pid,command['name'])
    result=original_measure(directory,processes,sequence,command,timeout)
    if response:result['foreground_gate']=response
    return result
b.measure_phase=measure
sys.argv=['compare_loom.py','--before',str(Path(meta['before_client']).parent),'--loom',meta['after_client'],'--loom-server',meta['common_server'],'--confirm-visible','--rounds','3','--frames','1200','--loom-width','600','--loom-height','400','--output','/tmp/loom-combined-controlled-resume-ab-1004']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:code=c.main()
finally:wake.terminate();wake.wait()
raise SystemExit(code)
