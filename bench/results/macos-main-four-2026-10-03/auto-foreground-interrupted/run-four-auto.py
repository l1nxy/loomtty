import sys, os, subprocess, copy, hashlib, time
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
original=b.run_one

def run_one(app, round_index, directory, args):
    quiet_since=None
    reported=False
    deadline=time.monotonic()+180
    while True:
        if time.monotonic()>deadline:raise RuntimeError('No stable idle interval within 180 seconds')
        try:
            b.check_build_interference()
            if quiet_since is None:quiet_since=time.monotonic()
            if time.monotonic()-quiet_since>=30:break
        except RuntimeError:
            quiet_since=None
            if not reported:
                print('  Waiting for unrelated Rust builds to finish before launch',flush=True)
                reported=True
        time.sleep(2)
    local=copy.copy(args)
    local.confirm_visible=False
    result=original(app,round_index,directory,local)
    result['automation_runner_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    result['visibility_confirmation']={
        'method':'CLI launch auto-foreground, as reported and explicitly requested by user; not independently verified by CUA',
        'continuous_visibility_monitoring':False}
    if app=='loomtty':result['tested_source_commit']='d60c18a'
    b.save_json(directory/'result.json',result)
    if 'error' in result:
        failure=directory.parent/'controller-failure.json'
        b.save_json(failure,{'app':app,'round':round_index,'result_directory':str(directory),'error':result['error']})
        raise RuntimeError(f"{app}: {result['error']}; result retained in {directory}")
    return result
b.run_one=run_one
bundle=root/'target/main-bench-20261003/loomtty.app/Contents/MacOS'
sys.argv=['macos_bench.py','--rounds','3','--frames','600','--loom',str(bundle/'loomtty'),'--loom-server',str(bundle/'loomtty-server'),'--loom-width','600','--loom-height','400','--vtebench',str(root.parent/'vtebench-extras-20261003'),'--vtebench-seconds','3','--output','/tmp/loom-four-auto-1003d']
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:
    code=b.main()
finally:
    wake.terminate();wake.wait()
raise SystemExit(code)
